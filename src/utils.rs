/// Utility functions used throughout the application

use std::path::PathBuf;

/// Get platform-specific debug log path
pub fn get_debug_log_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("stui-debug.log");
    path
}

/// Get platform-specific cache fallback path
pub fn get_cache_fallback_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push("stui-cache");
    path
}

/// Format bytes into human-readable string (e.g., "1.2 KB", "5.3 MB")
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
