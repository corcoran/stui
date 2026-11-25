//! UI state transition logic
//!
//! Pure functions for UI state cycling and transitions.

use crate::{DisplayMode, SortMode, model::VimCommandState};

/// True when any modal overlay (dialog, menu, popup or history modal) is open.
///
/// Toasts and the search bar are not modal. The render path dims the background
/// behind overlays when this is true, so it must cover every overlay `ui::render` draws.
pub fn modal_open(model: &crate::model::Model) -> bool {
    let ui = &model.ui;
    ui.confirm_action.is_some()
        || ui.show_setup_help
        || ui.pattern_selection.is_some()
        || ui.folder_type_selection.is_some()
        || ui.folder_group_menu.is_some()
        || ui.group_name_input.is_some()
        || ui.file_info_popup.is_some()
        || ui.folder_history_modal.is_some()
        || ui.system_log.is_some()
}

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
/// use stui::DisplayMode;
/// use stui::logic::ui::cycle_display_mode;
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
/// use stui::SortMode;
/// use stui::logic::ui::cycle_sort_mode;
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
/// use stui::logic::ui::toggle_sort_reverse;
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
/// use stui::model::VimCommandState;
/// use stui::logic::ui::next_vim_command_state;
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

/// Check if a toast notification should be dismissed based on elapsed time
///
/// Toasts are automatically dismissed after 1.5 seconds to avoid cluttering the UI.
/// This timeout balances between giving users time to read messages and maintaining
/// a clean interface.
///
/// # Arguments
/// * `elapsed_millis` - Milliseconds since the toast was created
///
/// # Returns
/// `true` if the toast should be dismissed, `false` otherwise
///
/// # Examples
/// ```
/// use stui::logic::ui::should_dismiss_toast;
///
/// assert_eq!(should_dismiss_toast(500), false);   // 0.5 seconds - still showing
/// assert_eq!(should_dismiss_toast(1000), true);   // 1 second - dismiss
/// assert_eq!(should_dismiss_toast(1500), true);   // 1.5 seconds - dismiss
/// ```
pub fn should_dismiss_toast(elapsed_millis: u128) -> bool {
    elapsed_millis >= 1000
}

/// Terminal title for the current view: `stui` in the folder list, then the folder label
/// and path (`stui · Projects/website`), ending in the file name while previewing.
pub fn terminal_title(model: &crate::model::Model) -> String {
    let nav = &model.navigation;
    let Some(level) = nav
        .focus_level
        .checked_sub(1)
        .and_then(|i| nav.breadcrumb_trail.get(i))
    else {
        return "stui".to_string();
    };
    let path = match &model.ui.file_info_popup {
        Some(popup) => popup.file_path.clone(),
        None => level
            .prefix
            .as_deref()
            .unwrap_or("")
            .trim_end_matches('/')
            .to_string(),
    };
    if path.is_empty() {
        format!("stui · {}", level.folder_label)
    } else {
        format!("stui · {}/{}", level.folder_label, path)
    }
}

/// The title to write, if any: only when titles are enabled and it changed since `last`
pub fn title_to_send(enabled: bool, last: Option<&str>, current: &str) -> Option<String> {
    (enabled && last != Some(current)).then(|| current.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // CYCLE DISPLAY MODE
    // ========================================

    #[test]
    fn test_cycle_display_mode_from_off() {
        assert_eq!(
            cycle_display_mode(DisplayMode::Off),
            DisplayMode::TimestampOnly
        );
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_only() {
        assert_eq!(
            cycle_display_mode(DisplayMode::TimestampOnly),
            DisplayMode::TimestampAndSize
        );
    }

    #[test]
    fn test_cycle_display_mode_from_timestamp_and_size() {
        assert_eq!(
            cycle_display_mode(DisplayMode::TimestampAndSize),
            DisplayMode::Off
        );
    }

    // ========================================
    // CYCLE SORT MODE
    // ========================================

    #[test]
    fn test_cycle_sort_mode_in_breadcrumb_view() {
        // Normal cycling in breadcrumb view (focus_level > 0)
        assert_eq!(
            cycle_sort_mode(SortMode::VisualIndicator, 1),
            Some(SortMode::Alphabetical)
        );
        assert_eq!(
            cycle_sort_mode(SortMode::Alphabetical, 1),
            Some(SortMode::LastModified)
        );
        assert_eq!(
            cycle_sort_mode(SortMode::LastModified, 1),
            Some(SortMode::FileSize)
        );
        assert_eq!(
            cycle_sort_mode(SortMode::FileSize, 1),
            Some(SortMode::VisualIndicator)
        );

        // Works at any breadcrumb level
        assert_eq!(
            cycle_sort_mode(SortMode::Alphabetical, 2),
            Some(SortMode::LastModified)
        );
        assert_eq!(
            cycle_sort_mode(SortMode::FileSize, 5),
            Some(SortMode::VisualIndicator)
        );
    }

    #[test]
    fn test_cycle_sort_mode_in_folder_list() {
        // No cycling in folder list (focus_level == 0)
        assert_eq!(cycle_sort_mode(SortMode::VisualIndicator, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::Alphabetical, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::LastModified, 0), None);
        assert_eq!(cycle_sort_mode(SortMode::FileSize, 0), None);
    }

    // ========================================
    // TOGGLE SORT REVERSE
    // ========================================

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

    // ========================================
    // VIM COMMAND STATE
    // ========================================

    #[test]
    fn test_next_vim_command_state_first_g_press() {
        // First 'g' press - transition to waiting state
        let (state, should_jump) = next_vim_command_state(VimCommandState::None, true);
        assert_eq!(state, VimCommandState::WaitingForSecondG);
        assert!(!should_jump);
    }

    #[test]
    fn test_next_vim_command_state_second_g_press() {
        // Second 'g' press - complete sequence and signal jump
        let (state, should_jump) = next_vim_command_state(VimCommandState::WaitingForSecondG, true);
        assert_eq!(state, VimCommandState::None);
        assert!(should_jump);
    }

    #[test]
    fn test_next_vim_command_state_reset_on_other_key() {
        // Any non-g key resets state from waiting
        let (state, should_jump) =
            next_vim_command_state(VimCommandState::WaitingForSecondG, false);
        assert_eq!(state, VimCommandState::None);
        assert!(!should_jump);

        // Non-g key in None state stays None
        let (state, should_jump) = next_vim_command_state(VimCommandState::None, false);
        assert_eq!(state, VimCommandState::None);
        assert!(!should_jump);
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

    // ========================================
    // TOAST DISMISSAL
    // ========================================

    #[test]
    fn test_should_dismiss_toast_before_timeout() {
        // Toast shown for less than 1 second - should NOT dismiss
        assert!(!should_dismiss_toast(0));
        assert!(!should_dismiss_toast(500));
        assert!(!should_dismiss_toast(999));
    }

    #[test]
    fn test_should_dismiss_toast_at_timeout() {
        // Exactly at timeout threshold - should dismiss
        assert!(should_dismiss_toast(1000));
    }

    #[test]
    fn test_should_dismiss_toast_after_timeout() {
        // Toast shown for more than 1 second - should dismiss
        assert!(should_dismiss_toast(1001));
        assert!(should_dismiss_toast(1500));
        assert!(should_dismiss_toast(2000));
        assert!(should_dismiss_toast(5000));
    }

    // ========================================
    // MODAL OPEN PREDICATE
    // ========================================

    use crate::model::{
        ConfirmAction, FileInfoPopupState, FolderHistoryModal, FolderTypeSelectionState, Model,
        PatternSelectionState,
    };

    #[test]
    fn test_modal_open_false_with_no_popups() {
        assert!(!modal_open(&Model::new(false)));
    }

    #[test]
    fn test_modal_open_with_group_dialogs() {
        use crate::model::types::{FolderGroupMenuState, GroupNameInputState};
        let menu = FolderGroupMenuState {
            folder_id: "a".to_string(),
            folder_label: "A".to_string(),
            current_group: String::new(),
            options: vec![],
            selected_index: 0,
        };
        let mut model = Model::new(false);
        model.ui.folder_group_menu = Some(menu.clone());
        assert!(modal_open(&model));
        model.ui.folder_group_menu = None;
        model.ui.group_name_input = Some(GroupNameInputState {
            menu,
            text: String::new(),
            show_empty_hint: false,
        });
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_false_with_only_toast() {
        let mut model = Model::new(false);
        model.ui.show_info("hello");
        assert!(!modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_each_confirm_action() {
        let actions = vec![
            ConfirmAction::Revert {
                folder_id: "f".into(),
                changed_files: vec![],
            },
            ConfirmAction::Delete {
                path: "/p".into(),
                name: "n".into(),
                is_dir: false,
            },
            ConfirmAction::IgnoreDelete {
                path: "/p".into(),
                name: "n".into(),
                is_dir: true,
            },
            ConfirmAction::PauseResume {
                folder_id: "f".into(),
                label: "l".into(),
                is_paused: false,
            },
            ConfirmAction::Rescan {
                folder_id: "f".into(),
                folder_label: "l".into(),
            },
        ];
        for action in actions {
            let mut model = Model::new(false);
            model.ui.confirm_action = Some(action);
            assert!(modal_open(&model));
        }
    }

    #[test]
    fn test_modal_open_true_for_setup_help() {
        let mut model = Model::new(false);
        model.ui.show_setup_help = true;
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_pattern_selection() {
        let mut model = Model::new(false);
        model.ui.pattern_selection = Some(PatternSelectionState {
            folder_id: "f".into(),
            item_name: "n".into(),
            patterns: vec![],
            selected_index: None,
        });
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_folder_type_selection() {
        let mut model = Model::new(false);
        model.ui.folder_type_selection = Some(FolderTypeSelectionState {
            folder_id: "f".into(),
            folder_label: "l".into(),
            current_type: "sendreceive".into(),
            selected_index: 1,
        });
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_file_info_popup() {
        let mut model = Model::new(false);
        model.ui.file_info_popup = Some(FileInfoPopupState {
            folder_id: "f".into(),
            file_path: "a.txt".into(),
            browse_item: crate::api::BrowseItem {
                name: "a.txt".into(),
                item_type: "FILE_INFO_TYPE_FILE".into(),
                mod_time: String::new(),
                size: 0,
            },
            file_details: None,
            file_content: Ok(String::new()),
            exists_on_disk: true,
            is_binary: false,
            is_image: false,
            art: None,
            archive: None,
            gzip: None,
            rendered_rows: None,
            scroll_offset: 0,
            raw_view: false,
        });
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_folder_history_modal() {
        let mut model = Model::new(false);
        model.ui.folder_history_modal = Some(FolderHistoryModal {
            folder_id: "f".into(),
            folder_label: "l".into(),
            entries: vec![],
            selected_index: 0,
            total_files_scanned: 0,
            loading: false,
            has_more: false,
            current_offset: 0,
            all_files_sorted: None,
        });
        assert!(modal_open(&model));
    }

    #[test]
    fn test_modal_open_true_for_system_log() {
        let mut model = Model::new(false);
        model.ui.system_log = Some(crate::model::types::SystemLogState {
            generation: 1,
            lines: vec![],
            filter: Default::default(),
            selected: None,
            detail: None,
            error: None,
            in_flight: false,
            loaded: false,
            last_request: std::time::Instant::now(),
        });
        assert!(modal_open(&model));
    }

    // ========================================
    // TERMINAL TITLE
    // ========================================

    fn level(label: &str, prefix: Option<&str>) -> crate::model::BreadcrumbLevel {
        crate::model::BreadcrumbLevel {
            folder_id: label.to_lowercase(),
            folder_label: label.to_string(),
            folder_path: "/data".to_string(),
            prefix: prefix.map(str::to_string),
            items: vec![],
            filtered_items: None,
            selected_index: None,
            file_sync_states: Default::default(),
            direct_states: Default::default(),
            ignored_exists: Default::default(),
            translated_base_path: "/data".to_string(),
        }
    }

    fn model_in(levels: Vec<crate::model::BreadcrumbLevel>) -> Model {
        let mut model = Model::new(false);
        model.navigation.focus_level = levels.len();
        model.navigation.breadcrumb_trail = levels;
        model
    }

    fn preview(model: &mut Model, file_path: &str) {
        model.ui.file_info_popup = Some(crate::model::FileInfoPopupState {
            folder_id: "projects".to_string(),
            file_path: file_path.to_string(),
            browse_item: crate::api::BrowseItem {
                name: file_path.rsplit('/').next().unwrap().to_string(),
                item_type: "FILE_INFO_TYPE_FILE".to_string(),
                mod_time: String::new(),
                size: 0,
            },
            file_details: None,
            file_content: Ok(String::new()),
            exists_on_disk: true,
            is_binary: false,
            is_image: false,
            art: None,
            archive: None,
            gzip: None,
            rendered_rows: None,
            scroll_offset: 0,
            raw_view: false,
        });
    }

    #[test]
    fn test_terminal_title_folder_list() {
        assert_eq!(terminal_title(&Model::new(false)), "stui");
    }

    #[test]
    fn test_terminal_title_folder_root_and_nested_path() {
        let root = model_in(vec![level("Projects", None)]);
        assert_eq!(terminal_title(&root), "stui · Projects");

        let nested = model_in(vec![
            level("Projects", None),
            level("Projects", Some("website/")),
        ]);
        assert_eq!(terminal_title(&nested), "stui · Projects/website");
    }

    #[test]
    fn test_terminal_title_preview_names_the_file() {
        let mut model = model_in(vec![
            level("Projects", None),
            level("Projects", Some("website/")),
        ]);
        preview(&mut model, "website/deploy.md");
        assert_eq!(terminal_title(&model), "stui · Projects/website/deploy.md");
    }

    #[test]
    fn test_terminal_title_keeps_spaces_and_non_ascii() {
        let model = model_in(vec![
            level("Фото и видео", None),
            level("Фото и видео", Some("2024 trip/")),
        ]);
        assert_eq!(terminal_title(&model), "stui · Фото и видео/2024 trip");
    }

    #[test]
    fn test_title_to_send() {
        assert_eq!(title_to_send(true, None, "stui"), Some("stui".to_string()));
        assert_eq!(title_to_send(true, Some("stui"), "stui"), None);
        assert_eq!(
            title_to_send(true, Some("stui"), "stui · Projects"),
            Some("stui · Projects".to_string())
        );
        assert_eq!(title_to_send(false, None, "stui"), None);
    }
}
