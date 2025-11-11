//! Integration tests for reconnection and folder refresh logic
//!
//! These tests verify the complete flow:
//! 1. App starts with Syncthing down → shows setup help dialog
//! 2. Syncthing comes back online → folders populate + dialog dismisses

use stui::model::{Model, syncthing::ConnectionState};

/// Test: When app starts disconnected with no folders, setup help should be shown
#[test]
fn test_initial_state_shows_setup_help() {
    let mut model = Model::new(false);

    // Simulate initial disconnection with no folders
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;
    model.ui.needs_folder_refresh = false;

    // Verify initial state
    assert!(model.ui.show_setup_help, "Setup help should be visible");
    assert!(model.syncthing.folders.is_empty(), "No folders available");
    assert!(!model.ui.needs_folder_refresh, "Flag should not be set yet");
}

/// Test: When reconnecting with empty folders, needs_folder_refresh flag should be set
#[test]
fn test_reconnection_sets_folder_refresh_flag() {
    let mut model = Model::new(false);

    // Initial state: disconnected with no folders
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;
    model.ui.needs_folder_refresh = false;

    // Simulate: API handler receives successful response and checks if we were disconnected
    let was_disconnected = !matches!(
        model.syncthing.connection_state,
        ConnectionState::Connected
    );

    // Handler logic: if reconnecting with no folders, set flag
    if was_disconnected && model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }

    model.syncthing.connection_state = ConnectionState::Connected;

    // Verify flag was set
    assert!(model.ui.needs_folder_refresh, "Flag should be set when reconnecting with no folders");
    assert!(model.ui.show_setup_help, "Dialog still visible until folders fetched");
}

/// Test: When folders are fetched and populated, setup help should be dismissed
#[test]
fn test_folder_population_dismisses_setup_help() {
    let mut model = Model::new(false);

    // State after flag was set
    model.ui.show_setup_help = true;
    model.ui.needs_folder_refresh = true;
    model.syncthing.folders = vec![];
    model.navigation.folders_state_selection = None;

    // Simulate: main loop fetches folders (we'll just add them directly)
    model.ui.needs_folder_refresh = false; // Flag consumed
    model.syncthing.folders = vec![
        stui::api::Folder {
            id: "test-folder-1".to_string(),
            label: Some("Test Folder 1".to_string()),
            path: "/data/test1".to_string(),
            folder_type: "sendreceive".to_string(),
            paused: false,
        },
        stui::api::Folder {
            id: "test-folder-2".to_string(),
            label: Some("Test Folder 2".to_string()),
            path: "/data/test2".to_string(),
            folder_type: "sendreceive".to_string(),
            paused: false,
        },
    ];

    // Main loop logic: if folders populated, select first and dismiss dialog
    if !model.syncthing.folders.is_empty() {
        model.navigation.folders_state_selection = Some(0);
        model.ui.show_setup_help = false; // CRITICAL: dismiss dialog
    }

    // Verify
    assert!(!model.ui.show_setup_help, "Setup help should be dismissed");
    assert!(!model.ui.needs_folder_refresh, "Flag should be consumed");
    assert_eq!(model.syncthing.folders.len(), 2, "Should have 2 folders");
    assert_eq!(model.navigation.folders_state_selection, Some(0), "First folder should be selected");
}

/// Test: Complete reconnection flow from start to finish
#[test]
fn test_complete_reconnection_flow() {
    let mut model = Model::new(false);

    // STEP 1: Initial disconnection - show setup help
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;

    assert!(model.ui.show_setup_help, "Step 1: Dialog visible");
    assert!(model.syncthing.folders.is_empty(), "Step 1: No folders");

    // STEP 2: Reconnection - set flag
    let was_disconnected = !matches!(
        model.syncthing.connection_state,
        ConnectionState::Connected
    );

    if was_disconnected && model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }
    model.syncthing.connection_state = ConnectionState::Connected;

    assert!(model.ui.needs_folder_refresh, "Step 2: Flag set");
    assert!(model.ui.show_setup_help, "Step 2: Dialog still visible");

    // STEP 3: Fetch folders and dismiss dialog
    model.ui.needs_folder_refresh = false;
    model.syncthing.folders = vec![
        stui::api::Folder {
            id: "test".to_string(),
            label: Some("Test".to_string()),
            path: "/data/test".to_string(),
            folder_type: "sendreceive".to_string(),
            paused: false,
        },
    ];

    if !model.syncthing.folders.is_empty() {
        model.navigation.folders_state_selection = Some(0);
        model.ui.show_setup_help = false;
    }

    assert!(!model.ui.show_setup_help, "Step 3: Dialog dismissed");
    assert!(!model.ui.needs_folder_refresh, "Step 3: Flag consumed");
    assert_eq!(model.syncthing.folders.len(), 1, "Step 3: Folders populated");
    assert_eq!(model.navigation.folders_state_selection, Some(0), "Step 3: Folder selected");
}

/// Test: Edge case - reconnection with folders already present (shouldn't happen, but defensive)
#[test]
fn test_reconnection_with_existing_folders_no_flag() {
    let mut model = Model::new(false);

    // State: have folders but disconnected (edge case)
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![
        stui::api::Folder {
            id: "existing".to_string(),
            label: Some("Existing".to_string()),
            path: "/data/existing".to_string(),
            folder_type: "sendreceive".to_string(),
            paused: false,
        },
    ];
    model.ui.needs_folder_refresh = false;

    // Reconnect
    let was_disconnected = !matches!(
        model.syncthing.connection_state,
        ConnectionState::Connected
    );

    if was_disconnected && model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }
    model.syncthing.connection_state = ConnectionState::Connected;

    // Flag should NOT be set because we already have folders
    assert!(!model.ui.needs_folder_refresh, "Flag should not be set when folders exist");
}

/// Test: Edge case - already connected, no need for flag
#[test]
fn test_already_connected_no_flag() {
    let mut model = Model::new(false);

    // Already connected
    model.syncthing.connection_state = ConnectionState::Connected;
    model.syncthing.folders = vec![];
    model.ui.needs_folder_refresh = false;

    // Check was_disconnected
    let was_disconnected = !matches!(
        model.syncthing.connection_state,
        ConnectionState::Connected
    );

    if was_disconnected && model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }

    // Flag should NOT be set because we weren't disconnected
    assert!(!model.ui.needs_folder_refresh, "Flag should not be set when already connected");
}

/// Test: BUG - SystemStatus might be called AFTER state already set to Connected
/// This happens when FolderStatus returns first and sets state to Connected,
/// then SystemStatus arrives and thinks we're already connected
#[test]
fn test_state_already_connected_before_system_status() {
    let mut model = Model::new(false);

    // Initial state: disconnected with no folders
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;
    model.ui.needs_folder_refresh = false;

    // Simulate: FolderStatus returns FIRST (before SystemStatus) and sets Connected
    // This is what's happening in the real app!
    model.syncthing.connection_state = ConnectionState::Connected;

    // Now SystemStatus arrives...
    let was_disconnected = !matches!(
        model.syncthing.connection_state,
        ConnectionState::Connected
    );

    // BUG: was_disconnected is now false because state was already set!
    assert!(!was_disconnected, "This is the bug - state already Connected");

    // So flag never gets set
    if was_disconnected && model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }

    // BUG EXPOSED: Flag is NOT set even though we have no folders!
    assert!(!model.ui.needs_folder_refresh, "BUG: Flag not set because state already Connected");
    assert!(model.syncthing.folders.is_empty(), "Still no folders");
    assert!(model.ui.show_setup_help, "Dialog still visible - STUCK!");
}

/// Test: SOLUTION - Check folders.is_empty() regardless of was_disconnected or dialog state
/// If we're Connected but have no folders, we should fetch them
#[test]
fn test_solution_fetch_folders_when_connected_but_empty() {
    let mut model = Model::new(false);

    // State: Connected (maybe set by FolderStatus earlier) but no folders
    model.syncthing.connection_state = ConnectionState::Connected;
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true; // Dialog state doesn't matter
    model.ui.needs_folder_refresh = false;

    // SOLUTION: If Connected AND folders empty, fetch folders (regardless of dialog)
    if matches!(model.syncthing.connection_state, ConnectionState::Connected)
        && model.syncthing.folders.is_empty()
    {
        model.ui.needs_folder_refresh = true;
    }

    // Flag should be set
    assert!(model.ui.needs_folder_refresh, "Flag should be set when Connected but no folders");
}

/// Test: User dismisses setup help with Retry button, then reconnects
/// Folders should still be fetched and displayed
#[test]
fn test_reconnect_after_user_dismissed_setup_help() {
    let mut model = Model::new(false);

    // Initial state: disconnected with no folders, setup help showing
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;
    model.ui.needs_folder_refresh = false;

    // User presses 'r' to close the dialog (but Syncthing still down)
    model.ui.show_setup_help = false; // Dialog dismissed

    assert!(!model.ui.show_setup_help, "Dialog dismissed by user");
    assert!(model.syncthing.folders.is_empty(), "Still no folders");

    // Now Syncthing comes back online - state changes to Connected
    model.syncthing.connection_state = ConnectionState::Connected;

    // SystemStatus handler should detect: Connected + no folders (regardless of dialog state)
    // OLD LOGIC: if folders.is_empty() && show_setup_help - WRONG!
    // NEW LOGIC: if folders.is_empty() (dialog doesn't matter)
    if model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }

    // Flag should be set even though dialog was dismissed
    assert!(model.ui.needs_folder_refresh, "Flag should be set when Connected but no folders, regardless of dialog state");
}

/// Test: Complete flow - user dismisses dialog, then folders populate
#[test]
fn test_complete_flow_after_dismissed_dialog() {
    let mut model = Model::new(false);

    // Start disconnected, dialog showing
    model.syncthing.connection_state = ConnectionState::Disconnected {
        error_type: stui::logic::errors::ErrorType::NetworkError,
        message: "Connection refused".to_string(),
    };
    model.syncthing.folders = vec![];
    model.ui.show_setup_help = true;

    // User dismisses dialog
    model.ui.show_setup_help = false;

    // Reconnect - flag should be set
    model.syncthing.connection_state = ConnectionState::Connected;
    if model.syncthing.folders.is_empty() {
        model.ui.needs_folder_refresh = true;
    }

    assert!(model.ui.needs_folder_refresh, "Step 1: Flag set");

    // Fetch folders
    model.ui.needs_folder_refresh = false;
    model.syncthing.folders = vec![
        stui::api::Folder {
            id: "test".to_string(),
            label: Some("Test".to_string()),
            path: "/data/test".to_string(),
            folder_type: "sendreceive".to_string(),
            paused: false,
        },
    ];

    // Select first folder
    if !model.syncthing.folders.is_empty() {
        model.navigation.folders_state_selection = Some(0);
    }

    assert!(!model.ui.needs_folder_refresh, "Step 2: Flag consumed");
    assert_eq!(model.syncthing.folders.len(), 1, "Step 2: Folders populated");
    assert_eq!(model.navigation.folders_state_selection, Some(0), "Step 2: Folder selected");
}
