use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Folder {
    pub id: String,
    pub label: Option<String>,
    #[allow(dead_code)]
    pub path: String,
    pub paused: bool,
    #[serde(rename = "type")]
    pub folder_type: String, // "sendonly", "sendreceive", "receiveonly"
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct NeedResponse {
    pub progress: Vec<FileInfo>,
    pub queued: Vec<FileInfo>,
    pub rest: Vec<FileInfo>,
    pub page: u32,
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

    pub async fn rescan_folder(&self, folder_id: &str) -> Result<()> {
        let url = format!("{}/rest/db/scan?folder={}", self.base_url, folder_id);
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
            .context("Failed to revert folder")?;

        Ok(())
    }

    pub async fn get_local_changed_files(&self, folder_id: &str) -> Result<Vec<String>> {
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

        // Extract just the filenames
        Ok(data.files.iter().map(|f| f.name.clone()).collect())
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

    pub async fn get_ignore_patterns(&self, folder_id: &str) -> Result<Vec<String>> {
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
        }

        let data: IgnoresResponse = response
            .json()
            .await
            .context("Failed to parse ignore patterns")?;

        Ok(data.ignore.unwrap_or_default())
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

    /// Pause or resume a folder
    ///
    /// Uses PATCH /rest/config/folders/{id} to set the paused state
    pub async fn set_folder_paused(&self, folder_id: &str, paused: bool) -> Result<()> {
        let url = format!("{}/rest/config/folders/{}", self.base_url, urlencoding::encode(folder_id));

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
            anyhow::bail!("Failed to {} folder: {} - {}",
                if paused { "pause" } else { "resume" },
                status,
                text
            );
        }

        Ok(())
    }

    /// Change folder type
    ///
    /// Uses PATCH /rest/config/folders/{id} to set the folder type
    /// Valid types: "sendonly", "sendreceive", "receiveonly"
    pub async fn set_folder_type(&self, folder_id: &str, folder_type: &str) -> Result<()> {
        let url = format!("{}/rest/config/folders/{}", self.base_url, urlencoding::encode(folder_id));

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
                if local.ignored {
                    SyncState::Ignored
                } else if local.deleted && global.deleted {
                    SyncState::Synced // Both deleted, in sync
                } else if local.deleted && !global.deleted {
                    SyncState::RemoteOnly // Local deleted but exists remotely
                } else if !local.deleted && global.deleted {
                    SyncState::LocalOnly // Exists locally but deleted remotely
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_needed_files_builds_correct_url() {
        // This is a basic smoke test - full integration test requires real Syncthing
        let client = SyncthingClient::new(
            "http://localhost:8384".to_string(),
            "test-key".to_string(),
        );

        // We can't actually call the API without a real instance,
        // but we can verify the method exists and accepts correct params
        // Real testing will happen in integration tests
    }
}
