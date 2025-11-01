//! UI state transition logic
//!
//! Pure functions for UI state cycling and transitions.

use crate::{model::VimCommandState, DisplayMode, SortMode};

/// Cycle to the next display mode: Off → TimestampOnly → TimestampAndSize → Off
///
/// Display modes control what information is shown for files and directories:
/// - Off: No additional info
/// - TimestampOnly: Show modification time
/// - TimestampAndSize: Show size and modification time
///
/// # Arguments
/// * `current` - The current display mode
///
/// # Returns
/// The next display mode in the cycle
///
/// # Examples
/// ```
/// use synctui::DisplayMode;
/// use synctui::logic::ui::cycle_display_mode;
///
/// assert_eq!(cycle_display_mode(DisplayMode::Off), DisplayMode::TimestampOnly);
/// assert_eq!(cycle_display_mode(DisplayMode::TimestampOnly), DisplayMode::TimestampAndSize);
/// assert_eq!(cycle_display_mode(DisplayMode::TimestampAndSize), DisplayMode::Off);
/// ```
pub fn cycle_display_mode(current: DisplayMode) -> DisplayMode {
    match current {
        DisplayMode::Off => DisplayMode::TimestampOnly,
        DisplayMode::TimestampOnly => DisplayMode::TimestampAndSize,
        DisplayMode::TimestampAndSize => DisplayMode::Off,
    }
}

/// Cycle to the next sort mode in sequence
///
/// Sort modes only apply to breadcrumb views (focus_level > 0), not the folder list.
/// Returns None if sorting is not available for the current view.
///
/// Sort cycle: VisualIndicator → Alphabetical → LastModified → FileSize → VisualIndicator
///
/// # Arguments
/// * `current` - The current sort mode
/// * `focus_level` - Current navigation focus level (0 = folder list, >0 = breadcrumb)
///
/// # Returns
/// `Some(next_mode)` if sorting is available, `None` if in folder list view
///
/// # Examples
/// ```
/// use synctui::SortMode;
/// use synctui::logic::ui::cycle_sort_mode;
///
/// // Cycling in breadcrumb view
/// assert_eq!(cycle_sort_mode(SortMode::VisualIndicator, 1), Some(SortMode::Alphabetical));
/// assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 1), Some(SortMode::LastModified));
/// assert_eq!(cycle_sort_mode(SortMode::LastModified, 1), Some(SortMode::FileSize));
/// assert_eq!(cycle_sort_mode(SortMode::FileSize, 1), Some(SortMode::VisualIndicator));
///
/// // No sorting in folder list
/// assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 0), None);
/// ```
pub fn cycle_sort_mode(current: SortMode, focus_level: usize) -> Option<SortMode> {
    // No sorting for folder list
    if focus_level == 0 {
        return None;
    }

    Some(match current {
        SortMode::VisualIndicator => SortMode::Alphabetical,
        SortMode::Alphabetical => SortMode::LastModified,
        SortMode::LastModified => SortMode::FileSize,
        SortMode::FileSize => SortMode::VisualIndicator,
    })
}

/// Toggle the sort reverse flag
///
/// Sort reversal only applies to breadcrumb views (focus_level > 0), not the folder list.
/// Returns None if sorting is not available for the current view.
///
/// # Arguments
/// * `current` - Current sort reverse state
/// * `focus_level` - Current navigation focus level (0 = folder list, >0 = breadcrumb)
///
/// # Returns
/// `Some(toggled_value)` if sorting is available, `None` if in folder list view
///
/// # Examples
/// ```
/// use synctui::logic::ui::toggle_sort_reverse;
///
/// // Toggling in breadcrumb view
/// assert_eq!(toggle_sort_reverse(false, 1), Some(true));
/// assert_eq!(toggle_sort_reverse(true, 1), Some(false));
///
/// // No sorting in folder list
/// assert_eq!(toggle_sort_reverse(false, 0), None);
/// assert_eq!(toggle_sort_reverse(true, 0), None);
/// ```
pub fn toggle_sort_reverse(current: bool, focus_level: usize) -> Option<bool> {
    // No sorting for folder list
    if focus_level == 0 {
        return None;
    }

    Some(!current)
}

/// Calculate next vim command state when 'g' key is pressed
///
/// Vim mode uses a two-key sequence 'gg' to jump to the first item.
/// This function handles the state machine for that sequence:
/// - First 'g' press: transition to WaitingForSecondG
/// - Second 'g' press (while waiting): return to None and signal completion
///
/// # Arguments
/// * `current` - Current vim command state
/// * `g_key_pressed` - Whether the 'g' key was just pressed
///
/// # Returns
/// * `(new_state, should_jump_to_first)` - New state and whether to execute jump
///
/// # Examples
/// ```
/// use synctui::model::VimCommandState;
/// use synctui::logic::ui::next_vim_command_state;
///
/// // First 'g' press - start sequence
/// let (state, jump) = next_vim_command_state(VimCommandState::None, true);
/// assert_eq!(state, VimCommandState::WaitingForSecondG);
/// assert_eq!(jump, false);
///
/// // Second 'g' press - complete sequence
/// let (state, jump) = next_vim_command_state(VimCommandState::WaitingForSecondG, true);
/// assert_eq!(state, VimCommandState::None);
/// assert_eq!(jump, true);
///
/// // Other key press - reset
/// let (state, jump) = next_vim_command_state(VimCommandState::WaitingForSecondG, false);
/// assert_eq!(state, VimCommandState::None);
/// assert_eq!(jump, false);
/// ```
pub fn next_vim_command_state(
    current: VimCommandState,
    g_key_pressed: bool,
) -> (VimCommandState, bool) {
    if !g_key_pressed {
        // Any non-g key resets the state
        return (VimCommandState::None, false);
    }

    match current {
        VimCommandState::None => {
            // First 'g' press - start waiting for second
            (VimCommandState::WaitingForSecondG, false)
        }
        VimCommandState::WaitingForSecondG => {
            // Second 'g' press - complete the 'gg' sequence
            (VimCommandState::None, true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_display_mode_from_off() {
        assert_eq!(cycle_display_mode(DisplayMode::Off), DisplayMode::TimestampOnly);
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_only() {
        assert_eq!(cycle_display_mode(DisplayMode::TimestampOnly), DisplayMode::TimestampAndSize);
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_and_size() {
        assert_eq!(cycle_display_mode(DisplayMode::TimestampAndSize), DisplayMode::Off);
    }

    #[test]
    fn test_cycle_sort_mode_in_breadcrumb_view() {
        // Normal cycling in breadcrumb view (focus_level > 0)
        assert_eq!(cycle_sort_mode(SortMode::VisualIndicator, 1), Some(SortMode::Alphabetical));
        assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 1), Some(SortMode::LastModified));
        assert_eq!(cycle_sort_mode(SortMode::LastModified, 1), Some(SortMode::FileSize));
        assert_eq!(cycle_sort_mode(SortMode::FileSize, 1), Some(SortMode::VisualIndicator));

        // Works at any breadcrumb level
        assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 2), Some(SortMode::LastModified));
        assert_eq!(cycle_sort_mode(SortMode::FileSize, 5), Some(SortMode::VisualIndicator));
    }

    #[test]
    fn test_cycle_sort_mode_in_folder_list() {
        // No cycling in folder list (focus_level == 0)
        assert_eq!(cycle_sort_mode(SortMode::VisualIndicator, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::LastModified, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::FileSize, 0), None);
    }

    #[test]
    fn test_toggle_sort_reverse_in_breadcrumb_view() {
        // Toggle in breadcrumb view (focus_level > 0)
        assert_eq!(toggle_sort_reverse(false, 1), Some(true));
        assert_eq!(toggle_sort_reverse(true, 1), Some(false));
        assert_eq!(toggle_sort_reverse(false, 2), Some(true));
        assert_eq!(toggle_sort_reverse(true, 5), Some(false));
    }

    #[test]
    fn test_toggle_sort_reverse_in_folder_list() {
        // No toggling in folder list (focus_level == 0)
        assert_eq!(toggle_sort_reverse(false, 0), None);
        assert_eq!(toggle_sort_reverse(true, 0), None);
    }

    #[test]
    fn test_next_vim_command_state_first_g_press() {
        // First 'g' press - transition to waiting state
        let (state, should_jump) = next_vim_command_state(VimCommandState::None, true);
        assert_eq!(state, VimCommandState::WaitingForSecondG);
        assert_eq!(should_jump, false);
    }

    #[test]
    fn test_next_vim_command_state_second_g_press() {
        // Second 'g' press - complete sequence and signal jump
        let (state, should_jump) = next_vim_command_state(VimCommandState::WaitingForSecondG, true);
        assert_eq!(state, VimCommandState::None);
        assert_eq!(should_jump, true);
    }

    #[test]
    fn test_next_vim_command_state_reset_on_other_key() {
        // Any non-g key resets state from waiting
        let (state, should_jump) = next_vim_command_state(VimCommandState::WaitingForSecondG, false);
        assert_eq!(state, VimCommandState::None);
        assert_eq!(should_jump, false);

        // Non-g key in None state stays None
        let (state, should_jump) = next_vim_command_state(VimCommandState::None, false);
        assert_eq!(state, VimCommandState::None);
        assert_eq!(should_jump, false);
    }

    #[test]
    fn test_next_vim_command_state_full_sequence() {
        // Simulate full 'gg' sequence
        let mut state = VimCommandState::None;
        let mut should_jump;

        // First 'g'
        (state, should_jump) = next_vim_command_state(state, true);
        assert_eq!(state, VimCommandState::WaitingForSecondG);
        assert!(!should_jump);

        // Second 'g'
        (state, should_jump) = next_vim_command_state(state, true);
        assert_eq!(state, VimCommandState::None);
        assert!(should_jump);
    }
}
