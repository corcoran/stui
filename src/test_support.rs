//! Helpers shared by test modules.

use crate::App;
use crate::api::{BrowseItem, SyncState};
use crate::services::api::ApiResponse;
use crate::services::events::CacheInvalidation;
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

/// Create a minimal test App instance
/// Only initializes the fields needed for Activity event testing
pub(crate) fn create_test_app() -> App {
    create_test_app_with_api_rx().0
}

/// Like `create_test_app`, but keeps the receiving end of `api_tx` so tests can
/// inspect queued API requests
pub(crate) fn create_test_app_with_api_rx() -> (
    App,
    tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
) {
    // Nothing listens on port 9, so stray requests fail at once and never reach a
    // real Syncthing on the default port
    create_test_app_at("http://127.0.0.1:9")
}

/// Test app whose API client talks to `base_url`
pub(crate) fn create_test_app_at(
    base_url: &str,
) -> (
    App,
    tokio::sync::mpsc::UnboundedReceiver<crate::services::api::ApiRequest>,
) {
    use crate::SyncthingClient;
    use crate::cache::CacheDb;
    use crate::config::Config;
    use crate::model::Model;
    use crate::ui::icons::{IconMode, IconRenderer, IconTheme};
    use std::collections::HashMap;
    use std::time::Duration;

    let config = Config {
        api_key: "test-key".to_string(),
        base_url: base_url.to_string(),
        path_map: HashMap::new(),
        vim_mode: false,
        icon_mode: "emoji".to_string(),
        open_command: None,
        clipboard_command: None,
        image_preview_enabled: false,
        image_protocol: "auto".to_string(),
        terminal_title: false,
    };

    let client = SyncthingClient::new(config.base_url.clone(), config.api_key.clone());
    let cache = CacheDb::new_in_memory().expect("Failed to create test cache");
    let (api_tx, api_request_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_api_response_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_invalidation_tx, invalidation_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_id_tx, event_id_rx) = tokio::sync::mpsc::unbounded_channel();
    let (image_update_tx, image_update_rx) = tokio::sync::mpsc::unbounded_channel();
    let (undelete_tx, undelete_rx) = tokio::sync::mpsc::unbounded_channel();

    let app = App {
        model: Model::new(config.vim_mode),
        client,
        cache,
        api_tx,
        api_rx,
        invalidation_rx,
        event_id_rx,
        icon_renderer: IconRenderer::new(IconMode::Emoji, IconTheme::default()),
        image_picker: None,
        image_update_tx,
        image_update_rx,
        path_map: config.path_map,
        open_command: config.open_command,
        clipboard_command: config.clipboard_command,
        base_url: config.base_url,
        last_status_update: std::time::Instant::now(),
        last_system_status_update: std::time::Instant::now(),
        last_connection_stats_fetch: std::time::Instant::now(),
        last_directory_update: std::time::Instant::now(),
        dir_child_states: HashMap::new(),
        pending_preview_need: None,
        terminal_title_enabled: false,
        last_terminal_title: None,
        undelete_jobs: HashMap::new(),
        undelete_tx,
        undelete_rx,
        last_db_flush: std::time::Instant::now(),
        last_reconnect_attempt: std::time::Instant::now(),
        reconnect_delay: Duration::from_secs(1),
        pending_sync_state_writes: Vec::new(),
        image_state_map: HashMap::new(),
    };
    (app, api_request_rx)
}

pub(crate) fn item_started(folder: &str, path: &str) -> CacheInvalidation {
    CacheInvalidation::ItemStarted {
        folder_id: folder.to_string(),
        file_path: path.to_string(),
        timestamp: SystemTime::now(),
    }
}

pub(crate) fn item_finished(folder: &str, path: &str) -> CacheInvalidation {
    CacheInvalidation::ItemFinished {
        folder_id: folder.to_string(),
        file_path: path.to_string(),
        failed: false,
        deleted: false,
        timestamp: SystemTime::now(),
    }
}

pub(crate) fn item_failed(folder: &str, path: &str) -> CacheInvalidation {
    CacheInvalidation::ItemFinished {
        folder_id: folder.to_string(),
        file_path: path.to_string(),
        failed: true,
        deleted: false,
        timestamp: SystemTime::now(),
    }
}

pub(crate) fn file_item(name: &str) -> BrowseItem {
    BrowseItem {
        name: name.to_string(),
        item_type: "FILE_INFO_TYPE_FILE".to_string(),
        mod_time: String::new(),
        size: 0,
    }
}

pub(crate) fn dir_item(name: &str) -> BrowseItem {
    BrowseItem {
        name: name.to_string(),
        item_type: "FILE_INFO_TYPE_DIRECTORY".to_string(),
        mod_time: String::new(),
        size: 0,
    }
}

/// Put the app in breadcrumb view of `folder` root with every item RemoteOnly
pub(crate) fn view_folder_root(app: &mut App, folder: &str, items: Vec<BrowseItem>) {
    let file_sync_states: HashMap<String, SyncState> = items
        .iter()
        .map(|i| (i.name.clone(), SyncState::RemoteOnly))
        .collect();
    app.model.navigation.breadcrumb_trail = vec![crate::model::BreadcrumbLevel {
        folder_id: folder.to_string(),
        folder_label: folder.to_string(),
        folder_path: format!("/data/{}", folder),
        prefix: None,
        items,
        filtered_items: None,
        selected_index: Some(0),
        file_sync_states,
        direct_states: Default::default(),
        ignored_exists: HashMap::new(),
        translated_base_path: format!("/data/{}", folder),
    }];
    app.model.navigation.focus_level = 1;
}

/// Sync states as the breadcrumb panel renders them for the focused level
pub(crate) fn displayed_states(app: &App) -> HashMap<String, SyncState> {
    let level = &app.model.navigation.breadcrumb_trail[0];
    crate::logic::sync_states::overlay_syncing_states(
        &level.items,
        &level.file_sync_states,
        level.prefix.as_deref(),
        app.model.syncthing.syncing_files.get(&level.folder_id),
    )
    .into_owned()
}

pub(crate) fn folder_status(state: &str) -> crate::api::FolderStatus {
    let mut json = serde_json::json!({});
    for key in [
        "sequence",
        "globalBytes",
        "globalDeleted",
        "globalDirectories",
        "globalFiles",
        "globalSymlinks",
        "globalTotalItems",
        "inSyncBytes",
        "inSyncFiles",
        "localBytes",
        "localDeleted",
        "localDirectories",
        "localFiles",
        "localSymlinks",
        "localTotalItems",
        "needBytes",
        "needDeletes",
        "needDirectories",
        "needFiles",
        "needSymlinks",
        "needTotalItems",
        "receiveOnlyChangedBytes",
        "receiveOnlyChangedDeletes",
        "receiveOnlyChangedDirectories",
        "receiveOnlyChangedFiles",
        "receiveOnlyChangedSymlinks",
        "receiveOnlyTotalItems",
    ] {
        json[key] = serde_json::json!(0);
    }
    json["state"] = serde_json::json!(state);
    serde_json::from_value(json).expect("valid FolderStatus")
}

pub(crate) fn status_result(folder: &str, state: &str) -> ApiResponse {
    status_result_at(folder, state, std::time::Instant::now())
}

pub(crate) fn status_result_at(
    folder: &str,
    state: &str,
    requested_at: std::time::Instant,
) -> ApiResponse {
    ApiResponse::FolderStatusResult {
        folder_id: folder.to_string(),
        status: Ok(folder_status(state)),
        requested_at,
    }
}

/// Record an outstanding need fetch, as the real request paths do
pub(crate) fn mark_fetched(app: &mut App, folder: &str) -> std::time::Instant {
    let at = std::time::Instant::now();
    app.model.syncthing.need_fetches.insert(
        folder.to_string(),
        crate::model::syncthing::NeedFetch::new(at, 0),
    );
    at
}

pub(crate) fn need_page(progress: &[&str], queued: &[&str]) -> crate::api::NeedResponse {
    let infos = |names: &[&str]| {
        names
            .iter()
            .map(|n| crate::api::FileInfo {
                name: n.to_string(),
                ..Default::default()
            })
            .collect()
    };
    crate::api::NeedResponse {
        progress: infos(progress),
        queued: infos(queued),
        rest: vec![],
        page: 1,
        perpage: 1000,
    }
}

/// Response to the need fetch identified by `requested_at`, for sequence 0
pub(crate) fn need_result(
    folder: &str,
    requested_at: std::time::Instant,
    progress: &[&str],
    queued: &[&str],
) -> ApiResponse {
    need_result_seq(folder, requested_at, 0, progress, queued)
}

pub(crate) fn need_result_seq(
    folder: &str,
    requested_at: std::time::Instant,
    sequence: u64,
    progress: &[&str],
    queued: &[&str],
) -> ApiResponse {
    ApiResponse::FolderNeed {
        folder_id: folder.to_string(),
        sequence,
        requested_at,
        need: Ok(need_page(progress, queued)),
        ignore_patterns: Ok(vec![]),
    }
}

pub(crate) fn paths(names: &[&str]) -> HashSet<String> {
    names.iter().map(|n| n.to_string()).collect()
}

pub(crate) fn file_details(state: SyncState) -> crate::api::FileDetails {
    use crate::api::{FileDetails, FileInfo};
    let synced = || FileInfo {
        sequence: 5,
        version: vec!["v1".to_string()],
        ..Default::default()
    };
    match state {
        SyncState::Synced => FileDetails {
            local: Some(synced()),
            global: Some(synced()),
            availability: vec![],
        },
        SyncState::RemoteOnly => FileDetails {
            local: None,
            global: Some(synced()),
            availability: vec![],
        },
        SyncState::Ignored => FileDetails {
            local: Some(FileInfo {
                ignored: true,
                ..synced()
            }),
            global: Some(synced()),
            availability: vec![],
        },
        other => panic!("file_details: unsupported state {:?}", other),
    }
}
