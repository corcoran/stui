use crate::services::events::SyncthingEvent;
use crate::utils;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

fn log_debug(msg: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(utils::get_debug_log_path())
    {
        let _ = writeln!(file, "{}", msg);
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FolderDevice {
    #[serde(rename = "deviceID")]
    pub device_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Folder {
    pub id: String,
    pub label: Option<String>,
    #[allow(dead_code)]
    pub path: String,
    pub paused: bool,
    #[serde(rename = "type")]
    pub folder_type: String, // "sendonly", "sendreceive", "receiveonly"
    #[serde(default)]
    pub devices: Vec<FolderDevice>,
    /// Syncthing 2.1+ folder group; `None` when Syncthing doesn't send the field
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    #[serde(rename = "deviceID")]
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
struct ConfigResponse {
    folders: Vec<Folder>,
    #[serde(default)]
    devices: Vec<Device>,
}

/// A log level: a name on Syncthing 2.x, a number on 1.x
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum RawLevel {
    Name(String),
    Number(i64),
}

/// One line of /rest/system/log
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct LogLine {
    pub when: String,
    pub message: String,
    pub level: RawLevel,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct BrowseItem {
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
    #[serde(rename = "modTime", default)]
    pub mod_time: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Synced,     // ✅ Local matches global
    OutOfSync,  // ⚠️ Local differs from global
    LocalOnly,  // 💻 Only on this device
    RemoteOnly, // ☁️ Only on remote devices
    Ignored,    // 🚫 In .stignore
    Syncing,    // 🔄 Currently syncing
    Unknown,    // ❓ Not yet determined
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct FileInfo {
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub invalid: bool,
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub blocks_hash: Option<String>,
    #[serde(default)]
    pub version: Vec<String>,
    // Additional fields that may be present
    #[serde(default)]
    pub modified: String,
    #[serde(default)]
    pub modified_by: String,
    #[serde(default)]
    pub inode_change: String,
    #[serde(default)]
    pub must_rescan: bool,
    #[serde(default)]
    pub no_permissions: bool,
    #[serde(default)]
    pub permissions: String,
    #[serde(default)]
    pub num_blocks: u64,
    #[serde(default)]
    pub local_flags: u64,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub previous_blocks_hash: Option<String>,
    #[serde(default, rename = "type")]
    pub file_type: String,
    // Platform is a complex object we don't need, just skip it
    #[serde(default, skip_serializing)]
    pub platform: serde_json::Value,
}

/// Response of GET /rest/db/ignores
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignores {
    /// Raw .stignore lines
    pub lines: Vec<String>,
    /// Lines after `#include` expansion (absent on old Syncthing versions)
    pub expanded: Option<Vec<String>>,
}

impl Ignores {
    /// The expanded patterns, or the raw lines when Syncthing didn't expand them
    pub fn effective(self) -> Vec<String> {
        self.expanded.unwrap_or(self.lines)
    }
}

/// One entry of /rest/db/localchanged
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalChangedFile {
    pub name: String,
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NeedResponse {
    pub progress: Vec<FileInfo>,
    pub queued: Vec<FileInfo>,
    pub rest: Vec<FileInfo>,
    #[allow(dead_code)]
    pub page: u32,
    #[allow(dead_code)]
    pub perpage: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DeviceAvailability {
    pub id: String,
    #[serde(rename = "fromTemporary")]
    pub from_temporary: bool,
}

/// Helper function to deserialize null as empty vector
fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone, Deserialize)]
pub struct FileDetails {
    pub local: Option<FileInfo>,
    pub global: Option<FileInfo>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub availability: Vec<DeviceAvailability>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    pub state: String,
    pub sequence: u64,
    #[allow(dead_code)]
    pub global_bytes: u64,
    #[allow(dead_code)]
    pub global_deleted: u64,
    #[allow(dead_code)]
    pub global_directories: u64,
    #[allow(dead_code)]
    pub global_files: u64,
    #[allow(dead_code)]
    pub global_symlinks: u64,
    #[allow(dead_code)]
    pub global_total_items: u64,
    #[allow(dead_code)]
    pub in_sync_bytes: u64,
    #[allow(dead_code)]
    pub in_sync_files: u64,
    #[allow(dead_code)]
    pub local_bytes: u64,
    #[allow(dead_code)]
    pub local_deleted: u64,
    #[allow(dead_code)]
    pub local_directories: u64,
    #[allow(dead_code)]
    pub local_files: u64,
    #[allow(dead_code)]
    pub local_symlinks: u64,
    #[allow(dead_code)]
    pub local_total_items: u64,
    #[allow(dead_code)]
    pub need_bytes: u64,
    #[allow(dead_code)]
    pub need_deletes: u64,
    #[allow(dead_code)]
    pub need_directories: u64,
    #[allow(dead_code)]
    pub need_files: u64,
    #[allow(dead_code)]
    pub need_symlinks: u64,
    pub need_total_items: u64,
    pub receive_only_changed_bytes: u64,
    #[allow(dead_code)]
    pub receive_only_changed_deletes: u64,
    #[allow(dead_code)]
    pub receive_only_changed_directories: u64,
    #[allow(dead_code)]
    pub receive_only_changed_files: u64,
    #[allow(dead_code)]
    pub receive_only_changed_symlinks: u64,
    pub receive_only_total_items: u64,
    #[serde(default)]
    pub errors: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    #[serde(rename = "myID")]
    #[allow(dead_code)]
    pub my_id: String,
    pub uptime: u64,
    #[allow(dead_code)]
    pub start_time: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTotal {
    #[allow(dead_code)]
    pub at: String,
    pub in_bytes_total: u64,
    pub out_bytes_total: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionStats {
    pub total: ConnectionTotal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub connected: bool,
    #[allow(dead_code)]
    pub address: String,
    #[allow(dead_code)]
    pub in_bytes_total: u64,
    #[allow(dead_code)]
    pub out_bytes_total: u64,
    #[allow(dead_code)]
    pub paused: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionsResponse {
    pub connections: std::collections::HashMap<String, ConnectionInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LastFileInfo {
    pub at: String,
    pub filename: String,
    /// Whether the last operation was a deletion (we still show these in "Last Update")
    #[allow(dead_code)]
    pub deleted: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStats {
    pub last_file: LastFileInfo,
    /// Last scan timestamp (not currently used)
    #[allow(dead_code)]
    pub last_scan: String,
}

/// Each folder's most recent file change as (time, path). Folders with no activity or an
/// unparseable time are left out.
pub(crate) fn last_file_updates(
    stats: std::collections::HashMap<String, FolderStats>,
) -> std::collections::HashMap<String, (std::time::SystemTime, String)> {
    stats
        .into_iter()
        .filter_map(|(folder_id, folder_stats)| {
            let at = crate::services::events::try_parse_event_time(&folder_stats.last_file.at)?;
            is_real_last_file(at, &folder_stats.last_file.filename)
                .then_some((folder_id, (at, folder_stats.last_file.filename)))
        })
        .collect()
}

/// False for the placeholder `lastFile` Syncthing reports for folders with no activity
/// (`at: "0001-01-01T00:00:00Z"`, `filename: ""`).
fn is_real_last_file(at: std::time::SystemTime, filename: &str) -> bool {
    !filename.is_empty() && at > std::time::UNIX_EPOCH
}

const FOLDER_CONFIG_PATCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct SyncthingClient {
    base_url: String,
    api_key: String,
    client: Client,
}

impl SyncthingClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self {
            base_url,
            api_key,
            client: Client::new(),
        }
    }

    /// Fetch system config (consolidates get_folders and get_device_name)
    async fn get_system_config(&self) -> Result<ConfigResponse> {
        let url = format!("{}/rest/system/config", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await?;

        let config: ConfigResponse = response.json().await?;

        Ok(config)
    }

    pub async fn get_folders(&self) -> Result<Vec<Folder>> {
        let config = self.get_system_config().await?;
        Ok(config.folders)
    }

    pub async fn get_devices(&self) -> Result<Vec<Device>> {
        let config = self.get_system_config().await?;
        Ok(config.devices)
    }

    pub async fn get_device_name(&self) -> Result<String> {
        // Get local device ID from system status
        let system_status = self.get_system_status().await?;
        let my_id = system_status.my_id;

        // Get all devices from config
        let config = self.get_system_config().await?;

        // Find the device that matches our local ID
        config
            .devices
            .iter()
            .find(|device| device.id == my_id)
            .map(|device| device.name.clone())
            .ok_or_else(|| anyhow::anyhow!("Local device ID not found in config"))
    }

    pub async fn browse_folder(
        &self,
        folder_id: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<BrowseItem>> {
        let mut url = format!("{}/rest/db/browse?folder={}", self.base_url, folder_id);

        if let Some(prefix) = prefix {
            url.push_str(&format!("&prefix={}", prefix));
        }

        log_debug(&format!(
            "DEBUG [browse_folder API]: Requesting folder={} prefix={:?}",
            folder_id, prefix
        ));

        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to browse folder")?;

        // Check if response is an error (plain text)
        let status = response.status();
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("API error: {}", error_text));
        }

        // Try to parse as JSON, handle paused/unavailable folders
        let text = response.text().await.context("Failed to read response")?;
        if text.contains("no such folder") || text.contains("paused") {
            // Return empty list for paused/unavailable folders
            return Ok(Vec::new());
        }

        let items: Vec<BrowseItem> =
            serde_json::from_str(&text).context("Failed to parse browse response")?;

        log_debug(&format!(
            "DEBUG [browse_folder API]: Received {} items for folder={} prefix={:?}",
            items.len(),
            folder_id,
            prefix
        ));

        Ok(items)
    }

    pub async fn get_folder_status(&self, folder_id: &str) -> Result<FolderStatus> {
        let url = format!("{}/rest/db/status?folder={}", self.base_url, folder_id);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch folder status")?;

        let status: FolderStatus = response
            .json()
            .await
            .context("Failed to parse folder status")?;

        Ok(status)
    }

    pub async fn get_file_info(&self, folder_id: &str, file_path: &str) -> Result<FileDetails> {
        let url = format!(
            "{}/rest/db/file?folder={}&file={}",
            self.base_url,
            folder_id,
            urlencoding::encode(file_path)
        );
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch file info")?;

        let details: FileDetails = response.json().await.context("Failed to parse file info")?;

        Ok(details)
    }

    /// Full folder scan.
    pub async fn rescan_folder(&self, folder_id: &str) -> Result<()> {
        self.rescan_paths(folder_id, &[]).await
    }

    /// Scan only `paths` (relative to the folder root). An empty slice scans the whole folder.
    pub async fn rescan_paths(&self, folder_id: &str, paths: &[String]) -> Result<()> {
        let url = scan_url(&self.base_url, folder_id, paths);
        self.client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to trigger rescan")?;

        Ok(())
    }

    pub async fn revert_folder(&self, folder_id: &str) -> Result<()> {
        let url = format!("{}/rest/db/revert?folder={}", self.base_url, folder_id);
        self.client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to revert folder")?
            .error_for_status()
            .context("Syncthing refused to revert the folder")?;

        Ok(())
    }

    pub async fn get_local_changed_files(&self, folder_id: &str) -> Result<Vec<String>> {
        Ok(self
            .get_local_changed_entries(folder_id)
            .await?
            .into_iter()
            .map(|f| f.name)
            .collect())
    }

    /// Receive-only local changes, including deletions
    pub async fn get_local_changed_entries(
        &self,
        folder_id: &str,
    ) -> Result<Vec<LocalChangedFile>> {
        let url = format!(
            "{}/rest/db/localchanged?folder={}",
            self.base_url, folder_id
        );
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch local changed files")?;

        #[derive(Deserialize)]
        struct LocalChangedResponse {
            files: Vec<FileInfo>,
        }

        let data: LocalChangedResponse = response
            .json()
            .await
            .context("Failed to parse local changed files")?;

        Ok(data
            .files
            .into_iter()
            .map(|f| LocalChangedFile {
                name: f.name,
                deleted: f.deleted,
            })
            .collect())
    }

    pub async fn get_local_changed_items(
        &self,
        folder_id: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<BrowseItem>> {
        let url = format!(
            "{}/rest/db/localchanged?folder={}",
            self.base_url, folder_id
        );
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch local changed files")?;

        #[derive(Deserialize)]
        struct LocalChangedResponse {
            files: Vec<FileInfo>,
        }

        let data: LocalChangedResponse = response
            .json()
            .await
            .context("Failed to parse local changed files")?;

        // Filter to current directory and convert to BrowseItems
        let prefix_str = prefix.unwrap_or("");
        let mut items = Vec::new();

        for file in data.files {
            // Skip deleted files - they no longer exist locally
            if file.deleted {
                continue;
            }

            // Skip files not in this directory
            if !file.name.starts_with(prefix_str) {
                continue;
            }

            // Get the relative name (remove prefix)
            let relative_name = file.name.strip_prefix(prefix_str).unwrap_or(&file.name);

            // Skip if it has subdirectories (we only want items in current level)
            if relative_name.contains('/') {
                continue;
            }

            // Convert to BrowseItem
            items.push(BrowseItem {
                name: relative_name.to_string(),
                size: file.size,
                mod_time: file.modified.clone(),
                item_type: if file.file_type == "FILE_INFO_TYPE_FILE" {
                    "FILE_INFO_TYPE_FILE".to_string()
                } else {
                    "FILE_INFO_TYPE_DIRECTORY".to_string()
                },
            });
        }

        Ok(items)
    }

    pub async fn get_needed_files(
        &self,
        folder_id: &str,
        page: Option<u32>,
        perpage: Option<u32>,
    ) -> Result<NeedResponse> {
        let mut url = format!("{}/rest/db/need?folder={}", self.base_url, folder_id);

        if let Some(page) = page {
            url.push_str(&format!("&page={}", page));
        }
        if let Some(perpage) = perpage {
            url.push_str(&format!("&perpage={}", perpage));
        }

        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to get needed files")?;

        response
            .json()
            .await
            .context("Failed to parse need response")
    }

    /// The folder's .stignore lines and their `#include`-expanded form
    pub async fn get_ignores(&self, folder_id: &str) -> Result<Ignores> {
        let url = format!("{}/rest/db/ignores?folder={}", self.base_url, folder_id);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch ignore patterns")?;

        #[derive(Deserialize)]
        struct IgnoresResponse {
            ignore: Option<Vec<String>>,
            expanded: Option<Vec<String>>,
        }

        let data: IgnoresResponse = response
            .json()
            .await
            .context("Failed to parse ignore patterns")?;

        Ok(Ignores {
            lines: data.ignore.unwrap_or_default(),
            expanded: data.expanded,
        })
    }

    pub async fn get_ignore_patterns(&self, folder_id: &str) -> Result<Vec<String>> {
        Ok(self.get_ignores(folder_id).await?.lines)
    }

    pub async fn set_ignore_patterns(&self, folder_id: &str, patterns: Vec<String>) -> Result<()> {
        let url = format!("{}/rest/db/ignores?folder={}", self.base_url, folder_id);

        #[derive(serde::Serialize)]
        struct IgnoresRequest {
            ignore: Vec<String>,
        }

        let request_body = IgnoresRequest { ignore: patterns };

        self.client
            .post(&url)
            .header("X-API-Key", &self.api_key)
            .json(&request_body)
            .send()
            .await
            .context("Failed to set ignore patterns")?;

        Ok(())
    }

    /// Syncthing's version string, e.g. `"v2.1.7"`
    pub async fn get_version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct VersionResponse {
            version: String,
        }
        let url = format!("{}/rest/system/version", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch Syncthing version")?;
        let version: VersionResponse = response
            .json()
            .await
            .context("Failed to parse Syncthing version")?;
        Ok(version.version)
    }

    /// Syncthing's log in the order it was recorded: the last 1000 lines, or those from the
    /// first one newer than `since` (RFC 3339)
    pub async fn get_system_log(&self, since: Option<&str>) -> Result<Vec<LogLine>> {
        #[derive(Deserialize)]
        struct LogResponse {
            /// null when nothing is newer
            messages: Option<Vec<LogLine>>,
        }
        let mut url = format!("{}/rest/system/log", self.base_url);
        if let Some(since) = since {
            url.push_str(&format!("?since={}", urlencoding::encode(since)));
        }
        let log: LogResponse = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            // The viewer sends nothing more until this one returns
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("Failed to fetch the log")?
            .error_for_status()
            .context("Syncthing refused to send the log")?
            .json()
            .await
            .context("Failed to parse the log")?;
        Ok(log.messages.unwrap_or_default())
    }

    pub async fn get_system_status(&self) -> Result<SystemStatus> {
        let url = format!("{}/rest/system/status", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch system status")?;

        let status: SystemStatus = response
            .json()
            .await
            .context("Failed to parse system status")?;

        Ok(status)
    }

    pub async fn get_connection_stats(&self) -> Result<ConnectionStats> {
        let url = format!("{}/rest/system/connections", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch connection stats")?;

        let stats: ConnectionStats = response
            .json()
            .await
            .context("Failed to parse connection stats")?;

        Ok(stats)
    }

    /// Get system connections (device connectivity and transfer stats)
    pub async fn get_system_connections(&self) -> Result<ConnectionsResponse> {
        let url = format!("{}/rest/system/connections", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to send connections request")?;

        let connections: ConnectionsResponse = response
            .json()
            .await
            .context("Failed to parse connections response")?;

        Ok(connections)
    }

    /// Fetch folder statistics to get last updated file per folder
    ///
    /// Returns HashMap of folder_id -> (timestamp, filename)
    /// Uses /rest/stats/folder which matches what Syncthing web GUI displays
    pub async fn get_folder_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, (std::time::SystemTime, String)>> {
        use std::collections::HashMap;

        let url = format!("{}/rest/stats/folder", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch folder stats")?;

        let stats: HashMap<String, FolderStats> = response
            .json()
            .await
            .context("Failed to parse folder stats")?;

        // Deleted files count too, matching the web GUI
        Ok(last_file_updates(stats))
    }

    /// Fetch folder events from Syncthing event stream
    ///
    /// Returns events starting from `since` event ID, limited to `limit` events.
    /// Uses /rest/events endpoint to get historical sync events.
    ///
    /// # Arguments
    /// * `since` - Event ID to start from (0 for all history, or recent ID for recent events)
    /// * `limit` - Maximum number of events to fetch
    ///
    /// # Returns
    /// Vec of SyncthingEvent objects containing event metadata and data
    #[allow(dead_code)] // Used in folder history feature (Task 4)
    pub async fn get_folder_events(&self, since: u64, limit: usize) -> Result<Vec<SyncthingEvent>> {
        let url = format!(
            "{}/rest/events?since={}&limit={}",
            self.base_url, since, limit
        );

        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch folder events")?;

        let events: Vec<SyncthingEvent> = response
            .json()
            .await
            .context("Failed to parse folder events")?;

        Ok(events)
    }

    /// Pause or resume a folder
    ///
    /// Uses PATCH /rest/config/folders/{id} to set the paused state
    pub async fn set_folder_paused(&self, folder_id: &str, paused: bool) -> Result<()> {
        let url = format!(
            "{}/rest/config/folders/{}",
            self.base_url,
            urlencoding::encode(folder_id)
        );

        let payload = serde_json::json!({
            "paused": paused
        });

        let response = self
            .client
            .patch(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to set folder paused state")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Failed to {} folder: {} - {}",
                if paused { "pause" } else { "resume" },
                status,
                text
            );
        }

        Ok(())
    }

    /// Set a folder's group (Syncthing 2.1+); "" removes it from its group
    pub async fn set_folder_group(&self, folder_id: &str, group: &str) -> Result<()> {
        self.patch_folder_config(
            folder_id,
            serde_json::json!({ "group": group }),
            FOLDER_CONFIG_PATCH_TIMEOUT,
        )
        .await
    }

    /// PATCH `/rest/config/folders/<id>` with `payload`. The key handler awaits this, so
    /// `timeout` bounds how long the UI can block on an unresponsive host.
    async fn patch_folder_config(
        &self,
        folder_id: &str,
        payload: serde_json::Value,
        timeout: std::time::Duration,
    ) -> Result<()> {
        let url = format!(
            "{}/rest/config/folders/{}",
            self.base_url,
            urlencoding::encode(folder_id)
        );
        let response = self
            .client
            .patch(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .context("Failed to update folder config")?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("{} - {}", status, text);
        }
        Ok(())
    }

    /// Change folder type
    ///
    /// Uses PATCH /rest/config/folders/{id} to set the folder type
    /// Valid types: "sendonly", "sendreceive", "receiveonly"
    pub async fn set_folder_type(&self, folder_id: &str, folder_type: &str) -> Result<()> {
        let url = format!(
            "{}/rest/config/folders/{}",
            self.base_url,
            urlencoding::encode(folder_id)
        );

        let payload = serde_json::json!({
            "type": folder_type
        });

        let response = self
            .client
            .patch(&url)
            .header("X-API-Key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to set folder type")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to set folder type: {} - {}", status, text);
        }

        Ok(())
    }
}

impl FileDetails {
    pub fn determine_sync_state(&self) -> SyncState {
        match (&self.local, &self.global) {
            // Both local and global are present
            (Some(local), Some(global)) => {
                // Check if file doesn't exist locally (sequence=0)
                // API returns local object even when file doesn't exist locally
                if local.sequence == 0 && global.sequence > 0 {
                    return SyncState::RemoteOnly;
                }

                if local.ignored {
                    SyncState::Ignored
                } else if local.deleted && global.deleted {
                    SyncState::Synced // Both deleted, in sync
                } else if local.deleted && !global.deleted {
                    SyncState::RemoteOnly // Local deleted but exists remotely
                } else if !local.deleted && global.deleted {
                    SyncState::LocalOnly // Exists locally but deleted remotely
                } else if local.invalid {
                    // Local additions in receive-only folders are marked as invalid
                    // They can't be synced to other devices, so they're LocalOnly
                    SyncState::LocalOnly
                } else if local.version != global.version || local.blocks_hash != global.blocks_hash
                {
                    SyncState::OutOfSync // Different versions or content
                } else {
                    SyncState::Synced // Fully synced
                }
            }
            // Only local present
            (Some(local), None) => {
                if local.ignored {
                    SyncState::Ignored
                } else if self.availability.is_empty() {
                    SyncState::LocalOnly
                } else {
                    SyncState::OutOfSync
                }
            }
            // Only global present
            (None, Some(_global)) => SyncState::RemoteOnly,
            // Neither present
            (None, None) => SyncState::Unknown,
        }
    }
}

/// `POST /rest/db/scan` URL; one `sub` parameter per path, none for a full scan.
pub(crate) fn scan_url(base_url: &str, folder_id: &str, subs: &[String]) -> String {
    let mut url = format!("{}/rest/db/scan?folder={}", base_url, folder_id);
    for sub in subs {
        url.push_str("&sub=");
        url.push_str(&urlencoding::encode(sub));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // LAST FILE FILTER
    // ========================================

    fn parse(at: &str) -> std::time::SystemTime {
        crate::services::events::parse_event_time_public(at)
    }

    #[test]
    fn test_is_real_last_file_skips_zero_time() {
        assert!(!is_real_last_file(
            parse("0001-01-01T00:00:00Z"),
            "file.txt"
        ));
    }

    #[test]
    fn test_is_real_last_file_skips_unix_epoch() {
        assert!(!is_real_last_file(
            parse("1970-01-01T00:00:00Z"),
            "file.txt"
        ));
    }

    #[test]
    fn test_is_real_last_file_skips_empty_filename() {
        assert!(!is_real_last_file(parse("2024-05-01T12:00:00Z"), ""));
    }

    #[test]
    fn test_is_real_last_file_keeps_normal_entry() {
        assert!(is_real_last_file(parse("2024-05-01T12:00:00Z"), "a/b.txt"));
    }

    fn stats_with(at: &str, filename: &str) -> std::collections::HashMap<String, FolderStats> {
        std::collections::HashMap::from([(
            "docs".to_string(),
            FolderStats {
                last_file: LastFileInfo {
                    at: at.to_string(),
                    filename: filename.to_string(),
                    deleted: false,
                },
                last_scan: String::new(),
            },
        )])
    }

    #[test]
    fn test_last_file_updates_keeps_real_entry() {
        let updates = last_file_updates(stats_with("2024-05-01T12:00:00Z", "a/b.txt"));
        let (at, file) = updates.get("docs").expect("real entry kept");
        assert_eq!(*at, parse("2024-05-01T12:00:00Z"));
        assert_eq!(file, "a/b.txt");
    }

    #[test]
    fn test_last_file_updates_skips_malformed_time() {
        let updates = last_file_updates(stats_with("garbage", "a/b.txt"));
        assert!(updates.is_empty(), "{:?}", updates);
    }

    #[test]
    fn test_last_file_updates_skips_no_activity_placeholder() {
        let updates = last_file_updates(stats_with("0001-01-01T00:00:00Z", ""));
        assert!(updates.is_empty(), "{:?}", updates);
    }

    #[tokio::test]
    async fn test_folder_config_patch_times_out_on_silent_server() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_sock, _) = listener.accept().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        let client = SyncthingClient::new(format!("http://{}", addr), "k".to_string());

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.patch_folder_config(
                "photos",
                serde_json::json!({ "group": "Work" }),
                std::time::Duration::from_millis(200),
            ),
        )
        .await
        .expect("the request's own timeout ends it, not the test's");

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_get_version_reads_version_field() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).await.unwrap();
            let body = r#"{"version":"v2.1.7","longVersion":"syncthing v2.1.7"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            sock.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let client = SyncthingClient::new(format!("http://{}", addr), "k".to_string());

        assert_eq!(client.get_version().await.unwrap(), "v2.1.7");
        let request = server.await.unwrap();
        assert!(
            request.starts_with("GET /rest/system/version"),
            "{}",
            request
        );
    }

    #[tokio::test]
    async fn test_get_needed_files_builds_correct_url() {
        // This is a basic smoke test - full integration test requires real Syncthing
        let _client =
            SyncthingClient::new("http://localhost:8384".to_string(), "test-key".to_string());

        // We can't actually call the API without a real instance,
        // but we can verify the method exists and accepts correct params
        // Real testing will happen in integration tests
    }

    #[test]
    fn test_folder_stats_parsing() {
        // Test parsing /rest/stats/folder response
        let stats_json = r#"{
            "folder-abc-123": {
                "lastFile": {
                    "at": "2025-11-09T23:24:15Z",
                    "filename": ".thumbnails/7726.jpg",
                    "deleted": false
                },
                "lastScan": "2025-11-10T04:29:36Z"
            },
            "folder-xyz-456": {
                "lastFile": {
                    "at": "2025-11-09T23:24:39Z",
                    "filename": "worduel-staging/main.dart.js",
                    "deleted": false
                },
                "lastScan": "2025-11-10T04:46:57Z"
            },
            "folder-deleted-789": {
                "lastFile": {
                    "at": "2025-11-09T20:00:00Z",
                    "filename": "deleted-file.txt",
                    "deleted": true
                },
                "lastScan": "2025-11-10T04:00:00Z"
            }
        }"#;

        use std::collections::HashMap;
        let stats: HashMap<String, FolderStats> =
            serde_json::from_str(stats_json).expect("Failed to parse folder stats");

        // Verify structure
        assert_eq!(stats.len(), 3, "Should have 3 folders");

        let folder_abc = stats
            .get("folder-abc-123")
            .expect("Should have folder-abc-123");
        assert_eq!(folder_abc.last_file.filename, ".thumbnails/7726.jpg");
        assert_eq!(folder_abc.last_file.at, "2025-11-09T23:24:15Z");
        assert!(!folder_abc.last_file.deleted);

        let folder_deleted = stats
            .get("folder-deleted-789")
            .expect("Should have folder-deleted-789");
        assert!(
            folder_deleted.last_file.deleted,
            "Deleted flag should be true"
        );

        let updates = last_file_updates(stats);

        // Should have all 3 folders (including deleted one - matches web GUI behavior)
        assert_eq!(
            updates.len(),
            3,
            "Should include all files, even deleted ones"
        );
        assert!(updates.contains_key("folder-abc-123"));
        assert!(updates.contains_key("folder-xyz-456"));
        assert!(
            updates.contains_key("folder-deleted-789"),
            "Should include deleted file"
        );

        // Verify file paths
        let abc_update = updates
            .get("folder-abc-123")
            .expect("Should have folder-abc-123");
        assert_eq!(abc_update.1, ".thumbnails/7726.jpg");
    }

    #[test]
    fn test_determine_sync_state_remote_only_with_sequence_zero() {
        // Bug: Files with local.sequence=0 (don't exist locally) were showing as OutOfSync
        // because empty local.version != global.version
        // Expected: sequence=0 should return RemoteOnly, not OutOfSync

        let file_details = FileDetails {
            local: Some(FileInfo {
                sequence: 0, // KEY: File doesn't exist locally
                deleted: false,
                ignored: false,
                version: vec![], // Empty version
                blocks_hash: None,
                ..Default::default()
            }),
            global: Some(FileInfo {
                sequence: 6629, // Exists remotely
                deleted: false,
                ignored: false,
                version: vec!["DEVICE:12345".to_string()], // Has version
                blocks_hash: Some("hash123".to_string()),
                ..Default::default()
            }),
            availability: vec![], // Has availability (remote devices have it)
        };

        let state = file_details.determine_sync_state();
        assert_eq!(
            state,
            SyncState::RemoteOnly,
            "File with local.sequence=0 should be RemoteOnly, not {:?}",
            state
        );
    }

    #[test]
    fn test_determine_sync_state_out_of_sync_with_actual_version_mismatch() {
        // Verify that real version mismatches (both files exist) still return OutOfSync
        let file_details = FileDetails {
            local: Some(FileInfo {
                sequence: 100, // File exists locally
                deleted: false,
                ignored: false,
                version: vec!["DEVICE:100".to_string()],
                blocks_hash: Some("hash_local".to_string()),
                ..Default::default()
            }),
            global: Some(FileInfo {
                sequence: 200, // Different global version
                deleted: false,
                ignored: false,
                version: vec!["DEVICE:200".to_string()],
                blocks_hash: Some("hash_global".to_string()),
                ..Default::default()
            }),
            availability: vec![],
        };

        let state = file_details.determine_sync_state();
        assert_eq!(
            state,
            SyncState::OutOfSync,
            "File with different versions should be OutOfSync"
        );
    }

    #[test]
    fn test_determine_sync_state_local_addition_in_receive_only_folder() {
        // Bug: Local additions in receive-only folders show as Synced instead of LocalOnly
        // When a file is added locally to a receive-only folder, Syncthing marks it with:
        // - invalid=true (can't be synced to other devices)
        // - localFlags=8 (bit flag for receive-only local addition)
        // - Both local and global have SAME version/hash (Syncthing copies local to global)
        //
        // Expected: Should return LocalOnly, not Synced
        let file_details = FileDetails {
            local: Some(FileInfo {
                sequence: 10017,
                deleted: false,
                ignored: false,
                invalid: true,  // KEY: Marked as invalid
                local_flags: 8, // KEY: Receive-only local addition flag
                version: vec!["LHNT25C:1762736484".to_string()],
                blocks_hash: Some("Xfbg4nYTWdMKgnUFjimfzAOBU0VF9Vz0PkGYP11MlFY=".to_string()),
                modified_by: "LHNT25C".to_string(),
                ..Default::default()
            }),
            global: Some(FileInfo {
                sequence: 10017, // Same as local
                deleted: false,
                ignored: false,
                invalid: true,
                local_flags: 8,
                version: vec!["LHNT25C:1762736484".to_string()], // Same as local
                blocks_hash: Some("Xfbg4nYTWdMKgnUFjimfzAOBU0VF9Vz0PkGYP11MlFY=".to_string()), // Same as local
                modified_by: "LHNT25C".to_string(),
                ..Default::default()
            }),
            availability: vec![], // No other devices have it
        };

        let state = file_details.determine_sync_state();
        assert_eq!(
            state,
            SyncState::LocalOnly,
            "Local addition in receive-only folder (invalid=true) should be LocalOnly, not {:?}",
            state
        );
    }

    #[test]
    fn test_get_folder_events_exists() {
        // Test that get_folder_events method exists with correct signature
        // We can't easily test async functions in unit tests without tokio runtime,
        // but we can verify the method exists by calling it in a type-checked way
        let _client = SyncthingClient {
            base_url: "http://localhost:8384".to_string(),
            api_key: "test-key".to_string(),
            client: reqwest::Client::new(),
        };

        // Verify method exists by referencing it (won't execute due to being async)
        let _method = SyncthingClient::get_folder_events;
        // Verify parameters are correct types
        let _since: u64 = 0;
        let _limit: usize = 100;
        // This would need tokio runtime to actually call:
        // let _ = client.get_folder_events(_since, _limit).await;
    }

    #[test]
    fn test_connections_response_parsing() {
        let json = r#"{
            "connections": {
                "DEVICE123-ABC": {
                    "connected": true,
                    "address": "192.168.1.10:22000",
                    "inBytesTotal": 1234567890,
                    "outBytesTotal": 9876543210,
                    "paused": false
                },
                "DEVICE456-DEF": {
                    "connected": false,
                    "address": "",
                    "inBytesTotal": 0,
                    "outBytesTotal": 0,
                    "paused": false
                }
            }
        }"#;

        let parsed: serde_json::Value = serde_json::from_str(json).unwrap();
        let connections = parsed.get("connections").unwrap();
        let device1 = connections.get("DEVICE123-ABC").unwrap();
        assert!(device1.get("connected").unwrap().as_bool().unwrap());
        assert_eq!(
            device1.get("address").unwrap().as_str().unwrap(),
            "192.168.1.10:22000"
        );

        let device2 = connections.get("DEVICE456-DEF").unwrap();
        assert!(!device2.get("connected").unwrap().as_bool().unwrap());
    }

    // ========================================
    // SCAN URL
    // ========================================

    #[test]
    fn test_scan_url_without_subs_is_full_scan() {
        let url = scan_url("http://h:8384", "docs", &[]);
        assert_eq!(url, "http://h:8384/rest/db/scan?folder=docs");
        assert!(!url.contains("sub="));
    }

    #[test]
    fn test_scan_url_repeats_sub_per_path() {
        let subs = vec!["notes/a.txt".to_string(), "other/dir".to_string()];
        assert_eq!(
            scan_url("http://h", "docs", &subs),
            "http://h/rest/db/scan?folder=docs&sub=notes%2Fa.txt&sub=other%2Fdir"
        );
    }

    #[test]
    fn test_scan_url_encodes_special_characters() {
        let subs = vec![
            "my notes/a b.txt".to_string(),
            "x#1&y=2".to_string(),
            "caf\u{e9}/\u{65e5}\u{672c}".to_string(),
        ];
        let url = scan_url("http://h", "docs", &subs);
        assert!(url.contains("&sub=my%20notes%2Fa%20b.txt"), "{}", url);
        assert!(url.contains("&sub=x%231%26y%3D2"), "{}", url);
        assert!(
            url.contains("&sub=caf%C3%A9%2F%E6%97%A5%E6%9C%AC"),
            "{}",
            url
        );
        assert_eq!(url.matches('&').count(), 3, "only the separators: {}", url);
        assert!(!url.contains('#'));
    }

    // ========================================
    // SYSTEM LOG
    // ========================================

    async fn system_log(
        status: &'static str,
        body: &'static str,
        since: Option<&str>,
    ) -> (Result<Vec<LogLine>>, String) {
        let (url, mut requests) = crate::test_http::spawn_stub(status, body).await;
        let client = SyncthingClient::new(url, "key".to_string());
        let lines = client.get_system_log(since).await;
        (lines, requests.recv().await.unwrap())
    }

    #[tokio::test]
    async fn test_get_system_log_sends_encoded_since() {
        let body = r#"{"messages":[]}"#;
        let (_, request) = system_log("200 OK", body, Some("2026-01-01T10:00:00+02:00")).await;
        assert_eq!(
            request,
            "GET /rest/system/log?since=2026-01-01T10%3A00%3A00%2B02%3A00 HTTP/1.1"
        );
        let (_, request) = system_log("200 OK", body, None).await;
        assert_eq!(request, "GET /rest/system/log HTTP/1.1");
    }

    #[tokio::test]
    async fn test_get_system_log_null_messages_is_empty() {
        let (lines, _) = system_log("200 OK", r#"{"messages":null}"#, None).await;
        assert_eq!(lines.unwrap(), vec![]);
    }

    #[tokio::test]
    async fn test_get_system_log_reads_string_and_number_levels() {
        let body = r#"{"messages":[
            {"when":"2026-01-01T10:00:00Z","message":"a","level":"WRN"},
            {"when":"2026-01-01T10:00:01Z","message":"b","level":3}]}"#;
        let (lines, _) = system_log("200 OK", body, None).await;
        let levels: Vec<_> = lines.unwrap().into_iter().map(|l| l.level).collect();
        assert_eq!(
            levels,
            [RawLevel::Name("WRN".to_string()), RawLevel::Number(3)]
        );
    }

    #[tokio::test]
    async fn test_get_system_log_rejects_http_errors() {
        let (lines, _) = system_log("403 Forbidden", r#"{"messages":[]}"#, None).await;
        assert!(lines.is_err());
    }
}
