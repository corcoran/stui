use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Folder {
    pub id: String,
    pub label: Option<String>,
    #[allow(dead_code)]
    pub path: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize)]
struct ConfigResponse {
    folders: Vec<Folder>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BrowseItem {
    pub name: String,
    #[serde(rename = "type")]
    pub item_type: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderStatus {
    pub state: String,
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
}

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

    pub async fn get_folders(&self) -> Result<Vec<Folder>> {
        let url = format!("{}/rest/system/config", self.base_url);
        let response = self
            .client
            .get(&url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .context("Failed to fetch system config")?;

        let config: ConfigResponse = response
            .json()
            .await
            .context("Failed to parse system config")?;

        Ok(config.folders)
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
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("API error: {}", error_text));
        }

        // Try to parse as JSON, handle paused/unavailable folders
        let text = response.text().await.context("Failed to read response")?;
        if text.contains("no such folder") || text.contains("paused") {
            // Return empty list for paused/unavailable folders
            return Ok(Vec::new());
        }

        let items: Vec<BrowseItem> = serde_json::from_str(&text)
            .context("Failed to parse browse response")?;

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
}
