//! Feedback for programs stui ran inside the terminal

use crate::App;

/// Toast for how a program run with `TerminalRequest::RunInTerminal` ended
pub fn handle_run_result(
    app: &mut App,
    program: &str,
    result: std::io::Result<std::process::ExitStatus>,
) {
    match result {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let code = status
                .code()
                .map_or_else(|| status.to_string(), |code| code.to_string());
            app.model
                .ui
                .show_error(format!("{} exited with status {}", program, code));
        }
        Err(error) => app
            .model
            .ui
            .show_error(format!("Couldn't run '{}': {}", program, error)),
    }
}
