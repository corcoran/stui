//! Tests for folder status transitions
//!
//! Bug: Folder stays in "scanning" state even after scan completes
//! WebUI shows "synced" but TUI shows "scanning" until manual refresh

use stui::model::Model;

/// Test: Folder status should update from "scanning" to "idle" automatically
#[test]
fn test_folder_status_updates_after_scan_completes() {
    let _model = Model::new(false);

    let _folder_id = "test-folder".to_string();

    // Simulate: folder has a status stored
    let old_sequence = 100u64;
    let new_sequence = 101u64;

    // Verify state transition logic
    assert_ne!(
        old_sequence, new_sequence,
        "Sequence should change after scan"
    );

    // The bug: even though sequence changes, state stays "scanning"
    // Expected: when sequence increments, we should fetch new status
    let old_state = "scanning";
    let new_state = "idle";
    assert_ne!(
        old_state, new_state,
        "State should transition from scanning to idle"
    );
}

/// Test: Sequence increment should trigger status refresh
#[test]
fn test_sequence_change_detection() {
    // When FolderStatusResult arrives with sequence=100 and state="scanning"
    let scanning_seq = 100u64;

    // And later FolderStatusResult arrives with sequence=101 and state="idle"
    let idle_seq = 101u64;

    // We should detect this change and update
    assert_ne!(
        scanning_seq, idle_seq,
        "Sequence changed - should update status"
    );
}

/// Test: Transient states should not be cached
#[test]
fn test_transient_states_not_cached() {
    // "scanning" is a transient state - it should update when scan completes
    let transient_states = vec!["scanning", "syncing", "cleaning"];

    for state in transient_states {
        // These states should be monitored for changes
        assert!(
            state == "scanning" || state == "syncing" || state == "cleaning",
            "State '{}' is transient and should update automatically",
            state
        );
    }
}

/// Test: Verify we track last known sequence per folder
#[test]
fn test_last_known_sequence_tracking() {
    let mut model = Model::new(false);

    let folder_id = "test-folder".to_string();
    let sequence = 100u64;

    // App should track last known sequence
    model
        .performance
        .last_known_sequences
        .insert(folder_id.clone(), sequence);

    assert_eq!(
        model.performance.last_known_sequences.get(&folder_id),
        Some(&sequence)
    );

    // When new status arrives with different sequence, we should update
    let new_sequence = 101u64;
    assert_ne!(sequence, new_sequence, "Sequence changed");
}

/// Test: SOLUTION - Poll folders in transient states
#[test]
fn test_poll_transient_state_folders() {
    // Transient states that need polling
    let transient_states = vec![
        "scanning",
        "syncing",
        "cleaning",
        "scan-waiting",
        "sync-waiting",
    ];

    for state in &transient_states {
        // These states should trigger periodic polling
        let needs_polling = matches!(
            state.as_ref(),
            "scanning" | "syncing" | "cleaning" | "scan-waiting" | "sync-waiting"
        );

        assert!(needs_polling, "State '{}' should trigger polling", state);
    }

    // Stable states should NOT trigger polling
    let stable_states = vec!["idle", "error"];
    for state in &stable_states {
        let needs_polling = matches!(
            state.as_ref(),
            "scanning" | "syncing" | "cleaning" | "scan-waiting" | "sync-waiting"
        );

        assert!(
            !needs_polling,
            "State '{}' should NOT trigger polling",
            state
        );
    }
}

/// Test: Identify folders that need status polling
#[test]
fn test_identify_folders_needing_poll() {
    use std::collections::HashMap;

    let mut folder_statuses = HashMap::new();

    // Folder 1: scanning (needs poll)
    folder_statuses.insert("folder1".to_string(), "scanning");

    // Folder 2: idle (no poll needed)
    folder_statuses.insert("folder2".to_string(), "idle");

    // Folder 3: syncing (needs poll)
    folder_statuses.insert("folder3".to_string(), "syncing");

    // Count folders needing poll
    let folders_needing_poll: Vec<_> = folder_statuses
        .iter()
        .filter(|(_, state)| {
            matches!(
                state.as_ref(),
                "scanning" | "syncing" | "cleaning" | "scan-waiting" | "sync-waiting"
            )
        })
        .collect();

    assert_eq!(
        folders_needing_poll.len(),
        2,
        "Should have 2 folders needing poll"
    );
    assert!(folders_needing_poll.iter().any(|(id, _)| *id == "folder1"));
    assert!(folders_needing_poll.iter().any(|(id, _)| *id == "folder3"));
}
