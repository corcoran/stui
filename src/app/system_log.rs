//! Log viewer: opening, polling and applying replies

use std::time::{Duration, Instant};

use crate::App;
use crate::api::LogLine;
use crate::logic::{self, system_log};
use crate::model::syncthing::ConnectionState;
use crate::model::types::SystemLogState;
use crate::services::api::ApiRequest;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

impl App {
    /// Open the log viewer and request the whole log. `Err` is the reason it can't open.
    pub fn open_system_log(&mut self) -> Result<(), String> {
        let version = self.model.syncthing.version.as_deref();
        if !system_log::supports_log_viewer(version) {
            return Err(system_log::unsupported_message(version));
        }
        self.model.ui.system_log_generation += 1;
        let generation = self.model.ui.system_log_generation;
        self.model.ui.system_log = Some(SystemLogState {
            generation,
            lines: Vec::new(),
            filter: Default::default(),
            selected: None,
            offset: 0,
            detail: None,
            error: None,
            in_flight: true,
            loaded: false,
            last_request: Instant::now(),
        });
        let _ = self.api_tx.send(ApiRequest::GetSystemLog {
            generation,
            since: None,
        });
        Ok(())
    }

    pub fn close_system_log(&mut self) {
        self.model.ui.system_log = None;
    }

    /// Request new lines when the viewer is open and a poll is due
    pub fn poll_system_log(&mut self) {
        if self.model.syncthing.connection_state != ConnectionState::Connected {
            return;
        }
        let Some(state) = self.model.ui.system_log.as_mut() else {
            return;
        };
        if state.in_flight || state.last_request.elapsed() < POLL_INTERVAL {
            return;
        }
        state.in_flight = true;
        state.last_request = Instant::now();
        let _ = self.api_tx.send(ApiRequest::GetSystemLog {
            generation: state.generation,
            since: system_log::poll_since(&state.lines),
        });
    }

    /// Apply a reply to the opening it was requested for. `Err` means the reply came from
    /// Syncthing 1.x and the viewer was closed.
    pub fn apply_system_log(
        &mut self,
        generation: u64,
        lines: Result<Vec<LogLine>, anyhow::Error>,
    ) -> Result<(), String> {
        let Some(state) = self.model.ui.system_log.as_mut() else {
            return Ok(());
        };
        if state.generation != generation {
            return Ok(());
        }
        state.in_flight = false;
        match lines {
            Ok(lines) if system_log::is_unsupported_reply(&lines) => {
                self.close_system_log();
                return Err(system_log::unsupported_message(
                    self.model.syncthing.version.as_deref(),
                ));
            }
            Ok(lines) => {
                state.loaded = true;
                state.error = None;
                system_log::merge(state, lines);
            }
            Err(e) => state.error = Some(logic::errors::short_error_message(&e)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::App;
    use crate::api::{LogLine, RawLevel};
    use crate::logic::system_log::LogFilter;
    use crate::model::syncthing::ConnectionState;
    use crate::model::types::{LogDetail, SystemLogState};
    use crate::services::api::ApiRequest;
    use crate::test_support::create_test_app_with_api_rx;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::UnboundedReceiver;

    const TOO_OLD: &str = "The log viewer needs Syncthing 2.0 or newer (this device runs v1.30.0)";

    fn line(second: u32, level: &str) -> LogLine {
        LogLine {
            when: format!("2026-01-01T10:{:02}:{:02}Z", second / 60, second % 60),
            message: format!("m{second}"),
            level: RawLevel::Name(level.to_string()),
        }
    }

    fn lines(range: std::ops::Range<u32>) -> Vec<LogLine> {
        range.map(|n| line(n, "INF")).collect()
    }

    fn numeric_line() -> LogLine {
        LogLine {
            level: RawLevel::Number(2),
            ..line(0, "INF")
        }
    }

    fn setup() -> (App, UnboundedReceiver<ApiRequest>) {
        let (mut app, rx) = create_test_app_with_api_rx();
        app.model.syncthing.connection_state = ConnectionState::Connected;
        app.model.syncthing.version = Some("v2.1.7".to_string());
        (app, rx)
    }

    /// Viewer open with `lines` loaded and its request consumed
    fn opened(lines: Vec<LogLine>) -> (App, UnboundedReceiver<ApiRequest>) {
        let (mut app, mut rx) = setup();
        app.open_system_log().unwrap();
        rx.try_recv().unwrap();
        app.apply_system_log(1, Ok(lines)).unwrap();
        (app, rx)
    }

    fn log(app: &mut App) -> &mut SystemLogState {
        app.model.ui.system_log.as_mut().unwrap()
    }

    fn make_due(app: &mut App) {
        log(app).last_request = Instant::now() - Duration::from_secs(3);
    }

    /// Generation and `since` of the next queued log request
    fn sent(rx: &mut UnboundedReceiver<ApiRequest>) -> Option<(u64, Option<String>)> {
        match rx.try_recv().ok()? {
            ApiRequest::GetSystemLog { generation, since } => Some((generation, since)),
            other => panic!("unexpected request {other:?}"),
        }
    }

    // ========================================
    // OPEN
    // ========================================

    #[test]
    fn test_open_requests_whole_log() {
        let (mut app, mut rx) = setup();
        app.open_system_log().unwrap();
        assert_eq!(sent(&mut rx), Some((1, None)));
        let state = log(&mut app);
        assert_eq!(state.filter, LogFilter::All);
        assert!(state.in_flight);
        assert!(!state.loaded);
    }

    #[test]
    fn test_open_refuses_1x() {
        let (mut app, mut rx) = setup();
        app.model.syncthing.version = Some("v1.30.0".to_string());
        assert_eq!(app.open_system_log(), Err(TOO_OLD.to_string()));
        assert!(app.model.ui.system_log.is_none());
        assert_eq!(sent(&mut rx), None);
    }

    #[test]
    fn test_open_allows_unknown_version() {
        let (mut app, _rx) = setup();
        app.model.syncthing.version = None;
        assert!(app.open_system_log().is_ok());
        assert!(app.model.ui.system_log.is_some());
    }

    #[test]
    fn test_reopen_starts_unfiltered_and_empty() {
        let (mut app, mut rx) = opened(lines(0..3));
        log(&mut app).filter = LogFilter::Errors;
        app.close_system_log();
        app.open_system_log().unwrap();
        assert_eq!(sent(&mut rx), Some((2, None)));
        let state = log(&mut app);
        assert_eq!(state.generation, 2);
        assert_eq!(state.filter, LogFilter::All);
        assert!(state.lines.is_empty());
    }

    // ========================================
    // POLL
    // ========================================

    #[test]
    fn test_poll_waits_two_seconds() {
        let (mut app, mut rx) = opened(lines(0..3));
        log(&mut app).last_request = Instant::now() - Duration::from_millis(1500);
        app.poll_system_log();
        assert_eq!(sent(&mut rx), None);
        log(&mut app).last_request = Instant::now() - Duration::from_millis(2100);
        app.poll_system_log();
        assert!(sent(&mut rx).is_some());
    }

    #[test]
    fn test_poll_sends_since_just_before_newest() {
        let (mut app, mut rx) = opened(vec![line(5, "INF"), line(9, "INF"), line(7, "INF")]);
        make_due(&mut app);
        app.poll_system_log();
        assert_eq!(
            sent(&mut rx),
            Some((1, Some("2026-01-01T10:00:08.999+00:00".to_string())))
        );
        assert!(log(&mut app).in_flight);
        assert!(log(&mut app).last_request.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn test_poll_sends_none_when_empty() {
        let (mut app, mut rx) = opened(vec![]);
        make_due(&mut app);
        app.poll_system_log();
        assert_eq!(sent(&mut rx), Some((1, None)));
    }

    #[test]
    fn test_poll_waits_for_reply() {
        let (mut app, mut rx) = setup();
        app.open_system_log().unwrap();
        sent(&mut rx);
        make_due(&mut app);
        app.poll_system_log();
        assert_eq!(sent(&mut rx), None);
    }

    #[test]
    fn test_poll_stops_when_closed() {
        let (mut app, mut rx) = opened(lines(0..3));
        make_due(&mut app);
        app.close_system_log();
        app.poll_system_log();
        assert_eq!(sent(&mut rx), None);
    }

    #[test]
    fn test_poll_pauses_while_disconnected() {
        let (mut app, mut rx) = opened(lines(0..3));
        make_due(&mut app);
        app.model.syncthing.connection_state = ConnectionState::Disconnected {
            error_type: crate::logic::errors::ErrorType::ConnectionRefused,
            message: "refused".to_string(),
        };
        app.poll_system_log();
        assert_eq!(sent(&mut rx), None);
        app.model.syncthing.connection_state = ConnectionState::Connected;
        app.poll_system_log();
        assert!(sent(&mut rx).is_some());
    }

    #[test]
    fn test_poll_runs_in_detail() {
        let (mut app, mut rx) = opened(lines(0..3));
        log(&mut app).detail = Some(LogDetail {
            line: line(0, "INF"),
            scroll: 0,
        });
        make_due(&mut app);
        app.poll_system_log();
        assert!(sent(&mut rx).is_some());
    }

    // ========================================
    // REPLIES
    // ========================================

    #[test]
    fn test_apply_merges_and_clears_flags() {
        let (mut app, _rx) = setup();
        app.open_system_log().unwrap();
        app.apply_system_log(1, Ok(lines(0..2))).unwrap();
        let state = log(&mut app);
        assert_eq!(state.lines.len(), 2);
        assert!(!state.in_flight);
        assert!(state.loaded);
        assert_eq!(state.error, None);
    }

    #[test]
    fn test_apply_failure_keeps_lines_and_retries() {
        let (mut app, mut rx) = opened(lines(0..2));
        app.apply_system_log(1, Err(anyhow::anyhow!("boom")))
            .unwrap();
        assert_eq!(log(&mut app).lines.len(), 2);
        assert_eq!(log(&mut app).error.as_deref(), Some("boom"));
        make_due(&mut app);
        app.poll_system_log();
        assert!(sent(&mut rx).is_some());
        app.apply_system_log(1, Ok(vec![])).unwrap();
        assert_eq!(log(&mut app).error, None);
    }

    #[test]
    fn test_apply_drops_stale_generation() {
        let (mut app, _rx) = setup();
        app.open_system_log().unwrap();
        app.close_system_log();
        app.open_system_log().unwrap();
        app.apply_system_log(1, Ok(lines(0..2))).unwrap();
        let state = log(&mut app);
        assert!(state.lines.is_empty());
        assert!(state.in_flight);
    }

    #[test]
    fn test_apply_after_close_is_ok_and_ignored() {
        let (mut app, _rx) = setup();
        app.open_system_log().unwrap();
        app.close_system_log();
        assert_eq!(app.apply_system_log(1, Ok(vec![numeric_line()])), Ok(()));
        assert!(app.model.ui.system_log.is_none());
    }

    #[test]
    fn test_apply_1x_reply_closes() {
        let (mut app, _rx) = setup();
        app.model.syncthing.version = None;
        app.open_system_log().unwrap();
        app.model.syncthing.version = Some("v1.30.0".to_string());
        assert_eq!(
            app.apply_system_log(1, Ok(vec![numeric_line()])),
            Err(TOO_OLD.to_string())
        );
        assert!(app.model.ui.system_log.is_none());
    }

    #[test]
    fn test_system_log_1x_reply_warns() {
        let (mut app, _rx) = setup();
        app.model.syncthing.version = None;
        app.open_system_log().unwrap();
        crate::handlers::handle_api_response(
            &mut app,
            crate::services::api::ApiResponse::SystemLogResult {
                generation: 1,
                lines: Ok(vec![numeric_line()]),
            },
        );
        let toast = app.model.ui.toast.as_ref().unwrap();
        assert_eq!(toast.kind, crate::model::ui::ToastKind::Warning);
        assert_eq!(toast.message, "The log viewer needs Syncthing 2.0 or newer");
    }

    // ========================================
    // KEYS
    // ========================================

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    async fn press(app: &mut App, code: KeyCode) {
        press_with(app, code, KeyModifiers::NONE).await;
    }

    async fn press_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        crate::handlers::handle_key(app, KeyEvent::new(code, modifiers))
            .await
            .unwrap();
    }

    async fn ctrl(app: &mut App, c: char) {
        press_with(app, KeyCode::Char(c), KeyModifiers::CONTROL).await;
    }

    #[tokio::test]
    async fn test_v_opens_in_folder_view() {
        let (mut app, mut rx) = setup();
        press(&mut app, KeyCode::Char('v')).await;
        assert!(app.model.ui.system_log.is_some());
        assert!(sent(&mut rx).is_some());
    }

    #[tokio::test]
    async fn test_g_before_opening_does_not_count_inside() {
        let (mut app, _rx) = setup();
        app.model.ui.vim_mode = true;
        press(&mut app, KeyCode::Char('g')).await;
        press(&mut app, KeyCode::Char('v')).await;
        app.apply_system_log(1, Ok(lines(0..3))).unwrap();
        press(&mut app, KeyCode::Char('g')).await;
        assert_eq!(log(&mut app).selected, Some(2));
    }

    #[tokio::test]
    async fn test_v_does_nothing_in_breadcrumbs() {
        let (mut app, _rx) = setup();
        crate::test_support::view_folder_root(&mut app, "f", vec![]);
        press(&mut app, KeyCode::Char('v')).await;
        assert!(app.model.ui.system_log.is_none());
    }

    #[tokio::test]
    async fn test_v_on_1x_warns() {
        let (mut app, _rx) = setup();
        app.model.syncthing.version = Some("v1.30.0".to_string());
        press(&mut app, KeyCode::Char('v')).await;
        assert!(app.model.ui.system_log.is_none());
        let toast = app.model.ui.toast.as_ref().unwrap();
        assert_eq!(toast.kind, crate::model::ui::ToastKind::Warning);
        assert_eq!(toast.message, TOO_OLD);
    }

    #[tokio::test]
    async fn test_v_cycles_filter() {
        let (mut app, _rx) = opened(lines(0..3));
        for filter in [LogFilter::Warnings, LogFilter::Errors, LogFilter::All] {
            press(&mut app, KeyCode::Char('v')).await;
            assert_eq!(log(&mut app).filter, filter);
        }
    }

    #[tokio::test]
    async fn test_esc_closes() {
        let (mut app, _rx) = opened(lines(0..3));
        press(&mut app, KeyCode::Esc).await;
        assert!(app.model.ui.system_log.is_none());
    }

    #[tokio::test]
    async fn test_v_does_nothing_in_detail() {
        let (mut app, _rx) = opened(lines(0..3));
        press(&mut app, KeyCode::Enter).await;
        press(&mut app, KeyCode::Char('v')).await;
        assert_eq!(log(&mut app).filter, LogFilter::All);
        assert!(log(&mut app).detail.is_some());
    }

    #[tokio::test]
    async fn test_enter_opens_detail_on_selected() {
        let (mut app, _rx) = opened(lines(0..3));
        press(&mut app, KeyCode::Up).await;
        press(&mut app, KeyCode::Enter).await;
        let detail = log(&mut app).detail.clone().unwrap();
        assert_eq!(detail.line.message, "m1");
        assert_eq!(detail.scroll, 0);
    }

    #[tokio::test]
    async fn test_enter_without_selection_does_nothing() {
        let (mut app, _rx) = opened(vec![]);
        press(&mut app, KeyCode::Enter).await;
        assert!(log(&mut app).detail.is_none());
    }

    #[tokio::test]
    async fn test_esc_leaves_detail_then_closes() {
        let (mut app, _rx) = opened(lines(0..3));
        press(&mut app, KeyCode::Enter).await;
        press(&mut app, KeyCode::Esc).await;
        assert!(log(&mut app).detail.is_none());
        press(&mut app, KeyCode::Esc).await;
        assert!(app.model.ui.system_log.is_none());
    }

    #[tokio::test]
    async fn test_arrows_move_and_scroll_detail() {
        let (mut app, _rx) = opened(lines(0..3));
        press(&mut app, KeyCode::Up).await;
        assert_eq!(log(&mut app).selected, Some(1));
        press(&mut app, KeyCode::Down).await;
        assert_eq!(log(&mut app).selected, Some(2));
        press(&mut app, KeyCode::Enter).await;
        let scroll = |app: &mut App| log(app).detail.as_ref().unwrap().scroll;
        press(&mut app, KeyCode::Down).await;
        press(&mut app, KeyCode::PageDown).await;
        assert_eq!(scroll(&mut app), 11);
        press(&mut app, KeyCode::Up).await;
        assert_eq!(scroll(&mut app), 10);
        press(&mut app, KeyCode::Home).await;
        press(&mut app, KeyCode::Up).await;
        assert_eq!(scroll(&mut app), 0);
        press(&mut app, KeyCode::End).await;
        press(&mut app, KeyCode::Down).await;
        assert_eq!(scroll(&mut app), u16::MAX);
        assert_eq!(log(&mut app).selected, Some(2));
    }

    async fn assert_page_home_end(app: &mut App) {
        for (code, selected) in [
            (KeyCode::PageUp, 20),
            (KeyCode::Home, 0),
            (KeyCode::PageDown, 10),
            (KeyCode::Up, 9),
            (KeyCode::Down, 10),
            (KeyCode::End, 30),
        ] {
            press(app, code).await;
            assert_eq!(log(app).selected, Some(selected), "{code:?}");
        }
    }

    #[tokio::test]
    async fn test_page_home_end() {
        let (mut app, _rx) = opened(lines(0..31));
        assert_page_home_end(&mut app).await;
        app.model.ui.vim_mode = true;
        assert_page_home_end(&mut app).await;
    }

    #[tokio::test]
    async fn test_vim_keys() {
        let (mut app, _rx) = opened(lines(0..31));
        app.model.ui.vim_mode = true;
        let selected = |app: &mut App| log(app).selected.unwrap();
        press(&mut app, KeyCode::Char('k')).await;
        assert_eq!(selected(&mut app), 29);
        press(&mut app, KeyCode::Char('j')).await;
        assert_eq!(selected(&mut app), 30);
        ctrl(&mut app, 'u').await;
        assert_eq!(selected(&mut app), 25);
        ctrl(&mut app, 'd').await;
        assert_eq!(selected(&mut app), 30);
        ctrl(&mut app, 'b').await;
        assert_eq!(selected(&mut app), 20);
        ctrl(&mut app, 'f').await;
        assert_eq!(selected(&mut app), 30);
        press(&mut app, KeyCode::Char('g')).await;
        assert_eq!(selected(&mut app), 30);
        press(&mut app, KeyCode::Char('g')).await;
        assert_eq!(selected(&mut app), 0);
        press_with(&mut app, KeyCode::Char('G'), KeyModifiers::SHIFT).await;
        assert_eq!(selected(&mut app), 30);
        // A key between the two cancels gg
        press(&mut app, KeyCode::Char('g')).await;
        press(&mut app, KeyCode::Char('k')).await;
        press(&mut app, KeyCode::Char('g')).await;
        assert_eq!(selected(&mut app), 29);
    }

    #[tokio::test]
    async fn test_vim_keys_need_vim_mode() {
        let (mut app, _rx) = opened(lines(0..31));
        for c in ['k', 'g', 'g'] {
            press(&mut app, KeyCode::Char(c)).await;
        }
        ctrl(&mut app, 'u').await;
        ctrl(&mut app, 'b').await;
        assert_eq!(log(&mut app).selected, Some(30));
    }

    #[tokio::test]
    async fn test_other_keys_are_swallowed() {
        let (mut app, _rx) = opened(lines(0..3));
        app.model.set_folders(vec![
            serde_json::from_value(serde_json::json!({
                "id": "f", "label": "f", "path": "/data", "paused": false, "type": "sendreceive"
            }))
            .unwrap(),
        ]);
        app.model.navigation.folders_state_selection = Some(0);
        press(&mut app, KeyCode::Char('p')).await;
        press(&mut app, KeyCode::Char('q')).await;
        assert!(app.model.ui.confirm_action.is_none());
        assert!(!app.model.ui.should_quit);
        assert!(app.model.ui.system_log.is_some());
    }

    #[tokio::test]
    async fn test_keys_on_empty_list_do_not_panic() {
        let (mut app, _rx) = opened(lines(0..3));
        app.model.ui.vim_mode = true;
        press(&mut app, KeyCode::Char('v')).await;
        assert_eq!(log(&mut app).selected, None);
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::Enter,
            KeyCode::Char('j'),
            KeyCode::Char('G'),
        ] {
            press(&mut app, code).await;
        }
        assert_eq!(log(&mut app).selected, None);
    }

    #[tokio::test]
    async fn test_detail_survives_line_dropping() {
        let (mut app, _rx) = opened(lines(0..1));
        press(&mut app, KeyCode::Enter).await;
        app.apply_system_log(1, Ok(lines(1..1002))).unwrap();
        let state = log(&mut app);
        assert!(!state.lines.contains(&line(0, "INF")));
        assert_eq!(state.detail.as_ref().unwrap().line.message, "m0");
    }
}
