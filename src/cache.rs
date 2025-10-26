use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::PathBuf;

use crate::api::{BrowseItem, FolderStatus, SyncState};

pub struct CacheDb {
    conn: Connection,
}

impl CacheDb {
    pub fn new() -> Result<Self> {
        let cache_dir = Self::get_cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;

        let db_path = cache_dir.join("cache.db");
        let conn = Connection::open(db_path)?;

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
                receive_only_changed_bytes INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS browse_cache (
                folder_id TEXT NOT NULL,
                folder_sequence INTEGER NOT NULL,
                prefix TEXT,
                name TEXT NOT NULL,
                item_type TEXT NOT NULL,
                PRIMARY KEY (folder_id, prefix, name)
            ) WITHOUT ROWID;

            CREATE TABLE IF NOT EXISTS sync_states (
                folder_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_sequence INTEGER NOT NULL,
                sync_state TEXT NOT NULL,
                PRIMARY KEY (folder_id, file_path)
            ) WITHOUT ROWID;
            ",
        )?;

        Ok(())
    }

    // Folder status caching
    pub fn get_folder_status(&self, folder_id: &str) -> Result<Option<FolderStatus>> {
        let mut stmt = self.conn.prepare(
            "SELECT state, need_total_items, receive_only_total_items, global_bytes,
                    local_bytes, need_bytes, receive_only_changed_bytes
             FROM folder_status WHERE folder_id = ?1"
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
                // Other fields with default values
                global_deleted: 0,
                global_directories: 0,
                global_files: 0,
                global_symlinks: 0,
                global_total_items: 0,
                in_sync_bytes: 0,
                in_sync_files: 0,
                local_deleted: 0,
                local_directories: 0,
                local_files: 0,
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

    pub fn save_folder_status(&self, folder_id: &str, status: &FolderStatus, sequence: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO folder_status
             (folder_id, sequence, state, need_total_items, receive_only_total_items,
              global_bytes, local_bytes, need_bytes, receive_only_changed_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
            ],
        )?;

        Ok(())
    }

    pub fn is_folder_status_valid(&self, folder_id: &str, current_sequence: u64) -> Result<bool> {
        let mut stmt = self.conn.prepare(
            "SELECT sequence FROM folder_status WHERE folder_id = ?1"
        )?;

        let cached_seq: Option<i64> = stmt.query_row(params![folder_id], |row| row.get(0)).ok();

        Ok(cached_seq.map_or(false, |seq| seq as u64 == current_sequence))
    }

    // Browse cache
    pub fn get_browse_items(&self, folder_id: &str, prefix: Option<&str>, folder_sequence: u64) -> Result<Option<Vec<BrowseItem>>> {
        // Check if cache is valid first
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT folder_sequence FROM browse_cache
             WHERE folder_id = ?1 AND prefix IS ?2 LIMIT 1"
        )?;

        let cached_seq: Option<i64> = stmt.query_row(params![folder_id, prefix], |row| row.get(0)).ok();

        if cached_seq.map_or(false, |seq| seq as u64 != folder_sequence) || cached_seq.is_none() {
            return Ok(None); // Cache is stale or doesn't exist
        }

        // Fetch cached items
        let mut stmt = self.conn.prepare(
            "SELECT name, item_type FROM browse_cache
             WHERE folder_id = ?1 AND prefix IS ?2"
        )?;

        let items = stmt.query_map(params![folder_id, prefix], |row| {
            Ok(BrowseItem {
                name: row.get(0)?,
                item_type: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

        Ok(Some(items))
    }

    pub fn save_browse_items(&self, folder_id: &str, prefix: Option<&str>, items: &[BrowseItem], folder_sequence: u64) -> Result<()> {
        // Use a transaction for better performance with many items
        let tx = self.conn.unchecked_transaction()?;

        // Delete old entries for this folder/prefix
        tx.execute(
            "DELETE FROM browse_cache WHERE folder_id = ?1 AND prefix IS ?2",
            params![folder_id, prefix],
        )?;

        // Insert new entries
        {
            let mut stmt = tx.prepare(
                "INSERT INTO browse_cache (folder_id, folder_sequence, prefix, name, item_type)
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            )?;

            for item in items {
                stmt.execute(params![
                    folder_id,
                    folder_sequence as i64,
                    prefix,
                    &item.name,
                    &item.item_type,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    // Get cached sync state without validation (for initial load)
    pub fn get_sync_state_unvalidated(&self, folder_id: &str, file_path: &str) -> Result<Option<SyncState>> {
        let mut stmt = self.conn.prepare(
            "SELECT sync_state FROM sync_states
             WHERE folder_id = ?1 AND file_path = ?2"
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

    // Sync states cache (validated by sequence)
    pub fn get_sync_state(&self, folder_id: &str, file_path: &str, file_sequence: u64) -> Result<Option<SyncState>> {
        let mut stmt = self.conn.prepare(
            "SELECT file_sequence, sync_state FROM sync_states
             WHERE folder_id = ?1 AND file_path = ?2"
        )?;

        let result = stmt.query_row(params![folder_id, file_path], |row| {
            let cached_seq: i64 = row.get(0)?;
            let state_str: String = row.get(1)?;
            Ok((cached_seq, state_str))
        });

        match result {
            Ok((cached_seq, state_str)) if cached_seq as u64 == file_sequence => {
                Ok(Some(Self::parse_sync_state(&state_str)))
            }
            Ok(_) => Ok(None), // Stale cache
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_sync_state(&self, folder_id: &str, file_path: &str, state: SyncState, file_sequence: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_states (folder_id, file_path, file_sequence, sync_state)
             VALUES (?1, ?2, ?3, ?4)",
            params![folder_id, file_path, file_sequence as i64, Self::serialize_sync_state(state)],
        )?;

        Ok(())
    }

    fn serialize_sync_state(state: SyncState) -> String {
        match state {
            SyncState::Synced => "Synced".to_string(),
            SyncState::OutOfSync => "OutOfSync".to_string(),
            SyncState::LocalOnly => "LocalOnly".to_string(),
            SyncState::RemoteOnly => "RemoteOnly".to_string(),
            SyncState::Ignored => "Ignored".to_string(),
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
            _ => SyncState::Unknown,
        }
    }

    // Invalidate all cache for a folder when sequence changes
    pub fn invalidate_folder(&self, folder_id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM browse_cache WHERE folder_id = ?1", params![folder_id])?;
        self.conn.execute("DELETE FROM sync_states WHERE folder_id = ?1", params![folder_id])?;
        Ok(())
    }
}
