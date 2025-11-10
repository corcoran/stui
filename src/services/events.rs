use anyhow::Result;
use chrono::DateTime;
use reqwest::Client;
use serde::Deserialize;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};
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

/// Parse RFC3339 timestamp from Syncthing event into SystemTime
/// Falls back to current time if parsing fails
fn parse_event_time(time_str: &str) -> SystemTime {
    parse_event_time_public(time_str)
}

/// Public version of parse_event_time for use in other modules
pub fn parse_event_time_public(time_str: &str) -> SystemTime {
    if let Ok(dt) = DateTime::parse_from_rfc3339(time_str) {
        SystemTime::from(dt)
    } else {
        log_debug(&format!(
            "DEBUG [EVENT]: Failed to parse timestamp '{}', using current time",
            time_str
        ));
        SystemTime::now()
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
    File {
        folder_id: String,
        file_path: String,
        timestamp: std::time::SystemTime,
    },
    /// Invalidate an entire directory
    Directory {
        folder_id: String,
        dir_path: String,
        #[allow(dead_code)]
        timestamp: std::time::SystemTime,
    },
    /// Item started syncing
    ItemStarted {
        folder_id: String,
        file_path: String,
        #[allow(dead_code)]
        timestamp: std::time::SystemTime,
    },
    /// Item finished syncing
    ItemFinished {
        folder_id: String,
        file_path: String,
        #[allow(dead_code)]
        timestamp: std::time::SystemTime,
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event_time_valid_rfc3339() {
        // Test parsing a valid RFC3339 timestamp from Syncthing
        let timestamp_str = "2025-11-09T23:38:41.765733116Z";
        let parsed = parse_event_time(timestamp_str);

        // Verify it's not the fallback (current time) by checking it's in the past
        let now = SystemTime::now();
        assert!(
            parsed < now,
            "Parsed timestamp should be in the past, not current time"
        );

        // Verify the time is reasonable (within last 24 hours for this test)
        let duration_since = now.duration_since(parsed).unwrap();
        assert!(
            duration_since.as_secs() < 86400,
            "Timestamp should be recent (within 24 hours for test)"
        );
    }

    #[test]
    fn test_parse_event_time_invalid_falls_back() {
        // Test that invalid timestamps fall back to current time
        let invalid_str = "not-a-timestamp";
        let parsed = parse_event_time(invalid_str);

        // Should be very close to now (within 1 second)
        let now = SystemTime::now();
        let diff = now.duration_since(parsed).unwrap_or(Duration::from_secs(0));
        assert!(
            diff.as_secs() < 1,
            "Invalid timestamp should fall back to current time"
        );
    }

    #[test]
    fn test_parse_event_time_preserves_timestamp_accuracy() {
        // Test that we preserve the exact timestamp from the event
        // Use a specific timestamp from 2025-01-01
        let timestamp_str = "2025-01-01T12:00:00.123456789Z";
        let parsed = parse_event_time(timestamp_str);

        // Convert back to check it matches
        use chrono::DateTime;
        let dt: chrono::DateTime<chrono::FixedOffset> =
            DateTime::parse_from_rfc3339(timestamp_str).unwrap();
        let expected = SystemTime::from(dt);

        // Should be identical (within nanosecond precision)
        let diff = parsed
            .duration_since(expected)
            .unwrap_or_else(|e| e.duration());
        assert!(
            diff.as_nanos() == 0,
            "Timestamp should be parsed with full precision"
        );
    }
}

async fn event_listener_loop(
    base_url: String,
    api_key: String,
    mut last_event_id: u64,
    invalidation_tx: mpsc::UnboundedSender<CacheInvalidation>,
    event_id_tx: mpsc::UnboundedSender<u64>,
) -> Result<()> {
    let client = Client::new();

    log_debug(&format!(
        "DEBUG [EVENT LISTENER]: Starting event listener, base_url={} last_event_id={}",
        base_url, last_event_id
    ));

    // Track if we've tried resetting event ID (to avoid reset loop)
    let mut tried_reset = false;

    loop {
        // Make long-polling request
        let url = format!(
            "{}/rest/events?since={}&timeout=60",
            base_url, last_event_id
        );
        log_debug(&format!("DEBUG [EVENT LISTENER]: Polling {}", url));

        match client.get(&url).header("X-API-Key", &api_key).send().await {
            Ok(response) => {
                let status = response.status();
                log_debug(&format!(
                    "DEBUG [EVENT LISTENER]: Got response, status={}",
                    status
                ));

                match response.json::<Vec<SyncthingEvent>>().await {
                    Ok(events) => {
                        log_debug(&format!(
                            "DEBUG [EVENT LISTENER]: Received {} events",
                            events.len()
                        ));

                        // If we've been getting 0 events and last_event_id is high,
                        // Syncthing might have restarted - try resetting to 0 once
                        if events.is_empty() && last_event_id > 1000 && !tried_reset {
                            last_event_id = 0;
                            tried_reset = true;
                            // Persist the reset so next startup uses 0
                            let _ = event_id_tx.send(0);
                            continue;
                        }

                        for event in &events {
                            // Debug: Log all events
                            log_debug(&format!(
                                "DEBUG [EVENT]: id={} type={} data={}",
                                event.id, event.event_type, event.data
                            ));

                            // Check for missed events (gap in IDs)
                            if event.id != last_event_id + 1 && last_event_id > 0 {
                                log_debug(&format!("DEBUG [EVENT]: WARNING - Missed events! Last ID: {}, Current ID: {}", last_event_id, event.id));
                            }

                            // Process events we care about
                            match event.event_type.as_str() {
                                "LocalIndexUpdated" => {
                                    // LocalIndexUpdated has a "filenames" array instead of "item"
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                    {
                                        if let Some(filenames) =
                                            event.data.get("filenames").and_then(|v| v.as_array())
                                        {
                                            let timestamp = parse_event_time(&event.time);
                                            for filename in filenames {
                                                if let Some(file_path) = filename.as_str() {
                                                    let invalidation = CacheInvalidation::File {
                                                        folder_id: folder_id.to_string(),
                                                        file_path: file_path.to_string(),
                                                        timestamp,
                                                    };

                                                    log_debug(&format!(
                                                        "DEBUG [EVENT]: Sending invalidation: {:?}",
                                                        invalidation
                                                    ));
                                                    let _ = invalidation_tx.send(invalidation);
                                                }
                                            }
                                        }
                                    }
                                }
                                "ItemStarted" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                    {
                                        if let Some(item_path) =
                                            event.data.get("item").and_then(|v| v.as_str())
                                        {
                                            let timestamp = parse_event_time(&event.time);
                                            let invalidation = CacheInvalidation::ItemStarted {
                                                folder_id: folder_id.to_string(),
                                                file_path: item_path.to_string(),
                                                timestamp,
                                            };
                                            log_debug(&format!(
                                                "DEBUG [EVENT]: ItemStarted: {:?}",
                                                invalidation
                                            ));
                                            let _ = invalidation_tx.send(invalidation);
                                        }
                                    }
                                }
                                "ItemFinished" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                    {
                                        if let Some(item_path) =
                                            event.data.get("item").and_then(|v| v.as_str())
                                        {
                                            let timestamp = parse_event_time(&event.time);

                                            // Send ItemFinished notification
                                            let finished_invalidation =
                                                CacheInvalidation::ItemFinished {
                                                    folder_id: folder_id.to_string(),
                                                    file_path: item_path.to_string(),
                                                    timestamp,
                                                };
                                            log_debug(&format!(
                                                "DEBUG [EVENT]: ItemFinished: {:?}",
                                                finished_invalidation
                                            ));
                                            let _ = invalidation_tx.send(finished_invalidation);

                                            // Also send cache invalidation
                                            let item_type =
                                                event.data.get("type").and_then(|v| v.as_str());
                                            let cache_invalidation = if item_type == Some("dir")
                                                || item_path.ends_with('/')
                                            {
                                                CacheInvalidation::Directory {
                                                    folder_id: folder_id.to_string(),
                                                    dir_path: item_path.to_string(),
                                                    timestamp,
                                                }
                                            } else {
                                                CacheInvalidation::File {
                                                    folder_id: folder_id.to_string(),
                                                    file_path: item_path.to_string(),
                                                    timestamp,
                                                }
                                            };
                                            log_debug(&format!(
                                                "DEBUG [EVENT]: Sending cache invalidation: {:?}",
                                                cache_invalidation
                                            ));
                                            let _ = invalidation_tx.send(cache_invalidation);
                                        }
                                    }
                                }
                                "LocalChangeDetected" | "RemoteChangeDetected" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                    {
                                        if let Some(item_path) =
                                            event.data.get("item").and_then(|v| v.as_str())
                                        {
                                            let timestamp = parse_event_time(&event.time);
                                            // Check if it's a directory
                                            let item_type =
                                                event.data.get("type").and_then(|v| v.as_str());

                                            let invalidation = if item_type == Some("dir")
                                                || item_path.ends_with('/')
                                            {
                                                // Directory change - invalidate entire directory
                                                CacheInvalidation::Directory {
                                                    folder_id: folder_id.to_string(),
                                                    dir_path: item_path.to_string(),
                                                    timestamp,
                                                }
                                            } else {
                                                // File change - invalidate single file
                                                CacheInvalidation::File {
                                                    folder_id: folder_id.to_string(),
                                                    file_path: item_path.to_string(),
                                                    timestamp,
                                                }
                                            };

                                            log_debug(&format!(
                                                "DEBUG [EVENT]: Sending invalidation: {:?}",
                                                invalidation
                                            ));
                                            let _ = invalidation_tx.send(invalidation);
                                        }
                                    }
                                }
                                "RemoteIndexUpdated" => {
                                    // Remote device's index changed - invalidate entire folder
                                    // This triggers sequence check which will refresh Browse if needed
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                    {
                                        let timestamp = parse_event_time(&event.time);
                                        let invalidation = CacheInvalidation::Directory {
                                            folder_id: folder_id.to_string(),
                                            dir_path: String::new(), // Empty = entire folder
                                            timestamp,
                                        };

                                        log_debug(&format!(
                                            "DEBUG [EVENT]: RemoteIndexUpdated - invalidating entire folder: {}",
                                            folder_id
                                        ));
                                        let _ = invalidation_tx.send(invalidation);
                                    }
                                }
                                _ => {
                                    // Ignore other event types (but log them for debugging)
                                    log_debug(&format!(
                                        "DEBUG [EVENT]: Ignoring event type: {}",
                                        event.event_type
                                    ));
                                }
                            }

                            last_event_id = event.id;
                        }

                        // Persist event ID every batch (not every single event for performance)
                        if !events.is_empty() {
                            log_debug(&format!(
                                "DEBUG [EVENT LISTENER]: Persisting last_event_id={}",
                                last_event_id
                            ));
                            let _ = event_id_tx.send(last_event_id);
                        }
                    }
                    Err(_) => {
                        log_debug("DEBUG [EVENT LISTENER]: Failed to parse events JSON");
                    }
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
