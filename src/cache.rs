use anyhow::Result;
use rusqlite::{params, Connection};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::api::{BrowseItem, FolderStatus, SyncState};

fn log_debug(msg: &str) {
    // Only log if debug mode is enabled
    if !crate::DEBUG_MODE.load(Ordering::Relaxed) {
        return;
    }

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/synctui-debug.log")
    {
        let _ = writeln!(file, "{}", msg);
    }
}

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn new() -> Result<Self> {
        let cache_dir = Self::get_cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;

        let db_path = cache_dir.join("cache.db");
        let conn = Connection::open(db_path)?;

        // Enable Write-Ahead Logging for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        let mut cache = CacheDb { conn };
        cache.init_schema()?;

        Ok(cache)
    }

    fn get_cache_dir() -> Result<PathBuf> {
        if let Some(cache_dir) = dirs::cache_dir() {
            Ok(cache_dir.join("synctui"))
        } else {
            // Fallback to /tmp if no cache dir available
            Ok(PathBuf::from("/tmp/synctui-cache"))
        }
    }

    fn init_schema(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS folder_status (
                folder_id TEXT PRIMARY KEY,
                sequence INTEGER NOT NULL,
                state TEXT NOT NULL,
                need_total_items INTEGER NOT NULL,
                receive_only_total_items INTEGER NOT NULL,
                global_bytes INTEGER NOT NULL,
                local_bytes INTEGER NOT NULL,
                need_bytes INTEGER NOT NULL,
                receive_only_changed_bytes INTEGER NOT NULL,
                global_total_items INTEGER NOT NULL DEFAULT 0,
                local_files INTEGER NOT NULL DEFAULT 0,
                local_directories INTEGER NOT NULL DEFAULT 0,
                global_files INTEGER NOT NULL DEFAULT 0,
                global_directories INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS browse_cache (
                folder_id TEXT NOT NULL,
                folder_sequence INTEGER NOT NULL,
                prefix TEXT,
                name TEXT NOT NULL,
                item_type TEXT NOT NULL,
                mod_time TEXT NOT NULL DEFAULT '',
                size INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (folder_id, prefix, name)
            ) WITHOUT ROWID;

            CREATE TABLE IF NOT EXISTS sync_states (
                folder_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_sequence INTEGER NOT NULL,
                sync_state TEXT NOT NULL,
                PRIMARY KEY (folder_id, file_path)
            ) WITHOUT ROWID;

            CREATE TABLE IF NOT EXISTS event_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_event_id INTEGER NOT NULL DEFAULT 0
            );

            -- Insert default row if it doesn't exist
            INSERT OR IGNORE INTO event_state (id, last_event_id) VALUES (1, 0);
            ",
        )?;

        Ok(())
    }

    // Folder status caching
    pub fn get_folder_status(&self, folder_id: &str) -> Result<Option<FolderStatus>> {
        let mut stmt = self.conn.prepare(
            "SELECT state, need_total_items, receive_only_total_items, global_bytes,
                    local_bytes, need_bytes, receive_only_changed_bytes, global_total_items,
                    local_files, local_directories, global_files, global_directories
             FROM folder_status WHERE folder_id = ?1",
        )?;

        let result = stmt.query_row(params![folder_id], |row| {
            Ok(FolderStatus {
                state: row.get(0)?,
                sequence: 0, // We don't return sequence from cache, will be checked separately
                need_total_items: row.get(1)?,
                receive_only_total_items: row.get(2)?,
                global_bytes: row.get(3)?,
                local_bytes: row.get(4)?,
                need_bytes: row.get(5)?,
                receive_only_changed_bytes: row.get(6)?,
                global_total_items: row.get(7)?,
                local_files: row.get(8)?,
                local_directories: row.get(9)?,
                global_files: row.get(10)?,
                global_directories: row.get(11)?,
                // Other fields with default values
                global_deleted: 0,
                global_symlinks: 0,
                in_sync_bytes: 0,
                in_sync_files: 0,
                local_deleted: 0,
                local_symlinks: 0,
                local_total_items: 0,
                need_deletes: 0,
                need_directories: 0,
                need_files: 0,
                need_symlinks: 0,
                receive_only_changed_deletes: 0,
                receive_only_changed_directories: 0,
                receive_only_changed_files: 0,
                receive_only_changed_symlinks: 0,
            })
        });

        match result {
            Ok(status) => Ok(Some(status)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_folder_status(
        &self,
        folder_id: &str,
        status: &FolderStatus,
        sequence: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO folder_status
             (folder_id, sequence, state, need_total_items, receive_only_total_items,
              global_bytes, local_bytes, need_bytes, receive_only_changed_bytes, global_total_items,
              local_files, local_directories, global_files, global_directories)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                folder_id,
                sequence as i64,
                &status.state,
                status.need_total_items as i64,
                status.receive_only_total_items as i64,
                status.global_bytes as i64,
                status.local_bytes as i64,
                status.need_bytes as i64,
                status.receive_only_changed_bytes as i64,
                status.global_total_items as i64,
                status.local_files as i64,
                status.local_directories as i64,
                status.global_files as i64,
                status.global_directories as i64,
            ],
        )?;

        Ok(())
    }

    pub fn invalidate_folder_status(&self, folder_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM folder_status WHERE folder_id = ?1",
            params![folder_id],
        )?;
        Ok(())
    }

    // Browse cache
    pub fn get_browse_items(
        &self,
        folder_id: &str,
        prefix: Option<&str>,
        folder_sequence: u64,
    ) -> Result<Option<Vec<BrowseItem>>> {
        // Convert None to empty string for PRIMARY KEY compatibility
        let prefix_str = prefix.unwrap_or("");

        // Check if cache is valid first
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT folder_sequence FROM browse_cache
             WHERE folder_id = ?1 AND prefix = ?2 LIMIT 1",
        )?;

        let cached_seq: Option<i64> = stmt
            .query_row(params![folder_id, prefix_str], |row| row.get(0))
            .ok();

        log_debug(&format!(
            "DEBUG [get_browse_items]: folder={} prefix={:?} requested_seq={} cached_seq={:?}",
            folder_id, prefix, folder_sequence, cached_seq
        ));

        if cached_seq.map_or(false, |seq| seq as u64 != folder_sequence) || cached_seq.is_none() {
            log_debug(&format!(
                "DEBUG [get_browse_items]: Cache MISS - returning None"
            ));
            return Ok(None); // Cache is stale or doesn't exist
        }

        log_debug(&format!(
            "DEBUG [get_browse_items]: Cache HIT - fetching items"
        ));

        // Fetch cached items
        let mut stmt = self.conn.prepare(
            "SELECT name, item_type, mod_time, size FROM browse_cache
             WHERE folder_id = ?1 AND prefix = ?2",
        )?;

        let items = stmt
            .query_map(params![folder_id, prefix_str], |row| {
                Ok(BrowseItem {
                    name: row.get(0)?,
                    item_type: row.get(1)?,
                    mod_time: row.get(2)?,
                    size: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(items))
    }

    /// Get all browse items for a folder (all prefixes) for recursive search
    ///
    /// Returns a list of (full_path, BrowseItem) tuples for all cached items in the folder.
    /// Only returns items from cache if folder_sequence matches.
    pub fn get_all_browse_items(
        &self,
        folder_id: &str,
        folder_sequence: u64,
    ) -> Result<Vec<(String, BrowseItem)>> {
        // Check if any cache exists for this folder with the correct sequence
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT folder_sequence FROM browse_cache
             WHERE folder_id = ?1 LIMIT 1",
        )?;

        let cached_seq: Option<i64> = stmt
            .query_row(params![folder_id], |row| row.get(0))
            .ok();

        if cached_seq.map_or(true, |seq| seq as u64 != folder_sequence) {
            // Cache is stale or doesn't exist
            return Ok(Vec::new());
        }

        // Fetch all cached items with their prefixes
        let mut stmt = self.conn.prepare(
            "SELECT prefix, name, item_type, mod_time, size FROM browse_cache
             WHERE folder_id = ?1 AND folder_sequence = ?2",
        )?;

        let items = stmt
            .query_map(params![folder_id, folder_sequence as i64], |row| {
                let prefix: String = row.get(0)?;
                let name: String = row.get(1)?;
                let item = BrowseItem {
                    name: name.clone(),
                    item_type: row.get(2)?,
                    mod_time: row.get(3)?,
                    size: row.get(4)?,
                };

                // Build full path
                let full_path = if prefix.is_empty() {
                    name
                } else {
                    format!("{}/{}", prefix, name)
                };

                Ok((full_path, item))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(items)
    }

    pub fn save_browse_items(
        &self,
        folder_id: &str,
        prefix: Option<&str>,
        items: &[BrowseItem],
        folder_sequence: u64,
    ) -> Result<()> {
        // Convert None to empty string for PRIMARY KEY compatibility
        let prefix_str = prefix.unwrap_or("");

        log_debug(&format!(
            "DEBUG [save_browse_items]: folder={} prefix={:?} seq={} item_count={}",
            folder_id,
            prefix,
            folder_sequence,
            items.len()
        ));

        // Use a transaction for better performance with many items
        let tx = self.conn.unchecked_transaction()?;

        // Delete old entries for this folder/prefix
        let deleted = tx.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix = ?2",
            params![folder_id, prefix_str],
        )?;
        log_debug(&format!(
            "DEBUG [save_browse_items]: Deleted {} old entries",
            deleted
        ));

        // Insert new entries
        {
            let mut stmt = tx.prepare(
                "INSERT INTO browse_cache (folder_id, folder_sequence, prefix, name, item_type, mod_time, size)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            )?;

            log_debug(&format!(
                "DEBUG [save_browse_items]: Starting insert loop for {} items",
                items.len()
            ));
            for (idx, item) in items.iter().enumerate() {
                match stmt.execute(params![
                    folder_id,
                    folder_sequence as i64,
                    prefix_str,
                    &item.name,
                    &item.item_type,
                    &item.mod_time,
                    item.size as i64,
                ]) {
                    Ok(_) => {}
                    Err(e) => {
                        log_debug(&format!("DEBUG [save_browse_items]: Insert failed at item {}: {} (name={}, type={})",
                                           idx, e, item.name, item.item_type));
                        return Err(e.into());
                    }
                }
            }
            log_debug(&format!("DEBUG [save_browse_items]: All inserts completed"));
        }

        log_debug(&format!(
            "DEBUG [save_browse_items]: Committing transaction"
        ));
        tx.commit()?;
        log_debug(&format!(
            "DEBUG [save_browse_items]: Successfully saved {} items",
            items.len()
        ));
        Ok(())
    }

    // Get cached sync state without validation (for initial load)
    pub fn get_sync_state_unvalidated(
        &self,
        folder_id: &str,
        file_path: &str,
    ) -> Result<Option<SyncState>> {
        let mut stmt = self.conn.prepare(
            "SELECT sync_state FROM sync_states
             WHERE folder_id = ?1 AND file_path = ?2",
        )?;

        let result = stmt.query_row(params![folder_id, file_path], |row| {
            let state_str: String = row.get(0)?;
            Ok(Self::parse_sync_state(&state_str))
        });

        match result {
            Ok(state) => Ok(Some(state)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_sync_state(
        &self,
        folder_id: &str,
        file_path: &str,
        state: SyncState,
        file_sequence: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_states (folder_id, file_path, file_sequence, sync_state)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                folder_id,
                file_path,
                file_sequence as i64,
                Self::serialize_sync_state(state)
            ],
        )?;

        Ok(())
    }

    /// Save multiple sync states in a single transaction for better performance
    pub fn save_sync_states_batch(
        &self,
        states: &[(String, String, SyncState, u64)], // (folder_id, file_path, state, sequence)
    ) -> Result<()> {
        if states.is_empty() {
            return Ok(());
        }

        log_debug(&format!(
            "DEBUG [save_sync_states_batch]: Saving {} states in batch",
            states.len()
        ));

        let start = std::time::Instant::now();
        let tx = self.conn.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO sync_states (folder_id, file_path, file_sequence, sync_state)
                 VALUES (?1, ?2, ?3, ?4)"
            )?;

            for (folder_id, file_path, state, seq) in states {
                stmt.execute(params![
                    folder_id,
                    file_path,
                    *seq as i64,
                    Self::serialize_sync_state(*state)
                ])?;
            }
        } // stmt is dropped here

        tx.commit()?;
        let elapsed = start.elapsed();
        log_debug(&format!(
            "DEBUG [save_sync_states_batch]: Flushed {} sync states in {:?}",
            states.len(),
            elapsed
        ));

        Ok(())
    }

    fn serialize_sync_state(state: SyncState) -> String {
        match state {
            SyncState::Synced => "Synced".to_string(),
            SyncState::OutOfSync => "OutOfSync".to_string(),
            SyncState::LocalOnly => "LocalOnly".to_string(),
            SyncState::RemoteOnly => "RemoteOnly".to_string(),
            SyncState::Ignored => "Ignored".to_string(),
            SyncState::Syncing => "Syncing".to_string(),
            SyncState::Unknown => "Unknown".to_string(),
        }
    }

    fn parse_sync_state(s: &str) -> SyncState {
        match s {
            "Synced" => SyncState::Synced,
            "OutOfSync" => SyncState::OutOfSync,
            "LocalOnly" => SyncState::LocalOnly,
            "RemoteOnly" => SyncState::RemoteOnly,
            "Ignored" => SyncState::Ignored,
            // Syncing is transient - never persist across app restarts
            "Syncing" => {
                log_debug("WARNING: Found stale 'Syncing' state in cache, converting to Unknown");
                SyncState::Unknown
            }
            _ => SyncState::Unknown,
        }
    }

    // Invalidate all cache for a folder when sequence changes
    pub fn invalidate_folder(&self, folder_id: &str) -> Result<()> {
        log_debug(&format!(
            "DEBUG [invalidate_folder]: Invalidating all cache for folder={}",
            folder_id
        ));
        let browse_deleted = self.conn.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1",
            params![folder_id],
        )?;
        let sync_deleted = self.conn.execute(
            "DELETE FROM sync_states WHERE folder_id = ?1",
            params![folder_id],
        )?;
        log_debug(&format!(
            "DEBUG [invalidate_folder]: Deleted {} browse entries, {} sync entries",
            browse_deleted, sync_deleted
        ));
        Ok(())
    }

    // Invalidate cache for a single file
    pub fn invalidate_single_file(&self, folder_id: &str, file_path: &str) -> Result<()> {
        log_debug(&format!(
            "DEBUG [invalidate_single_file]: folder={} file={}",
            folder_id, file_path
        ));

        // Delete sync state for this file
        let sync_deleted = self.conn.execute(
            "DELETE FROM sync_states WHERE folder_id = ?1 AND file_path = ?2",
            params![folder_id, file_path],
        )?;

        // Also invalidate browse cache for the parent directory
        // Extract parent directory path
        let parent_dir = if let Some(last_slash) = file_path.rfind('/') {
            &file_path[..last_slash + 1] // Include trailing slash
        } else {
            "" // File is in root directory
        };

        let browse_deleted = self.conn.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix = ?2",
            params![folder_id, parent_dir],
        )?;

        log_debug(&format!("DEBUG [invalidate_single_file]: Deleted {} sync state entries, {} browse entries for parent dir '{}'", sync_deleted, browse_deleted, parent_dir));
        Ok(())
    }

    // Invalidate cache for a directory and all its contents
    pub fn invalidate_directory(&self, folder_id: &str, dir_path: &str) -> Result<()> {
        log_debug(&format!(
            "DEBUG [invalidate_directory]: folder={} dir={}",
            folder_id, dir_path
        ));

        // Normalize directory path - ensure it ends with /
        let normalized_dir = if dir_path.is_empty() {
            String::new()
        } else if dir_path.ends_with('/') {
            dir_path.to_string()
        } else {
            format!("{}/", dir_path)
        };

        // Delete browse cache for this exact directory
        let browse_deleted = self.conn.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix = ?2",
            params![folder_id, &normalized_dir],
        )?;

        // Delete sync states for all files in this directory and subdirectories
        // Use LIKE with pattern matching: "dir/%" matches "dir/file" and "dir/subdir/file"
        let pattern = if normalized_dir.is_empty() {
            "%".to_string() // Root directory - match everything
        } else {
            format!("{}%", normalized_dir)
        };

        let sync_deleted = self.conn.execute(
            "DELETE FROM sync_states WHERE folder_id = ?1 AND file_path LIKE ?2",
            params![folder_id, &pattern],
        )?;

        log_debug(&format!(
            "DEBUG [invalidate_directory]: Deleted {} browse entries, {} sync entries",
            browse_deleted, sync_deleted
        ));
        Ok(())
    }

    // Event ID persistence
    pub fn get_last_event_id(&self) -> Result<u64> {
        let mut stmt = self
            .conn
            .prepare("SELECT last_event_id FROM event_state WHERE id = 1")?;
        let event_id: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(event_id as u64)
    }

    pub fn save_last_event_id(&self, event_id: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE event_state SET last_event_id = ?1 WHERE id = 1",
            params![event_id as i64],
        )?;
        Ok(())
    }
}
