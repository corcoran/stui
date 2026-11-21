//! Feedback for finished undelete batches

use crate::App;
use crate::app::ignore::UndeleteDone;

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Report a finished undelete batch and start the folder's next one
pub fn handle_undelete_done(app: &mut App, done: UndeleteDone) {
    let what = match done.paths.as_slice() {
        [one] => file_name(one).to_string(),
        many => format!("{} files", many.len()),
    };
    match &done.result {
        Ok(()) => app.model.ui.show_success(format!("Restored {}", what)),
        Err(e) => {
            app.model
                .ui
                .show_error(format!("Failed to restore {}: {}", what, e));
            app.mark_undelete_failed(&done);
        }
    }
    app.finish_undelete_batch(&done);
}
