use anyhow::Result;
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::api::{BrowseItem, FolderStatus, NeedResponse, SyncState};
use crate::model::types::FolderSyncBreakdown;
use crate::utils;

fn log_debug(msg: &str) {
    // Only log if debug mode is enabled
    if !crate::DEBUG_MODE.load(Ordering::Relaxed) {
        return;
    }

    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(utils::get_debug_log_path())
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
        cache.ensure_out_of_sync_columns()?;
        cache.ensure_local_changed_columns()?;

        Ok(cache)
    }

    /// Create an in-memory cache for testing
    #[allow(dead_code)]
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut cache = CacheDb { conn };
        cache.init_schema()?;
        cache.ensure_out_of_sync_columns()?;
        cache.ensure_local_changed_columns()?;
        Ok(cache)
    }

    fn get_cache_dir() -> Result<PathBuf> {
        if let Some(cache_dir) = dirs::cache_dir() {
            Ok(cache_dir.join("stui"))
        } else {
            // Fallback to platform-specific temp dir if no cache dir available
            Ok(utils::get_cache_fallback_path())
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
                last_event_id INTEGER NOT NULL DEFAULT 0,
                device_name TEXT
            );

            CREATE TABLE IF NOT EXISTS cached_folders (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                data TEXT NOT NULL
            );

            -- Insert default row if it doesn't exist
            INSERT OR IGNORE INTO event_state (id, last_event_id) VALUES (1, 0);
            ",
        )?;

        Ok(())
    }

    fn ensure_out_of_sync_columns(&self) -> Result<()> {
        // Check if columns exist
        let has_columns: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sync_states') WHERE name IN ('need_category', 'need_cached_at')",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count == 2)
            },
        )?;

        if !has_columns {
            self.conn
                .execute("ALTER TABLE sync_states ADD COLUMN need_category TEXT", [])?;
            self.conn.execute(
                "ALTER TABLE sync_states ADD COLUMN need_cached_at INTEGER",
                [],
            )?;
        }

        Ok(())
    }

    fn ensure_local_changed_columns(&self) -> Result<()> {
        // Check if columns exist
        let has_columns: bool = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sync_states')
             WHERE name IN ('local_changed', 'local_cached_at')",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count == 2)
            },
        )?;

        if !has_columns {
            self.conn.execute(
                "ALTER TABLE sync_states ADD COLUMN local_changed INTEGER DEFAULT 0",
                [],
            )?;
            self.conn.execute(
                "ALTER TABLE sync_states ADD COLUMN local_cached_at INTEGER",
                [],
            )?;
        }

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
            let cached_state: String = row.get(0)?;
            // Don't restore transient states like "scanning" from cache - default to "idle"
            let state = if cached_state == "scanning" || cached_state == "syncing" {
                "idle".to_string()
            } else {
                cached_state
            };

            Ok(FolderStatus {
                state,
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

        // If folder_sequence is 0, skip validation and use whatever is cached (offline mode)
        if folder_sequence != 0
            && (cached_seq.map_or(false, |seq| seq as u64 != folder_sequence)
                || cached_seq.is_none())
        {
            log_debug(&format!(
                "DEBUG [get_browse_items]: Cache MISS - returning None"
            ));
            return Ok(None); // Cache is stale or doesn't exist
        }

        // If we have no cached data at all, return None
        if cached_seq.is_none() {
            log_debug(&format!(
                "DEBUG [get_browse_items]: No cache exists - returning None"
            ));
            return Ok(None);
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

        let cached_seq: Option<i64> = stmt.query_row(params![folder_id], |row| row.get(0)).ok();

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

        // CRITICAL FIX: Check if folder sequence changed from cached data
        // If so, delete ALL old cached entries to prevent cache inconsistency
        let existing_seq: Option<i64> = tx
            .query_row(
                "SELECT DISTINCT folder_sequence FROM browse_cache
                 WHERE folder_id = ?1 LIMIT 1",
                params![folder_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(old_seq) = existing_seq {
            if old_seq as u64 != folder_sequence {
                // Sequence changed - delete ALL old cached data for this folder
                let cleared = tx.execute(
                    "DELETE FROM browse_cache WHERE folder_id = ?1",
                    params![folder_id],
                )?;
                log_debug(&format!(
                    "DEBUG [save_browse_items]: Sequence changed ({} -> {}), cleared {} entries for entire folder",
                    old_seq, folder_sequence, cleared
                ));
            }
        }

        // Delete old entries for this specific folder/prefix (in case of same-sequence update)
        let deleted = tx.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix = ?2",
            params![folder_id, prefix_str],
        )?;
        log_debug(&format!(
            "DEBUG [save_browse_items]: Deleted {} old entries for prefix {:?}",
            deleted, prefix
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

    /// Save sync state for a single file
    ///
    /// IMPORTANT: Uses UPSERT to preserve need_category and need_cached_at columns
    pub fn save_sync_state(
        &self,
        folder_id: &str,
        file_path: &str,
        state: SyncState,
        file_sequence: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_states (folder_id, file_path, file_sequence, sync_state, need_category, need_cached_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL)
             ON CONFLICT(folder_id, file_path)
             DO UPDATE SET
                file_sequence = excluded.file_sequence,
                sync_state = excluded.sync_state
                -- Explicitly preserve need_category and need_cached_at by not updating them",
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
    ///
    /// IMPORTANT: This uses UPSERT to preserve need_category and need_cached_at columns
    /// when updating existing rows. This prevents race conditions where cache_needed_files()
    /// sets need_category, then save_sync_states_batch() clears it.
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
            // Use INSERT ... ON CONFLICT to preserve need_category and need_cached_at
            let mut stmt = tx.prepare(
                "INSERT INTO sync_states (folder_id, file_path, file_sequence, sync_state, need_category, need_cached_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL)
                 ON CONFLICT(folder_id, file_path)
                 DO UPDATE SET
                    file_sequence = excluded.file_sequence,
                    sync_state = excluded.sync_state
                    -- Explicitly preserve need_category and need_cached_at by not updating them"
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

        // Delete browse cache
        // When dir_path is empty (entire folder invalidation), delete ALL prefixes
        // When dir_path is specific, delete only that exact prefix
        let browse_deleted = if normalized_dir.is_empty() {
            // Delete ALL browse cache entries for this folder (all prefixes)
            self.conn.execute(
                "DELETE FROM browse_cache WHERE folder_id = ?1",
                params![folder_id],
            )?
        } else {
            // Delete only the specific prefix
            self.conn.execute(
                "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix = ?2",
                params![folder_id, &normalized_dir],
            )?
        };

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

    // Folder caching for graceful degradation
    /// Save folders to cache (for graceful degradation on startup)
    pub fn save_folders(&self, folders: &[crate::api::Folder]) -> Result<()> {
        let json = serde_json::to_string(folders)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO cached_folders (id, data) VALUES (1, ?1)",
            params![json],
        )?;

        log_debug(&format!(
            "DEBUG [save_folders]: Cached {} folders",
            folders.len()
        ));

        Ok(())
    }

    /// Get all cached folders
    pub fn get_all_folders(&self) -> Result<Vec<crate::api::Folder>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM cached_folders WHERE id = 1")?;

        let json: String = stmt.query_row([], |row| row.get(0))?;
        let folders: Vec<crate::api::Folder> = serde_json::from_str(&json)?;

        log_debug(&format!(
            "DEBUG [get_all_folders]: Loaded {} folders from cache",
            folders.len()
        ));

        Ok(folders)
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

    pub fn cache_needed_files(&self, folder_id: &str, need_response: &NeedResponse) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Process progress array (downloading)
        for file in &need_response.progress {
            self.conn.execute(
                "INSERT OR REPLACE INTO sync_states
                 (folder_id, file_path, file_sequence, sync_state, need_category, need_cached_at)
                 VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                params![folder_id, &file.name, "Syncing", "downloading", now],
            )?;
        }

        // Process queued array
        for file in &need_response.queued {
            self.conn.execute(
                "INSERT OR REPLACE INTO sync_states
                 (folder_id, file_path, file_sequence, sync_state, need_category, need_cached_at)
                 VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                params![folder_id, &file.name, "RemoteOnly", "queued", now],
            )?;
        }

        // Process rest array (categorize as remote_only or modified based on local state)
        for file in &need_response.rest {
            // For now, mark as remote_only (we'll refine categorization later)
            self.conn.execute(
                "INSERT OR REPLACE INTO sync_states
                 (folder_id, file_path, file_sequence, sync_state, need_category, need_cached_at)
                 VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                params![folder_id, &file.name, "RemoteOnly", "remote_only", now],
            )?;
        }

        Ok(())
    }

    pub fn cache_local_changed_files(&self, folder_id: &str, file_paths: &[String]) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Clear all local_changed flags for this folder first
        self.conn.execute(
            "UPDATE sync_states
             SET local_changed = 0, local_cached_at = NULL
             WHERE folder_id = ?1",
            params![folder_id],
        )?;

        // Set local_changed flag for provided files
        for file_path in file_paths {
            self.conn.execute(
                "INSERT INTO sync_states (folder_id, file_path, file_sequence, sync_state, local_changed, local_cached_at)
                 VALUES (?1, ?2, 0, 'LocalOnly', 1, ?3)
                 ON CONFLICT(folder_id, file_path) DO UPDATE SET
                     local_changed = 1,
                     local_cached_at = ?3",
                params![folder_id, file_path, now],
            )?;
        }

        Ok(())
    }

    pub fn get_folder_sync_breakdown(&self, folder_id: &str) -> Result<FolderSyncBreakdown> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let ttl = 30; // 30 seconds
        let cutoff = now - ttl;

        let mut breakdown = FolderSyncBreakdown::default();

        // First, get counts from need_category (remote changes from /rest/db/need)
        let mut stmt = self.conn.prepare(
            "SELECT need_category, COUNT(*)
             FROM sync_states
             WHERE folder_id = ?1
               AND need_category IS NOT NULL
               AND need_cached_at > ?2
             GROUP BY need_category",
        )?;

        let rows = stmt.query_map(params![folder_id, cutoff], |row| {
            let category: String = row.get(0)?;
            let count: usize = row.get(1)?;
            Ok((category, count))
        })?;

        for row in rows {
            let (category, count) = row?;
            match category.as_str() {
                "downloading" => breakdown.downloading = count,
                "queued" => breakdown.queued = count,
                "remote_only" => breakdown.remote_only = count,
                "modified" => breakdown.modified = count,
                "local_only" => breakdown.local_only = count,
                _ => {}
            }
        }

        // Also count files with sync_state = 'LocalOnly' (local changes not in /rest/db/need)
        let local_only_count: usize = self.conn.query_row(
            "SELECT COUNT(*)
             FROM sync_states
             WHERE folder_id = ?1
               AND sync_state = 'LocalOnly'",
            params![folder_id],
            |row| row.get(0),
        )?;

        breakdown.local_only += local_only_count;

        Ok(breakdown)
    }

    pub fn invalidate_out_of_sync_categories(&self, folder_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sync_states
             SET need_category = NULL, need_cached_at = NULL
             WHERE folder_id = ?1",
            params![folder_id],
        )?;
        Ok(())
    }

    /// Get all out-of-sync items for a folder (items with need_category set)
    /// Returns map of file_path -> category
    pub fn get_out_of_sync_items(&self, folder_id: &str) -> Result<HashMap<String, String>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let ttl = 30; // 30 seconds
        let cutoff = now - ttl;

        let mut stmt = self.conn.prepare(
            "SELECT file_path, need_category
             FROM sync_states
             WHERE folder_id = ?1
               AND need_category IS NOT NULL
               AND need_cached_at > ?2",
        )?;

        let rows = stmt.query_map(params![folder_id, cutoff], |row| {
            let file_path: String = row.get(0)?;
            let category: String = row.get(1)?;
            Ok((file_path, category))
        })?;

        let mut items = HashMap::new();
        for row in rows {
            let (path, category) = row?;
            items.insert(path, category);
        }

        Ok(items)
    }

    pub fn get_local_changed_items(&self, folder_id: &str) -> Result<Vec<String>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        let ttl = 30; // 30 seconds
        let cutoff = now - ttl;

        let mut stmt = self.conn.prepare(
            "SELECT file_path
             FROM sync_states
             WHERE folder_id = ?1
               AND local_changed = 1
               AND local_cached_at > ?2",
        )?;

        let rows = stmt.query_map(params![folder_id, cutoff], |row| {
            let file_path: String = row.get(0)?;
            Ok(file_path)
        })?;

        let mut items = Vec::new();
        for row in rows {
            items.push(row?);
        }

        Ok(items)
    }

    pub fn invalidate_local_changed(&self, folder_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sync_states
             SET local_changed = 0, local_cached_at = NULL
             WHERE folder_id = ?1",
            params![folder_id],
        )?;
        Ok(())
    }

    pub fn get_device_name(&self) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT device_name FROM event_state WHERE id = 1")?;
        let device_name: Option<String> = stmt.query_row([], |row| row.get(0))?;
        Ok(device_name)
    }

    pub fn save_device_name(&self, device_name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE event_state SET device_name = ?1 WHERE id = 1",
            params![device_name],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{FileInfo, NeedResponse};

    #[test]
    fn test_cache_needed_files_stores_categories() {
        let cache = CacheDb::new_in_memory().unwrap();

        let need_response = NeedResponse {
            progress: vec![FileInfo {
                name: "downloading.txt".to_string(),
                size: 100,
                ..Default::default()
            }],
            queued: vec![FileInfo {
                name: "queued.txt".to_string(),
                size: 200,
                ..Default::default()
            }],
            rest: vec![],
            page: 1,
            perpage: 100,
        };

        cache
            .cache_needed_files("test-folder", &need_response)
            .unwrap();

        // Verify categories were stored
        // (We'll implement get_folder_sync_breakdown next to verify)
    }

    #[test]
    fn test_save_sync_states_batch_preserves_need_category() {
        let cache = CacheDb::new_in_memory().unwrap();

        // First, cache needed files (sets need_category and need_cached_at)
        let need_response = NeedResponse {
            progress: vec![],
            queued: vec![],
            rest: vec![
                FileInfo {
                    name: "status-test".to_string(),
                    ..Default::default()
                },
                FileInfo {
                    name: "test".to_string(),
                    ..Default::default()
                },
            ],
            page: 1,
            perpage: 100,
        };

        cache
            .cache_needed_files("test-folder", &need_response)
            .unwrap();

        // Verify need_category was set
        let items = cache.get_out_of_sync_items("test-folder").unwrap();
        assert_eq!(items.len(), 2, "Should have 2 items with need_category");
        assert_eq!(items.get("status-test"), Some(&"remote_only".to_string()));
        assert_eq!(items.get("test"), Some(&"remote_only".to_string()));

        // Now simulate save_sync_states_batch being called (like from pending writes flush)
        let batch = vec![
            (
                "test-folder".to_string(),
                "status-test".to_string(),
                SyncState::OutOfSync,
                100,
            ),
            (
                "test-folder".to_string(),
                "test".to_string(),
                SyncState::OutOfSync,
                100,
            ),
        ];

        cache.save_sync_states_batch(&batch).unwrap();

        // BUG: need_category should still be set, but it gets cleared!
        let items_after = cache.get_out_of_sync_items("test-folder").unwrap();
        assert_eq!(
            items_after.len(),
            2,
            "Should STILL have 2 items with need_category after batch save"
        );
        assert_eq!(
            items_after.get("status-test"),
            Some(&"remote_only".to_string()),
            "need_category should be preserved"
        );
        assert_eq!(
            items_after.get("test"),
            Some(&"remote_only".to_string()),
            "need_category should be preserved"
        );
    }

    #[test]
    fn test_get_folder_sync_breakdown_counts_categories() {
        let cache = CacheDb::new_in_memory().unwrap();

        // Setup test data
        let need_response = NeedResponse {
            progress: vec![
                FileInfo {
                    name: "file1.txt".to_string(),
                    ..Default::default()
                },
                FileInfo {
                    name: "file2.txt".to_string(),
                    ..Default::default()
                },
            ],
            queued: vec![FileInfo {
                name: "file3.txt".to_string(),
                ..Default::default()
            }],
            rest: vec![],
            page: 1,
            perpage: 100,
        };

        cache
            .cache_needed_files("test-folder", &need_response)
            .unwrap();

        let breakdown = cache.get_folder_sync_breakdown("test-folder").unwrap();

        assert_eq!(breakdown.downloading, 2);
        assert_eq!(breakdown.queued, 1);
        assert_eq!(breakdown.remote_only, 0);
        assert_eq!(breakdown.modified, 0);
        assert_eq!(breakdown.local_only, 0);
    }

    #[test]
    fn test_invalidate_out_of_sync_categories_clears_data() {
        let cache = CacheDb::new_in_memory().unwrap();

        let need_response = NeedResponse {
            progress: vec![FileInfo {
                name: "file1.txt".to_string(),
                ..Default::default()
            }],
            queued: vec![],
            rest: vec![],
            page: 1,
            perpage: 100,
        };

        cache
            .cache_needed_files("test-folder", &need_response)
            .unwrap();

        let before = cache.get_folder_sync_breakdown("test-folder").unwrap();
        assert_eq!(before.downloading, 1);

        cache
            .invalidate_out_of_sync_categories("test-folder")
            .unwrap();

        let after = cache.get_folder_sync_breakdown("test-folder").unwrap();
        assert_eq!(after.downloading, 0);
    }

    #[test]
    fn test_local_changed_columns_exist() {
        let cache = CacheDb::new_in_memory().unwrap();

        // Verify columns exist
        let has_local_changed: bool = cache.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sync_states') WHERE name = 'local_changed'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count == 1)
            },
        ).unwrap();

        let has_local_cached_at: bool = cache.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('sync_states') WHERE name = 'local_cached_at'",
            [],
            |row| {
                let count: i32 = row.get(0)?;
                Ok(count == 1)
            },
        ).unwrap();

        assert!(has_local_changed, "local_changed column should exist");
        assert!(has_local_cached_at, "local_cached_at column should exist");
    }

    #[test]
    fn test_cache_local_changed_files_stores_flag() {
        let cache = CacheDb::new_in_memory().unwrap();

        let local_files = vec!["dir1/file1.txt".to_string(), "file2.txt".to_string()];

        cache
            .cache_local_changed_files("test-folder", &local_files)
            .unwrap();

        // Verify files were marked as local_changed
        let items = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.contains(&"dir1/file1.txt".to_string()));
        assert!(items.contains(&"file2.txt".to_string()));
    }

    #[test]
    fn test_cache_local_changed_files_respects_ttl() {
        let cache = CacheDb::new_in_memory().unwrap();

        use rusqlite::params;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Insert file with timestamp 40 seconds in the past (expired)
        let past = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
            - 40;

        cache.conn.execute(
            "INSERT INTO sync_states (folder_id, file_path, file_sequence, sync_state, local_changed, local_cached_at)
             VALUES ('test-folder', 'old-file.txt', 0, 'LocalOnly', 1, ?1)",
            params![past],
        ).unwrap();

        // Should return empty due to TTL expiration (40s > 30s)
        let items = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(items.len(), 0, "Expired items should not be returned");

        // Now cache fresh data - should be returned
        let fresh_files = vec!["fresh-file.txt".to_string()];
        cache
            .cache_local_changed_files("test-folder", &fresh_files)
            .unwrap();

        let fresh_items = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(fresh_items.len(), 1, "Fresh items should be returned");
        assert_eq!(fresh_items[0], "fresh-file.txt");
    }

    #[test]
    fn test_invalidate_local_changed_clears_data() {
        let cache = CacheDb::new_in_memory().unwrap();

        let local_files = vec!["file1.txt".to_string()];
        cache
            .cache_local_changed_files("test-folder", &local_files)
            .unwrap();

        let before = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(before.len(), 1);

        cache.invalidate_local_changed("test-folder").unwrap();

        let after = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(after.len(), 0);
    }

    #[test]
    fn test_cache_includes_deleted_files() {
        // This test verifies that the cache can store and retrieve deleted file paths
        // The bug was that services/api.rs was calling get_local_changed_items()
        // which filters out deleted files, instead of get_local_changed_files()
        // which includes all files
        let cache = CacheDb::new_in_memory().unwrap();

        // Simulate caching deleted files (which should come from API)
        let local_files = vec![
            "deleted-file.jpg".to_string(), // Deleted file
            "added-file.txt".to_string(),   // Added file
        ];

        cache
            .cache_local_changed_files("test-folder", &local_files)
            .unwrap();

        // Both files should be in cache, including deleted ones
        let items = cache.get_local_changed_items("test-folder").unwrap();
        assert_eq!(
            items.len(),
            2,
            "Cache should include all files from API, including deleted"
        );
        assert!(
            items.contains(&"deleted-file.jpg".to_string()),
            "Deleted files should be cached"
        );
        assert!(
            items.contains(&"added-file.txt".to_string()),
            "Added files should be cached"
        );
    }
}
