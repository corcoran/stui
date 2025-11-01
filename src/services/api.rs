use anyhow::Result;
use std::collections::{HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use crate::api::{
    BrowseItem, ConnectionStats, Device, FileDetails, FolderStatus, SyncthingClient, SystemStatus,
};

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
    ConnectionStats,
    Devices,
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
    GetFolderStatus { folder_id: String },

    /// Trigger folder rescan (always high priority)
    RescanFolder { folder_id: String },

    /// Get system status (device info, uptime)
    GetSystemStatus,

    /// Get global connection/transfer statistics
    GetConnectionStats,

    /// Get list of all devices
    GetDevices,
}

impl ApiRequest {
    /// Extract priority from request
    fn priority(&self) -> Priority {
        match self {
            ApiRequest::BrowseFolder { priority, .. } => *priority,
            ApiRequest::GetFileInfo { priority, .. } => *priority,
            // Folder status polling is medium priority (background refresh)
            ApiRequest::GetFolderStatus { .. } => Priority::Medium,
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
            ApiRequest::GetFolderStatus { folder_id } => RequestKey::FolderStatus {
                folder_id: folder_id.clone(),
            },
            // Write operations don't deduplicate
            ApiRequest::RescanFolder { .. } => RequestKey::Browse {
                folder_id: format!("write-{:?}", std::time::Instant::now()),
                prefix: None,
            },
            ApiRequest::GetSystemStatus => RequestKey::SystemStatus,
            ApiRequest::GetConnectionStats => RequestKey::ConnectionStats,
            ApiRequest::GetDevices => RequestKey::Devices,
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
    },

    FileInfoResult {
        folder_id: String,
        file_path: String,
        details: Result<FileDetails, anyhow::Error>,
    },

    FolderStatusResult {
        folder_id: String,
        status: Result<FolderStatus, anyhow::Error>,
    },

    RescanResult {
        folder_id: String,
        success: bool,
        error: Option<anyhow::Error>,
    },

    SystemStatusResult {
        status: Result<SystemStatus, anyhow::Error>,
    },

    ConnectionStatsResult {
        stats: Result<ConnectionStats, anyhow::Error>,
    },

    DevicesResult {
        devices: Result<Vec<Device>, anyhow::Error>,
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
    pub fn new(
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
        let insert_pos = self
            .request_queue
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
            match &response {
                ApiResponse::FileInfoResult {
                    folder_id,
                    file_path,
                    ..
                } => {
                    log_debug(&format!(
                        "DEBUG [API Service]: Sending FileInfoResult for folder={} path={}",
                        folder_id, file_path
                    ));
                }
                _ => {}
            }

            let _ = response_tx.send(response);

            // Notify service that this request is complete
            let _ = completion_tx.send(InternalMessage::Completed(completion_key));
        });
    }

    /// Execute an API request and return the response
    async fn execute_request(client: &SyncthingClient, request: ApiRequest) -> ApiResponse {
        match request {
            ApiRequest::BrowseFolder {
                folder_id, prefix, ..
            } => {
                let items = client
                    .browse_folder(&folder_id, prefix.as_deref())
                    .await;

                ApiResponse::BrowseResult {
                    folder_id,
                    prefix,
                    items,
                }
            }

            ApiRequest::GetFileInfo {
                folder_id,
                file_path,
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
                    details,
                }
            }

            ApiRequest::GetFolderStatus { folder_id } => {
                let status = client
                    .get_folder_status(&folder_id)
                    .await;

                ApiResponse::FolderStatusResult { folder_id, status }
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
                        error: Some(e),
                    },
                }
            }

            ApiRequest::GetSystemStatus => {
                let status = client.get_system_status().await;

                ApiResponse::SystemStatusResult { status }
            }

            ApiRequest::GetConnectionStats => {
                let stats = client
                    .get_connection_stats()
                    .await;

                ApiResponse::ConnectionStatsResult { stats }
            }

            ApiRequest::GetDevices => {
                let devices = client.get_devices().await;

                ApiResponse::DevicesResult { devices }
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
