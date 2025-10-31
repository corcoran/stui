//! Sync State Logic
//!
//! This module contains logic for sync state management and transitions.

use crate::api::SyncState;

/// Get the priority of a sync state for sorting
///
/// Lower number = higher priority (displayed first)
///
/// Priority order:
/// 1. OutOfSync (⚠️) - Most important
/// 2. Syncing (🔄) - Active operation
/// 3. RemoteOnly (☁️)
/// 4. LocalOnly (💻)
/// 5. Ignored (🚫)
/// 6. Unknown (❓)
/// 7. Synced (✅) - Least important
pub fn sync_state_priority(state: SyncState) -> u8 {
    match state {
        SyncState::OutOfSync => 0,  // ⚠️ Most important
        SyncState::Syncing => 1,    // 🔄 Active operation
        SyncState::RemoteOnly => 2, // ☁️
        SyncState::LocalOnly => 3,  // 💻
        SyncState::Ignored => 4,    // 🚫
        SyncState::Unknown => 5,    // ❓
        SyncState::Synced => 6,     // ✅ Least important
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_state_priority_order() {
        // OutOfSync has highest priority (lowest number)
        assert_eq!(sync_state_priority(SyncState::OutOfSync), 0);
        assert_eq!(sync_state_priority(SyncState::Syncing), 1);
        assert_eq!(sync_state_priority(SyncState::RemoteOnly), 2);
        assert_eq!(sync_state_priority(SyncState::LocalOnly), 3);
        assert_eq!(sync_state_priority(SyncState::Ignored), 4);
        assert_eq!(sync_state_priority(SyncState::Unknown), 5);

        // Synced has lowest priority (highest number)
        assert_eq!(sync_state_priority(SyncState::Synced), 6);
    }

    #[test]
    fn test_priority_ordering() {
        // OutOfSync should come before Synced
        assert!(sync_state_priority(SyncState::OutOfSync) < sync_state_priority(SyncState::Synced));

        // Syncing should come before RemoteOnly
        assert!(sync_state_priority(SyncState::Syncing) < sync_state_priority(SyncState::RemoteOnly));

        // Unknown should come before Synced
        assert!(sync_state_priority(SyncState::Unknown) < sync_state_priority(SyncState::Synced));
    }
}
