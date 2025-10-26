use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use crate::api::{BrowseItem, FileDetails, Folder, FolderStatus, SyncthingClient};

/// Priority level for API requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,   // User-initiated actions (navigation, toggle ignore)
    Medium, // Visible items (current directory contents)
    Low,    // Prefetching, background updates
}

/// Unique identifier for deduplicating requests
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RequestKey {
    Browse { folder_id: String, prefix: Option<String> },
    FileInfo { folder_id: String, file_path: String },
    FolderStatus { folder_id: String },
    IgnorePatterns { folder_id: String },
}

/// API request types
#[derive(Debug, Clone)]
pub enum ApiRequest {
    /// Browse folder contents
    BrowseFolder {
        folder_id: String,
        prefix: Option<String>,
        priority: Priority,
    },

    /// Get detailed file information
    GetFileInfo {
        folder_id: String,
        file_path: String,
        priority: Priority,
    },

    /// Get folder sync status
    GetFolderStatus {
        folder_id: String,
    },

    /// Get ignore patterns for folder
    GetIgnorePatterns {
        folder_id: String,
    },

    /// Set ignore patterns (always high priority)
    SetIgnorePatterns {
        folder_id: String,
        patterns: Vec<String>,
    },

    /// Trigger folder rescan (always high priority)
    RescanFolder {
        folder_id: String,
    },

    /// Revert folder to remote state (always high priority)
    RevertFolder {
        folder_id: String,
    },

    /// Get locally changed files in receive-only folder
    GetLocalChangedFiles {
        folder_id: String,
    },
}

impl ApiRequest {
    /// Extract priority from request
    fn priority(&self) -> Priority {
        match self {
            ApiRequest::BrowseFolder { priority, .. } => *priority,
            ApiRequest::GetFileInfo { priority, .. } => *priority,
            // All other operations are high priority
            _ => Priority::High,
        }
    }

    /// Generate a unique key for deduplication
    fn key(&self) -> RequestKey {
        match self {
            ApiRequest::BrowseFolder { folder_id, prefix, .. } => {
                RequestKey::Browse {
                    folder_id: folder_id.clone(),
                    prefix: prefix.clone(),
                }
            }
            ApiRequest::GetFileInfo { folder_id, file_path, .. } => {
                RequestKey::FileInfo {
                    folder_id: folder_id.clone(),
                    file_path: file_path.clone(),
                }
            }
            ApiRequest::GetFolderStatus { folder_id } => {
                RequestKey::FolderStatus {
                    folder_id: folder_id.clone(),
                }
            }
            ApiRequest::GetIgnorePatterns { folder_id } => {
                RequestKey::IgnorePatterns {
                    folder_id: folder_id.clone(),
                }
            }
            // Write operations don't deduplicate
            _ => RequestKey::Browse {
                folder_id: format!("write-{:?}", std::time::Instant::now()),
                prefix: None,
            },
        }
    }
}

/// API response types
#[derive(Debug, Clone)]
pub enum ApiResponse {
    BrowseResult {
        folder_id: String,
        prefix: Option<String>,
        items: Result<Vec<BrowseItem>, String>,
    },

    FileInfoResult {
        folder_id: String,
        file_path: String,
        details: Result<FileDetails, String>,
    },

    FolderStatusResult {
        folder_id: String,
        status: Result<FolderStatus, String>,
    },

    IgnorePatternsResult {
        folder_id: String,
        patterns: Result<Vec<String>, String>,
    },

    SetIgnorePatternsResult {
        folder_id: String,
        success: bool,
        error: Option<String>,
    },

    RescanResult {
        folder_id: String,
        success: bool,
        error: Option<String>,
    },

    RevertResult {
        folder_id: String,
        success: bool,
        error: Option<String>,
    },

    LocalChangedFilesResult {
        folder_id: String,
        files: Result<Vec<String>, String>,
    },
}

/// API service worker that processes requests in the background
pub struct ApiService {
    client: SyncthingClient,
    request_queue: VecDeque<(ApiRequest, Priority)>,
    in_flight: HashSet<RequestKey>,
    response_tx: mpsc::UnboundedSender<ApiResponse>,
    max_concurrent: usize,
}

impl ApiService {
    pub fn new(
        client: SyncthingClient,
        response_tx: mpsc::UnboundedSender<ApiResponse>,
    ) -> Self {
        Self {
            client,
            request_queue: VecDeque::new(),
            in_flight: HashSet::new(),
            response_tx,
            max_concurrent: 10, // Limit concurrent API calls
        }
    }

    /// Add a request to the queue
    fn enqueue(&mut self, request: ApiRequest) {
        let key = request.key();

        // Don't queue if already in flight (deduplication)
        if self.in_flight.contains(&key) {
            return;
        }

        let priority = request.priority();

        // Insert based on priority (high priority at front)
        let insert_pos = self.request_queue
            .iter()
            .position(|(_, p)| *p < priority)
            .unwrap_or(self.request_queue.len());

        self.request_queue.insert(insert_pos, (request, priority));
    }

    /// Process the next request from the queue
    async fn process_next(&mut self) {
        if self.in_flight.len() >= self.max_concurrent {
            return; // At capacity, wait for some to complete
        }

        let Some((request, _)) = self.request_queue.pop_front() else {
            return; // Queue is empty
        };

        let key = request.key();
        self.in_flight.insert(key.clone());

        // Clone what we need for the async task
        let client = self.client.clone();
        let response_tx = self.response_tx.clone();

        // Spawn task to handle this request
        tokio::spawn(async move {
            let response = Self::execute_request(&client, request).await;
            let _ = response_tx.send(response);
        });

        // Note: We remove from in_flight when the UI receives the response
        // This is handled by the main app loop
    }

    /// Execute an API request and return the response
    async fn execute_request(client: &SyncthingClient, request: ApiRequest) -> ApiResponse {
        match request {
            ApiRequest::BrowseFolder { folder_id, prefix, .. } => {
                let items = client.browse_folder(&folder_id, prefix.as_deref()).await
                    .map_err(|e| e.to_string());

                ApiResponse::BrowseResult {
                    folder_id,
                    prefix,
                    items,
                }
            }

            ApiRequest::GetFileInfo { folder_id, file_path, .. } => {
                let details = client.get_file_info(&folder_id, &file_path).await
                    .map_err(|e| e.to_string());

                ApiResponse::FileInfoResult {
                    folder_id,
                    file_path,
                    details,
                }
            }

            ApiRequest::GetFolderStatus { folder_id } => {
                let status = client.get_folder_status(&folder_id).await
                    .map_err(|e| e.to_string());

                ApiResponse::FolderStatusResult {
                    folder_id,
                    status,
                }
            }

            ApiRequest::GetIgnorePatterns { folder_id } => {
                let patterns = client.get_ignore_patterns(&folder_id).await
                    .map_err(|e| e.to_string());

                ApiResponse::IgnorePatternsResult {
                    folder_id,
                    patterns,
                }
            }

            ApiRequest::SetIgnorePatterns { folder_id, patterns } => {
                match client.set_ignore_patterns(&folder_id, patterns).await {
                    Ok(()) => ApiResponse::SetIgnorePatternsResult {
                        folder_id,
                        success: true,
                        error: None,
                    },
                    Err(e) => ApiResponse::SetIgnorePatternsResult {
                        folder_id,
                        success: false,
                        error: Some(e.to_string()),
                    },
                }
            }

            ApiRequest::RescanFolder { folder_id } => {
                match client.rescan_folder(&folder_id).await {
                    Ok(()) => ApiResponse::RescanResult {
                        folder_id,
                        success: true,
                        error: None,
                    },
                    Err(e) => ApiResponse::RescanResult {
                        folder_id,
                        success: false,
                        error: Some(e.to_string()),
                    },
                }
            }

            ApiRequest::RevertFolder { folder_id } => {
                match client.revert_folder(&folder_id).await {
                    Ok(()) => ApiResponse::RevertResult {
                        folder_id,
                        success: true,
                        error: None,
                    },
                    Err(e) => ApiResponse::RevertResult {
                        folder_id,
                        success: false,
                        error: Some(e.to_string()),
                    },
                }
            }

            ApiRequest::GetLocalChangedFiles { folder_id } => {
                let files = client.get_local_changed_files(&folder_id).await
                    .map_err(|e| e.to_string());

                ApiResponse::LocalChangedFilesResult {
                    folder_id,
                    files,
                }
            }
        }
    }

    /// Mark a request as completed (called when UI receives response)
    pub fn mark_completed(&mut self, response: &ApiResponse) {
        let key = match response {
            ApiResponse::BrowseResult { folder_id, prefix, .. } => {
                RequestKey::Browse {
                    folder_id: folder_id.clone(),
                    prefix: prefix.clone(),
                }
            }
            ApiResponse::FileInfoResult { folder_id, file_path, .. } => {
                RequestKey::FileInfo {
                    folder_id: folder_id.clone(),
                    file_path: file_path.clone(),
                }
            }
            ApiResponse::FolderStatusResult { folder_id, .. } => {
                RequestKey::FolderStatus {
                    folder_id: folder_id.clone(),
                }
            }
            ApiResponse::IgnorePatternsResult { folder_id, .. } => {
                RequestKey::IgnorePatterns {
                    folder_id: folder_id.clone(),
                }
            }
            // Write operations don't track in-flight status
            _ => return,
        };

        self.in_flight.remove(&key);
    }
}

/// Spawn the API service worker
pub fn spawn_api_service(
    client: SyncthingClient,
) -> (mpsc::UnboundedSender<ApiRequest>, mpsc::UnboundedReceiver<ApiResponse>) {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<ApiRequest>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<ApiResponse>();

    tokio::spawn(async move {
        let mut service = ApiService::new(client, response_tx);

        // Ticker for processing queue
        let mut tick = interval(Duration::from_millis(10));

        loop {
            tokio::select! {
                // Receive new requests
                Some(request) = request_rx.recv() => {
                    service.enqueue(request);
                }

                // Process queue at regular intervals
                _ = tick.tick() => {
                    // Process multiple requests per tick if queue has items
                    for _ in 0..5 {
                        if service.request_queue.is_empty() {
                            break;
                        }
                        service.process_next().await;
                    }
                }
            }
        }
    });

    (request_tx, response_rx)
}
