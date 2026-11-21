use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

use crate::api::{
    BrowseItem, ConnectionStats, Device, FileDetails, FolderStatus, LocalChangedFile, NeedResponse,
    SyncthingClient, SystemStatus,
};
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

/// Page size of the need fetch behind the syncing set and the need index. Pagination spans
/// progress, queued and rest in that order; a folder needing more items gets an incomplete index.
pub const NEED_INDEX_PER_PAGE: u32 = 1000;

/// Index to insert a request of `priority` so the queue stays highest-priority first
/// (`High < Medium < Low` in `Ord`) and FIFO within a priority.
fn queue_insert_position(queued: impl Iterator<Item = Priority>, priority: Priority) -> usize {
    let mut len = 0;
    for (idx, p) in queued.enumerate() {
        if p > priority {
            return idx;
        }
        len = idx + 1;
    }
    len
}

/// Priority level for API requests
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    High,   // User-initiated actions (navigation, toggle ignore)
    Medium, // Visible items (current directory contents)
    Low,    // Prefetching, background updates
}

/// Unique identifier for deduplicating requests
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RequestKey {
    Browse {
        folder_id: String,
        prefix: Option<String>,
    },
    FileInfo {
        folder_id: String,
        file_path: String,
    },
    FolderStatus {
        folder_id: String,
    },
    SystemStatus,
    Version,
    ConnectionStats,
    Devices,
    NeededFiles {
        folder_id: String,
        generation: u64,
        page: u32,
    },
    LocalChanged {
        folder_id: String,
    },
    FolderNeed {
        folder_id: String,
        requested_at: std::time::Instant,
    },
    LocalChangedIndex {
        folder_id: String,
        sequence: u64,
    },
}

/// API request types
#[derive(Debug, Clone)]
pub enum ApiRequest {
    /// Browse folder contents
    BrowseFolder {
        folder_id: String,
        prefix: Option<String>,
        priority: Priority,
        requested_at: std::time::Instant,
    },

    /// Get detailed file information
    GetFileInfo {
        folder_id: String,
        file_path: String,
        priority: Priority,
        requested_at: std::time::Instant,
    },

    /// Get folder sync status
    GetFolderStatus {
        folder_id: String,
        requested_at: std::time::Instant,
    },

    /// Trigger folder rescan (always high priority). Empty `paths` scans the whole folder,
    /// otherwise only those folder-relative paths.
    RescanFolder {
        folder_id: String,
        paths: Vec<String>,
    },

    /// Get system status (device info, uptime)
    GetSystemStatus,

    /// Get the Syncthing version string
    GetVersion,

    /// Get global connection/transfer statistics
    GetConnectionStats,

    /// Get list of all devices
    GetDevices,

    /// One page of /rest/db/need for the out-of-sync filter. `generation` and
    /// `sequence` come from the folder's `NeedPaging` and identify the fetch the page belongs to.
    /// Pages after the first are low priority.
    GetNeededFiles {
        folder_id: String,
        page: u32,
        perpage: u32,
        generation: u64,
        sequence: u64,
    },

    /// Get local changed files (receive-only folders)
    GetLocalChanged { folder_id: String },

    /// First page of /rest/db/need plus the expanded ignore patterns, feeding the syncing set
    /// and the need index. `requested_at` identifies the request (see `accept_need_response`).
    GetFolderNeed {
        folder_id: String,
        sequence: u64,
        requested_at: std::time::Instant,
    },

    /// /rest/db/localchanged for the need index (receive-only folders)
    GetLocalChangedIndex {
        folder_id: String,
        sequence: u64,
        requested_at: std::time::Instant,
    },
}

impl ApiRequest {
    /// Extract priority from request
    pub(crate) fn priority(&self) -> Priority {
        match self {
            ApiRequest::BrowseFolder { priority, .. } => *priority,
            ApiRequest::GetFileInfo { priority, .. } => *priority,
            // Folder status polling is medium priority (background refresh)
            ApiRequest::GetFolderStatus { .. } => Priority::Medium,
            ApiRequest::GetNeededFiles { page, .. } if *page > 1 => Priority::Low,
            // All other operations (SystemStatus, Devices, Rescan, etc) are high priority
            _ => Priority::High,
        }
    }

    /// Generate a unique key for deduplication
    fn key(&self) -> RequestKey {
        match self {
            ApiRequest::BrowseFolder {
                folder_id, prefix, ..
            } => RequestKey::Browse {
                folder_id: folder_id.clone(),
                prefix: prefix.clone(),
            },
            ApiRequest::GetFileInfo {
                folder_id,
                file_path,
                ..
            } => RequestKey::FileInfo {
                folder_id: folder_id.clone(),
                file_path: file_path.clone(),
            },
            ApiRequest::GetFolderStatus { folder_id, .. } => RequestKey::FolderStatus {
                folder_id: folder_id.clone(),
            },
            // Write operations don't deduplicate
            ApiRequest::RescanFolder { .. } => RequestKey::Browse {
                folder_id: format!("write-{:?}", std::time::Instant::now()),
                prefix: None,
            },
            ApiRequest::GetSystemStatus => RequestKey::SystemStatus,
            ApiRequest::GetVersion => RequestKey::Version,
            ApiRequest::GetConnectionStats => RequestKey::ConnectionStats,
            ApiRequest::GetDevices => RequestKey::Devices,
            ApiRequest::GetNeededFiles {
                folder_id,
                generation,
                page,
                ..
            } => RequestKey::NeededFiles {
                folder_id: folder_id.clone(),
                generation: *generation,
                page: *page,
            },
            ApiRequest::GetLocalChanged { folder_id } => RequestKey::LocalChanged {
                folder_id: folder_id.clone(),
            },
            ApiRequest::GetFolderNeed {
                folder_id,
                requested_at,
                ..
            } => RequestKey::FolderNeed {
                folder_id: folder_id.clone(),
                requested_at: *requested_at,
            },
            ApiRequest::GetLocalChangedIndex {
                folder_id,
                sequence,
                ..
            } => RequestKey::LocalChangedIndex {
                folder_id: folder_id.clone(),
                sequence: *sequence,
            },
        }
    }
}

/// API response types
#[derive(Debug)]
pub enum ApiResponse {
    BrowseResult {
        folder_id: String,
        prefix: Option<String>,
        items: Result<Vec<BrowseItem>, anyhow::Error>,
        requested_at: std::time::Instant,
    },

    FileInfoResult {
        folder_id: String,
        file_path: String,
        details: Box<Result<FileDetails, anyhow::Error>>,
        requested_at: std::time::Instant,
    },

    FolderStatusResult {
        folder_id: String,
        status: Result<FolderStatus, anyhow::Error>,
        requested_at: std::time::Instant,
    },

    RescanResult {
        folder_id: String,
        success: bool,
        error: Option<anyhow::Error>,
    },

    SystemStatusResult {
        status: Result<SystemStatus, anyhow::Error>,
    },

    VersionResult {
        version: Result<String, anyhow::Error>,
    },

    ConnectionStatsResult {
        stats: Result<ConnectionStats, anyhow::Error>,
    },

    DevicesResult {
        devices: Result<Vec<Device>, anyhow::Error>,
    },

    NeededFiles {
        folder_id: String,
        generation: u64,
        sequence: u64,
        page: u32,
        perpage: u32,
        response: Result<NeedResponse, anyhow::Error>,
    },

    LocalChanged {
        folder_id: String,
        file_paths: Vec<String>,
    },

    FolderNeed {
        folder_id: String,
        sequence: u64,
        requested_at: std::time::Instant,
        need: Result<NeedResponse, anyhow::Error>,
        ignore_patterns: Result<Vec<String>, anyhow::Error>,
    },

    LocalChangedIndex {
        folder_id: String,
        sequence: u64,
        requested_at: std::time::Instant,
        files: Result<Vec<LocalChangedFile>, anyhow::Error>,
    },
}

/// Internal message for tracking completed requests
#[allow(dead_code)]
pub(crate) enum InternalMessage {
    Completed(RequestKey),
}

/// API service worker that processes requests in the background
pub struct ApiService {
    client: SyncthingClient,
    request_queue: VecDeque<(ApiRequest, Priority)>,
    in_flight: HashSet<RequestKey>,
    response_tx: mpsc::UnboundedSender<ApiResponse>,
    completion_tx: mpsc::UnboundedSender<InternalMessage>,
    max_concurrent: usize,
}

impl ApiService {
    pub(crate) fn new(
        client: SyncthingClient,
        response_tx: mpsc::UnboundedSender<ApiResponse>,
        completion_tx: mpsc::UnboundedSender<InternalMessage>,
    ) -> Self {
        Self {
            client,
            request_queue: VecDeque::new(),
            in_flight: HashSet::new(),
            response_tx,
            completion_tx,
            max_concurrent: 10, // Limit concurrent API calls
        }
    }

    /// Add a request to the queue
    fn enqueue(&mut self, request: ApiRequest) {
        // NOTE: We removed in_flight deduplication because it was never being cleared
        // when responses completed. Deduplication is now handled by loading_sync_states
        // in main.rs before requests are even sent.

        let priority = request.priority();

        // Insert based on priority (high priority at front)
        let insert_pos =
            queue_insert_position(self.request_queue.iter().map(|(_, p)| *p), priority);

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

        // Track in-flight for concurrency limiting
        let key = request.key();
        self.in_flight.insert(key.clone());

        // Clone what we need for the async task
        let client = self.client.clone();
        let response_tx = self.response_tx.clone();
        let completion_tx = self.completion_tx.clone();
        let completion_key = key.clone();

        // Spawn task to handle this request
        // Note: No per-request retries - background reconnection handles that
        tokio::spawn(async move {
            let response = Self::execute_request(&client, request).await;

            // Log before sending response
            if let ApiResponse::FileInfoResult {
                folder_id,
                file_path,
                ..
            } = &response
            {
                log_debug(&format!(
                    "DEBUG [API Service]: Sending FileInfoResult for folder={} path={}",
                    folder_id, file_path
                ));
            }

            let _ = response_tx.send(response);

            // Notify service that this request is complete
            let _ = completion_tx.send(InternalMessage::Completed(completion_key));
        });
    }

    /// Helper to get local changed file paths
    async fn get_local_changed_files(
        client: &SyncthingClient,
        folder_id: &str,
    ) -> Result<Vec<String>> {
        // Use get_local_changed_files() which includes deleted files
        // NOT get_local_changed_items() which filters them out for browse UI
        client.get_local_changed_files(folder_id).await
    }

    /// Execute an API request and return the response
    async fn execute_request(client: &SyncthingClient, request: ApiRequest) -> ApiResponse {
        match request {
            ApiRequest::BrowseFolder {
                folder_id,
                prefix,
                requested_at,
                ..
            } => {
                let items = client.browse_folder(&folder_id, prefix.as_deref()).await;

                ApiResponse::BrowseResult {
                    folder_id,
                    prefix,
                    items,
                    requested_at,
                }
            }

            ApiRequest::GetFileInfo {
                folder_id,
                file_path,
                requested_at,
                ..
            } => {
                log_debug(&format!(
                    "DEBUG [API Service GetFileInfo]: START folder={} path={}",
                    folder_id, file_path
                ));
                let details = client
                    .get_file_info(&folder_id, &file_path)
                    .await
                    .map_err(|e| {
                        log_debug(&format!(
                            "DEBUG [API Service GetFileInfo]: ERROR folder={} path={} error={}",
                            folder_id, file_path, e
                        ));
                        e
                    });

                log_debug(&format!(
                    "DEBUG [API Service GetFileInfo]: END folder={} path={} success={}",
                    folder_id,
                    file_path,
                    details.is_ok()
                ));

                ApiResponse::FileInfoResult {
                    folder_id,
                    file_path,
                    details: Box::new(details),
                    requested_at,
                }
            }

            ApiRequest::GetFolderStatus {
                folder_id,
                requested_at,
            } => {
                let status = client.get_folder_status(&folder_id).await;

                ApiResponse::FolderStatusResult {
                    folder_id,
                    status,
                    requested_at,
                }
            }

            ApiRequest::RescanFolder { folder_id, paths } => {
                match client.rescan_paths(&folder_id, &paths).await {
                    Ok(()) => ApiResponse::RescanResult {
                        folder_id,
                        success: true,
                        error: None,
                    },
                    Err(e) => ApiResponse::RescanResult {
                        folder_id,
                        success: false,
                        error: Some(e),
                    },
                }
            }

            ApiRequest::GetSystemStatus => {
                let status = client.get_system_status().await;

                ApiResponse::SystemStatusResult { status }
            }

            ApiRequest::GetVersion => ApiResponse::VersionResult {
                version: client.get_version().await,
            },

            ApiRequest::GetConnectionStats => {
                let stats = client.get_connection_stats().await;

                ApiResponse::ConnectionStatsResult { stats }
            }

            ApiRequest::GetDevices => {
                let devices = client.get_devices().await;

                ApiResponse::DevicesResult { devices }
            }

            ApiRequest::GetNeededFiles {
                folder_id,
                page,
                perpage,
                generation,
                sequence,
            } => {
                let response = client
                    .get_needed_files(&folder_id, Some(page), Some(perpage))
                    .await;
                if let Err(e) = &response {
                    log_debug(&format!(
                        "Failed to get needed files for {} page {}: {}",
                        folder_id, page, e
                    ));
                }
                ApiResponse::NeededFiles {
                    folder_id,
                    generation,
                    sequence,
                    page,
                    perpage,
                    response,
                }
            }

            ApiRequest::GetLocalChanged { folder_id } => {
                match Self::get_local_changed_files(client, &folder_id).await {
                    Ok(file_paths) => ApiResponse::LocalChanged {
                        folder_id: folder_id.clone(),
                        file_paths,
                    },
                    Err(e) => {
                        log_debug(&format!(
                            "Failed to get local changed files for {}: {}",
                            folder_id, e
                        ));
                        // Return empty response on error
                        ApiResponse::LocalChanged {
                            folder_id,
                            file_paths: vec![],
                        }
                    }
                }
            }

            ApiRequest::GetFolderNeed {
                folder_id,
                sequence,
                requested_at,
            } => {
                let (need, ignore_patterns) = tokio::join!(
                    client.get_needed_files(&folder_id, Some(1), Some(NEED_INDEX_PER_PAGE)),
                    client.get_ignores(&folder_id),
                );
                let ignore_patterns = ignore_patterns.map(crate::api::Ignores::effective);
                if let Err(e) = &need {
                    log_debug(&format!("Failed to get need for {}: {}", folder_id, e));
                }
                if let Err(e) = &ignore_patterns {
                    log_debug(&format!("Failed to get ignores for {}: {}", folder_id, e));
                }
                ApiResponse::FolderNeed {
                    folder_id,
                    sequence,
                    requested_at,
                    need,
                    ignore_patterns,
                }
            }

            ApiRequest::GetLocalChangedIndex {
                folder_id,
                sequence,
                requested_at,
            } => {
                let files = client.get_local_changed_entries(&folder_id).await;
                if let Err(e) = &files {
                    log_debug(&format!(
                        "Failed to get local changes for {}: {}",
                        folder_id, e
                    ));
                }
                ApiResponse::LocalChangedIndex {
                    folder_id,
                    sequence,
                    requested_at,
                    files,
                }
            }
        }
    }
}

/// Spawn the API service worker
pub fn spawn_api_service(
    client: SyncthingClient,
) -> (
    mpsc::UnboundedSender<ApiRequest>,
    mpsc::UnboundedReceiver<ApiResponse>,
) {
    let (request_tx, mut request_rx) = mpsc::unbounded_channel::<ApiRequest>();
    let (response_tx, response_rx) = mpsc::unbounded_channel::<ApiResponse>();
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel::<InternalMessage>();

    tokio::spawn(async move {
        let mut service = ApiService::new(client, response_tx, completion_tx);

        // Ticker for processing queue
        let mut tick = interval(Duration::from_millis(10));

        loop {
            tokio::select! {
                // Receive new requests
                Some(request) = request_rx.recv() => {
                    service.enqueue(request);
                }

                // Handle completion notifications
                Some(InternalMessage::Completed(key)) = completion_rx.recv() => {
                    service.in_flight.remove(&key);
                    // Only log when there are still requests in flight (to reduce log spam)
                    if !service.in_flight.is_empty() {
                        log_debug(&format!("DEBUG [API Service]: Removed from in_flight, now {} in flight", service.in_flight.len()));
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // QUEUE ORDERING
    // ========================================

    fn insert(queue: &mut Vec<Priority>, p: Priority) {
        let pos = queue_insert_position(queue.iter().copied(), p);
        queue.insert(pos, p);
    }

    #[test]
    fn test_high_goes_ahead_of_lower_priorities() {
        let mut q = vec![Priority::Medium, Priority::Low];
        insert(&mut q, Priority::High);
        assert_eq!(q, vec![Priority::High, Priority::Medium, Priority::Low]);
    }

    #[test]
    fn test_low_goes_behind_higher_priorities() {
        let mut q = vec![Priority::High, Priority::Medium];
        insert(&mut q, Priority::Low);
        assert_eq!(q, vec![Priority::High, Priority::Medium, Priority::Low]);
    }

    #[test]
    fn test_same_priority_is_fifo() {
        let mut q = vec![Priority::High, Priority::Medium, Priority::Low];
        let pos = queue_insert_position(q.iter().copied(), Priority::Medium);
        assert_eq!(pos, 2, "new Medium goes after existing Medium, before Low");
        q.insert(pos, Priority::Medium);
        assert_eq!(
            q,
            vec![
                Priority::High,
                Priority::Medium,
                Priority::Medium,
                Priority::Low
            ]
        );
    }

    #[test]
    fn test_empty_queue_inserts_at_front() {
        assert_eq!(queue_insert_position(std::iter::empty(), Priority::Low), 0);
    }
}
