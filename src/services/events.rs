use crate::utils;
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
        .open(utils::get_debug_log_path())
    {
        let _ = writeln!(file, "{}", msg);
    }
}

/// Parse RFC3339 timestamp from Syncthing event into SystemTime
/// Falls back to current time if parsing fails
fn parse_event_time(time_str: &str) -> SystemTime {
    parse_event_time_public(time_str)
}

/// RFC3339 timestamp from Syncthing, or `None` if it doesn't parse
pub fn try_parse_event_time(time_str: &str) -> Option<SystemTime> {
    DateTime::parse_from_rfc3339(time_str)
        .ok()
        .map(SystemTime::from)
}

/// Public version of parse_event_time for use in other modules
pub fn parse_event_time_public(time_str: &str) -> SystemTime {
    try_parse_event_time(time_str).unwrap_or_else(|| {
        log_debug(&format!(
            "DEBUG [EVENT]: Failed to parse timestamp '{}', using current time",
            time_str
        ));
        SystemTime::now()
    })
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

/// Whether an ItemFinished event's data reports a failure. A missing, null or empty
/// `error` means the item was synced.
pub(crate) fn item_finished_failed(data: &serde_json::Value) -> bool {
    data.get("error")
        .and_then(|e| {
            if e.is_string() {
                e.as_str().map(|m| !m.is_empty())
            } else {
                Some(!e.is_null())
            }
        })
        .unwrap_or(false)
}

/// The invalidation a config event sends, if any
pub(crate) fn config_invalidation(event_type: &str) -> Option<CacheInvalidation> {
    (event_type == "ConfigSaved").then_some(CacheInvalidation::FolderConfigChanged)
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
        /// The event carried an error: the item is still needed
        failed: bool,
        /// The finished action removed the item locally
        deleted: bool,
        #[allow(dead_code)]
        timestamp: std::time::SystemTime,
    },
    /// Syncthing saved its config (folders may be paused, resumed, added or edited)
    FolderConfigChanged,
    /// Activity event for status bar display
    Activity {
        folder_id: String,
        event_message: String,
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
                                log_debug(&format!(
                                    "DEBUG [EVENT]: WARNING - Missed events! Last ID: {}, Current ID: {}",
                                    last_event_id, event.id
                                ));
                            }

                            // Process events we care about
                            match event.event_type.as_str() {
                                "LocalIndexUpdated" => {
                                    // LocalIndexUpdated has a "filenames" array instead of "item"
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                        && let Some(filenames) =
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
                                "ItemStarted" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                        && let Some(item_path) =
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
                                "ItemFinished" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                        && let Some(item_path) =
                                            event.data.get("item").and_then(|v| v.as_str())
                                    {
                                        let timestamp = parse_event_time(&event.time);

                                        // Send ItemFinished notification
                                        let finished_invalidation =
                                            CacheInvalidation::ItemFinished {
                                                folder_id: folder_id.to_string(),
                                                file_path: item_path.to_string(),
                                                failed: item_finished_failed(&event.data),
                                                deleted: event
                                                    .data
                                                    .get("action")
                                                    .and_then(|v| v.as_str())
                                                    == Some("delete"),
                                                timestamp,
                                            };
                                        log_debug(&format!(
                                            "DEBUG [EVENT]: ItemFinished: {:?}",
                                            finished_invalidation
                                        ));
                                        let _ = invalidation_tx.send(finished_invalidation);

                                        // Send Activity event for status bar
                                        let action = event
                                            .data
                                            .get("action")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("synced");
                                        let item_type =
                                            event.data.get("type").and_then(|v| v.as_str());
                                        let item_type_str = if item_type == Some("dir")
                                            || item_path.ends_with('/')
                                        {
                                            "folder"
                                        } else {
                                            "file"
                                        };

                                        // Extract just the filename/dirname for display
                                        let display_name = item_path
                                            .trim_end_matches('/')
                                            .split('/')
                                            .next_back()
                                            .unwrap_or(item_path);

                                        let event_message = format!(
                                            "{} {} '{}'",
                                            action.to_uppercase(),
                                            item_type_str,
                                            display_name
                                        );

                                        let activity_event = CacheInvalidation::Activity {
                                            folder_id: folder_id.to_string(),
                                            event_message,
                                            timestamp,
                                        };
                                        log_debug(&format!(
                                            "DEBUG [EVENT]: Activity: {:?}",
                                            activity_event
                                        ));
                                        let _ = invalidation_tx.send(activity_event);

                                        // Also send cache invalidation
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
                                "LocalChangeDetected" | "RemoteChangeDetected" => {
                                    if let Some(folder_id) =
                                        event.data.get("folder").and_then(|v| v.as_str())
                                        && let Some(item_path) =
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
                                // The payload may be an old config replayed after a restart,
                                // so the app refetches the config instead of using it
                                event_type => match config_invalidation(event_type) {
                                    Some(invalidation) => {
                                        let _ = invalidation_tx.send(invalidation);
                                    }
                                    None => log_debug(&format!(
                                        "DEBUG [EVENT]: Ignoring event type: {}",
                                        event_type
                                    )),
                                },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_event_time_valid_rfc3339() {
        // Test parsing a valid RFC3339 timestamp from Syncthing
        // Use a dynamically generated timestamp that's always 1 hour in the past
        use chrono::Utc;
        let one_hour_ago = Utc::now() - chrono::Duration::hours(1);
        let timestamp_str = one_hour_ago.to_rfc3339();

        let parsed = parse_event_time(&timestamp_str);

        // Verify it's not the fallback (current time) by checking it's in the past
        let now = SystemTime::now();
        assert!(
            parsed < now,
            "Parsed timestamp should be in the past, not current time"
        );

        // Verify the time is reasonable (should be about 1 hour ago, within 2 hours tolerance)
        let duration_since = now.duration_since(parsed).unwrap();
        assert!(
            duration_since.as_secs() > 3000, // At least 50 minutes
            "Timestamp should be at least 50 minutes old"
        );
        assert!(
            duration_since.as_secs() < 7200, // Less than 2 hours
            "Timestamp should be less than 2 hours old"
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
    fn test_try_parse_event_time_valid() {
        let parsed = try_parse_event_time("2025-11-09T23:24:15Z").expect("valid RFC3339");
        assert_eq!(
            parsed
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1762730655
        );
    }

    #[test]
    fn test_try_parse_event_time_invalid_is_none() {
        assert_eq!(try_parse_event_time("not-a-timestamp"), None);
        assert_eq!(try_parse_event_time(""), None);
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

    // ========================================
    // ITEM FINISHED ERRORS
    // ========================================

    #[test]
    fn test_item_finished_without_error_succeeded() {
        assert!(!item_finished_failed(&serde_json::json!({"item": "a"})));
    }

    #[test]
    fn test_item_finished_null_error_succeeded() {
        assert!(!item_finished_failed(
            &serde_json::json!({"item": "a", "error": null})
        ));
    }

    #[test]
    fn test_item_finished_empty_error_succeeded() {
        assert!(!item_finished_failed(
            &serde_json::json!({"item": "a", "error": ""})
        ));
    }

    #[test]
    fn test_item_finished_error_message_failed() {
        assert!(item_finished_failed(
            &serde_json::json!({"item": "a", "error": "permission denied"})
        ));
    }

    // ========================================
    // CONFIG CHANGES
    // ========================================

    #[test]
    fn test_config_saved_event_reports_folder_config_change() {
        assert!(matches!(
            config_invalidation("ConfigSaved"),
            Some(CacheInvalidation::FolderConfigChanged)
        ));
    }

    #[test]
    fn test_other_events_report_no_config_change() {
        assert!(config_invalidation("FolderSummary").is_none());
        assert!(config_invalidation("ItemFinished").is_none());
    }
}
