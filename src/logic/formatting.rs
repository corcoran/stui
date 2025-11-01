//! Formatting and display logic
//!
//! Pure functions for formatting data for human-readable display.

/// Format uptime duration in human-readable format
///
/// Converts seconds into a compact representation showing the two most
/// significant units (days+hours, hours+minutes, or just minutes).
///
/// # Arguments
/// * `seconds` - Uptime in seconds
///
/// # Returns
/// Formatted string like "5d 3h", "2h 45m", or "30m"
///
/// # Examples
/// ```
/// use synctui::logic::formatting::format_uptime;
///
/// assert_eq!(format_uptime(0), "0m");
/// assert_eq!(format_uptime(30), "0m");  // Rounds down to 0 minutes
/// assert_eq!(format_uptime(60), "1m");
/// assert_eq!(format_uptime(3600), "1h 0m");
/// assert_eq!(format_uptime(3660), "1h 1m");
/// assert_eq!(format_uptime(86400), "1d 0h");
/// assert_eq!(format_uptime(90061), "1d 1h");  // 1 day, 1 hour, 1 minute (drops minutes)
/// ```
pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_uptime_zero() {
        assert_eq!(format_uptime(0), "0m");
    }

    #[test]
    fn test_format_uptime_seconds_only() {
        // Seconds are discarded, rounds down to minutes
        assert_eq!(format_uptime(30), "0m");
        assert_eq!(format_uptime(59), "0m");
    }

    #[test]
    fn test_format_uptime_minutes() {
        assert_eq!(format_uptime(60), "1m");
        assert_eq!(format_uptime(120), "2m");
        assert_eq!(format_uptime(1800), "30m");
        assert_eq!(format_uptime(3599), "59m");
    }

    #[test]
    fn test_format_uptime_hours() {
        // 1 hour exact
        assert_eq!(format_uptime(3600), "1h 0m");
        // 1 hour 30 minutes
        assert_eq!(format_uptime(5400), "1h 30m");
        // 2 hours 45 minutes
        assert_eq!(format_uptime(9900), "2h 45m");
        // 23 hours 59 minutes
        assert_eq!(format_uptime(86340), "23h 59m");
    }

    #[test]
    fn test_format_uptime_days() {
        // 1 day exact
        assert_eq!(format_uptime(86400), "1d 0h");
        // 1 day 1 hour (drops minutes)
        assert_eq!(format_uptime(90000), "1d 1h");
        // 5 days 12 hours
        assert_eq!(format_uptime(475200), "5d 12h");
        // 30 days 23 hours
        assert_eq!(format_uptime(2674800), "30d 23h");
    }

    #[test]
    fn test_format_uptime_mixed() {
        // 1 day, 2 hours, 30 minutes - shows days+hours, drops minutes
        assert_eq!(format_uptime(95400), "1d 2h");
    }
}
