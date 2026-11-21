//! SIGINT and SIGQUIT, which the terminal sends to stui as well as to a program running in
//! the terminal (Ctrl-C, Ctrl-\ in `less`)

use crate::App;

impl App {
    /// Whether SIGINT or SIGQUIT arrived while stui owned the terminal
    pub(crate) fn quit_requested_by_signal(&mut self) -> bool {
        let mut requested = false;
        while self.signal_rx.try_recv().is_ok() {
            requested = true;
        }
        requested
    }

    /// Drops signals meant for a program that just ran in the terminal
    pub(crate) fn discard_signals(&mut self) {
        while self.signal_rx.try_recv().is_ok() {}
    }
}

/// Forwards SIGINT and SIGQUIT to `tx` instead of letting them end stui. Installed once at
/// startup; the default handlers come back in programs stui starts (exec resets them).
#[cfg(unix)]
pub(crate) fn forward_quit_signals(tx: tokio::sync::mpsc::UnboundedSender<()>) {
    use tokio::signal::unix::{SignalKind, signal};
    for kind in [SignalKind::interrupt(), SignalKind::quit()] {
        let tx = tx.clone();
        match signal(kind) {
            Ok(mut stream) => {
                tokio::spawn(async move {
                    while stream.recv().await.is_some() {
                        let _ = tx.send(());
                    }
                });
            }
            Err(e) => crate::log_debug(&format!("Could not handle signal {:?}: {}", kind, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::*;

    #[test]
    fn test_signal_while_stui_owns_the_terminal_quits() {
        let mut app = create_test_app();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.signal_rx = rx;
        tx.send(()).unwrap();

        assert!(app.quit_requested_by_signal());
        assert!(!app.quit_requested_by_signal());
    }

    #[test]
    fn test_signals_during_a_terminal_program_are_discarded() {
        let mut app = create_test_app();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.signal_rx = rx;
        tx.send(()).unwrap();
        tx.send(()).unwrap();

        app.discard_signals();

        assert!(!app.quit_requested_by_signal());
    }
}
