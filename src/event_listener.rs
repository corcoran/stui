use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::sync::mpsc;

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

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SyncthingEvent {
    pub id: u64,
    #[serde(rename = "globalID")]
    pub global_id: u64,
    pub time: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum CacheInvalidation {
    /// Invalidate a single file
    File { folder_id: String, file_path: String },
    /// Invalidate an entire directory
    Directory { folder_id: String, dir_path: String },
}

/// Spawn the event listener task
pub fn spawn_event_listener(
    base_url: String,
    api_key: String,
    last_event_id: u64,
    invalidation_tx: mpsc::UnboundedSender<CacheInvalidation>,
    event_id_tx: mpsc::UnboundedSender<u64>,
) {
    tokio::spawn(async move {
        if let Err(e) = event_listener_loop(
            base_url,
            api_key,
            last_event_id,
            invalidation_tx,
            event_id_tx,
        )
        .await
        {
            eprintln!("Event listener fatal error: {}", e);
        }
    });
}

async fn event_listener_loop(
    base_url: String,
    api_key: String,
    mut last_event_id: u64,
    invalidation_tx: mpsc::UnboundedSender<CacheInvalidation>,
    event_id_tx: mpsc::UnboundedSender<u64>,
) -> Result<()> {
    let client = Client::new();

    log_debug(&format!("DEBUG [EVENT LISTENER]: Starting event listener, base_url={} last_event_id={}", base_url, last_event_id));

    loop {
        // Make long-polling request
        let url = format!("{}/rest/events?since={}&timeout=60", base_url, last_event_id);
        log_debug(&format!("DEBUG [EVENT LISTENER]: Polling {}", url));

        match client
            .get(&url)
            .header("X-API-Key", &api_key)
            .send()
            .await
        {
            Ok(response) => {
                log_debug(&format!("DEBUG [EVENT LISTENER]: Got response, status={}", response.status()));

                if let Ok(events) = response.json::<Vec<SyncthingEvent>>().await {
                    log_debug(&format!("DEBUG [EVENT LISTENER]: Received {} events", events.len()));

                    for event in &events {
                        // Debug: Log all events
                        log_debug(&format!("DEBUG [EVENT]: id={} type={} data={}", event.id, event.event_type, event.data));

                        // Check for missed events (gap in IDs)
                        if event.id != last_event_id + 1 && last_event_id > 0 {
                            log_debug(&format!("DEBUG [EVENT]: WARNING - Missed events! Last ID: {}, Current ID: {}", last_event_id, event.id));
                        }

                        // Process events we care about
                        match event.event_type.as_str() {
                            "LocalIndexUpdated" => {
                                // LocalIndexUpdated has a "filenames" array instead of "item"
                                if let Some(folder_id) = event.data.get("folder").and_then(|v| v.as_str()) {
                                    if let Some(filenames) = event.data.get("filenames").and_then(|v| v.as_array()) {
                                        for filename in filenames {
                                            if let Some(file_path) = filename.as_str() {
                                                let invalidation = CacheInvalidation::File {
                                                    folder_id: folder_id.to_string(),
                                                    file_path: file_path.to_string(),
                                                };

                                                log_debug(&format!("DEBUG [EVENT]: Sending invalidation: {:?}", invalidation));
                                                let _ = invalidation_tx.send(invalidation);
                                            }
                                        }
                                    }
                                }
                            }
                            "ItemFinished" | "LocalChangeDetected" | "RemoteChangeDetected" => {
                                if let Some(folder_id) = event.data.get("folder").and_then(|v| v.as_str()) {
                                    if let Some(item_path) = event.data.get("item").and_then(|v| v.as_str()) {
                                        // Check if it's a directory
                                        let item_type = event.data.get("type").and_then(|v| v.as_str());

                                        let invalidation = if item_type == Some("dir") || item_path.ends_with('/') {
                                            // Directory change - invalidate entire directory
                                            CacheInvalidation::Directory {
                                                folder_id: folder_id.to_string(),
                                                dir_path: item_path.to_string(),
                                            }
                                        } else {
                                            // File change - invalidate single file
                                            CacheInvalidation::File {
                                                folder_id: folder_id.to_string(),
                                                file_path: item_path.to_string(),
                                            }
                                        };

                                        log_debug(&format!("DEBUG [EVENT]: Sending invalidation: {:?}", invalidation));
                                        let _ = invalidation_tx.send(invalidation);
                                    }
                                }
                            }
                            _ => {
                                // Ignore other event types (but log them for debugging)
                                log_debug(&format!("DEBUG [EVENT]: Ignoring event type: {}", event.event_type));
                            }
                        }

                        last_event_id = event.id;
                    }

                    // Persist event ID every batch (not every single event for performance)
                    if !events.is_empty() {
                        log_debug(&format!("DEBUG [EVENT LISTENER]: Persisting last_event_id={}", last_event_id));
                        let _ = event_id_tx.send(last_event_id);
                    }
                } else {
                    log_debug("DEBUG [EVENT LISTENER]: Failed to parse events JSON");
                }
            }
            Err(e) => {
                log_debug(&format!("DEBUG [EVENT LISTENER]: Connection error: {}", e));
                // Wait before retrying
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        // Loop continues immediately - long-polling pattern
        // If timeout occurred (empty array), we keep same last_event_id and try again
    }
}
