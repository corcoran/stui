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
/// use stui::logic::formatting::format_uptime;
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

/// Format file size in human-readable format with 4-character alignment
///
/// Converts bytes into a compact, aligned representation suitable for terminal display.
/// Always uses 4 characters or less (with decimal point counting as a character).
///
/// # Format Examples
/// - `0` → `"   0"`
/// - `512` → ` 512"`
/// - `1536` → `"1.5K"`
/// - `15360` → `" 15K"`
/// - `1572864` → `"1.5M"`
/// - `1073741824` → `"  1G"`
///
/// # Arguments
/// * `size` - Size in bytes
///
/// # Returns
/// Formatted string with 4-character alignment (spaces included for small values)
///
/// # Examples
/// ```
/// use stui::logic::formatting::format_human_size;
///
/// assert_eq!(format_human_size(0), "   0");
/// assert_eq!(format_human_size(512), " 512");
/// assert_eq!(format_human_size(1536), "1.5K");
/// assert_eq!(format_human_size(15360), " 15K");
/// assert_eq!(format_human_size(1572864), "1.5M");
/// ```
pub fn format_human_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if size == 0 {
        return "   0".to_string();
    } else if size < KB {
        return format!("{:>4}", size);
    } else if size < MB {
        let kb = size as f64 / KB as f64;
        if kb < 10.0 {
            return format!("{:.1}K", kb);
        } else {
            return format!("{:>3}K", (size / KB));
        }
    } else if size < GB {
        let mb = size as f64 / MB as f64;
        if mb < 10.0 {
            return format!("{:.1}M", mb);
        } else {
            return format!("{:>3}M", (size / MB));
        }
    } else if size < TB {
        let gb = size as f64 / GB as f64;
        if gb < 10.0 {
            return format!("{:.1}G", gb);
        } else {
            return format!("{:>3}G", (size / GB));
        }
    } else {
        let tb = size as f64 / TB as f64;
        if tb < 10.0 {
            return format!("{:.1}T", tb);
        } else {
            return format!("{:>3}T", (size / TB));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // FORMAT UPTIME
    // ========================================

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

    // ========================================
    // FORMAT HUMAN SIZE
    // ========================================

    #[test]
    fn test_format_human_size_zero() {
        assert_eq!(format_human_size(0), "   0");
    }

    #[test]
    fn test_format_human_size_bytes() {
        assert_eq!(format_human_size(1), "   1");
        assert_eq!(format_human_size(512), " 512");
        assert_eq!(format_human_size(1023), "1023");
    }

    #[test]
    fn test_format_human_size_kilobytes() {
        assert_eq!(format_human_size(1024), "1.0K");
        assert_eq!(format_human_size(1536), "1.5K");
        assert_eq!(format_human_size(10240), " 10K");
        assert_eq!(format_human_size(102400), "100K");
    }

    #[test]
    fn test_format_human_size_megabytes() {
        assert_eq!(format_human_size(1048576), "1.0M");
        assert_eq!(format_human_size(1572864), "1.5M");
        assert_eq!(format_human_size(10485760), " 10M");
        assert_eq!(format_human_size(104857600), "100M");
    }

    #[test]
    fn test_format_human_size_gigabytes() {
        assert_eq!(format_human_size(1073741824), "1.0G");
        assert_eq!(format_human_size(1610612736), "1.5G");
        assert_eq!(format_human_size(10737418240), " 10G");
    }

    #[test]
    fn test_format_human_size_terabytes() {
        assert_eq!(format_human_size(1099511627776), "1.0T");
        assert_eq!(format_human_size(1649267441664), "1.5T");
    }
}
