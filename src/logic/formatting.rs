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
        "   0".to_string()
    } else if size < KB {
        format!("{:>4}", size)
    } else if size < MB {
        let kb = size as f64 / KB as f64;
        if kb < 10.0 {
            format!("{:.1}K", kb)
        } else {
            format!("{:>3}K", (size / KB))
        }
    } else if size < GB {
        let mb = size as f64 / MB as f64;
        if mb < 10.0 {
            format!("{:.1}M", mb)
        } else {
            format!("{:>3}M", (size / MB))
        }
    } else if size < TB {
        let gb = size as f64 / GB as f64;
        if gb < 10.0 {
            format!("{:.1}G", gb)
        } else {
            format!("{:>3}G", (size / GB))
        }
    } else {
        let tb = size as f64 / TB as f64;
        if tb < 10.0 {
            format!("{:.1}T", tb)
        } else {
            format!("{:>3}T", (size / TB))
        }
    }
}

/// Format an RFC 3339 timestamp in the given timezone using a chrono format string.
///
/// Returns `None` if the input does not parse.
pub fn format_rfc3339_in<Tz>(rfc3339: &str, tz: &Tz, fmt: &str) -> Option<String>
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|dt| dt.with_timezone(tz).format(fmt).to_string())
}

/// Format an RFC 3339 timestamp in the machine's local timezone.
pub fn format_rfc3339_local(rfc3339: &str, fmt: &str) -> Option<String> {
    format_rfc3339_in(rfc3339, &chrono::Local, fmt)
}

/// Format a `SystemTime` in the given timezone. Out-of-range values format as the Unix epoch.
pub fn format_system_time_in<Tz>(time: &std::time::SystemTime, tz: &Tz, fmt: &str) -> String
where
    Tz: chrono::TimeZone,
    Tz::Offset: std::fmt::Display,
{
    use std::time::UNIX_EPOCH;

    let secs = time
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let utc = chrono::DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
    utc.with_timezone(tz).format(fmt).to_string()
}

/// Format a `SystemTime` in the machine's local timezone.
pub fn format_system_time_local(time: &std::time::SystemTime, fmt: &str) -> String {
    format_system_time_in(time, &chrono::Local, fmt)
}

/// Format an RFC 3339 datetime string as local time (YYYY-MM-DD HH:MM:SS)
///
/// Returns the original string if parsing fails.
pub fn format_datetime(rfc3339: &str) -> String {
    format_rfc3339_local(rfc3339, "%Y-%m-%d %H:%M:%S").unwrap_or_else(|| rfc3339.to_string())
}

/// Format time since event for status bar
///
/// Converts a SystemTime timestamp into a human-readable relative time string.
///
/// # Arguments
/// * `timestamp` - SystemTime of the event
///
/// # Returns
/// Formatted string like "5 sec ago", "2 min ago", "3 hr ago", "Yesterday", or "5 days ago"
///
/// # Examples
/// ```no_run
/// use std::time::{SystemTime, Duration};
/// use stui::logic::formatting::format_time_since;
///
/// let now = SystemTime::now();
/// let thirty_secs_ago = now - Duration::from_secs(30);
/// assert_eq!(format_time_since(thirty_secs_ago), "30 sec ago");
/// ```
pub fn format_time_since(timestamp: std::time::SystemTime) -> String {
    let elapsed = timestamp
        .elapsed()
        .unwrap_or(std::time::Duration::from_secs(0));

    let secs = elapsed.as_secs();

    if secs < 60 {
        format!("{} sec ago", secs)
    } else if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hr ago", secs / 3600)
    } else if secs < 172800 {
        "Yesterday".to_string()
    } else {
        format!("{} days ago", secs / 86400)
    }
}

/// Name of the device whose ID starts with `short_id`, as in the short device IDs
/// Syncthing reports in `modifiedBy`. `None` when no device, or more than one, matches.
pub fn device_name_by_short_id<'a>(
    devices: &'a [crate::api::Device],
    short_id: &str,
) -> Option<&'a str> {
    if short_id.is_empty() {
        return None;
    }
    let mut matches = devices.iter().filter(|d| d.id.starts_with(short_id));
    match (matches.next(), matches.next()) {
        (Some(device), None) => Some(device.name.as_str()),
        _ => None,
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

    // ========================================
    // FORMAT TIME SINCE
    // ========================================

    #[test]
    fn test_format_time_since_seconds() {
        let timestamp = std::time::SystemTime::now() - std::time::Duration::from_secs(30);
        let formatted = format_time_since(timestamp);
        assert!(formatted.contains("sec ago"));
    }

    #[test]
    fn test_format_time_since_minutes() {
        let timestamp = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        let formatted = format_time_since(timestamp);
        assert!(formatted.contains("min ago"));
    }

    #[test]
    fn test_format_time_since_hours() {
        let timestamp = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        let formatted = format_time_since(timestamp);
        assert!(formatted.contains("hr ago"));
    }

    // ========================================
    // LOCAL TIME CONVERSION
    // ========================================

    fn est() -> chrono::FixedOffset {
        chrono::FixedOffset::west_opt(5 * 3600).unwrap()
    }

    #[test]
    fn test_rfc3339_z_converts_to_offset() {
        let out = format_rfc3339_in("2025-10-26T20:58:21.580021398Z", &est(), "%Y-%m-%d %H:%M");
        assert_eq!(out.as_deref(), Some("2025-10-26 15:58"));
    }

    #[test]
    fn test_rfc3339_z_crosses_midnight_to_previous_date() {
        let out = format_rfc3339_in("2025-01-16T03:44:10Z", &est(), "%Y-%m-%d %H:%M:%S");
        assert_eq!(out.as_deref(), Some("2025-01-15 22:44:10"));
    }

    #[test]
    fn test_rfc3339_zero_offset_form_converts() {
        let out = format_rfc3339_in("2025-01-16T03:44:10+00:00", &est(), "%H:%M");
        assert_eq!(out.as_deref(), Some("22:44"));
    }

    #[test]
    fn test_rfc3339_non_utc_offset_converts() {
        let out = format_rfc3339_in("2025-01-15T22:30:00+02:00", &est(), "%Y-%m-%d %H:%M");
        assert_eq!(out.as_deref(), Some("2025-01-15 15:30"));
    }

    #[test]
    fn test_rfc3339_unparseable_returns_none() {
        assert_eq!(format_rfc3339_in("invalid", &est(), "%H:%M"), None);
        assert_eq!(format_rfc3339_in("", &est(), "%H:%M"), None);
    }

    #[test]
    fn test_system_time_converts_to_offset() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_736_999_050);
        // 2025-01-16 03:44:10 UTC
        assert_eq!(
            format_system_time_in(&t, &est(), "%Y-%m-%d %H:%M:%S"),
            "2025-01-15 22:44:10"
        );
    }

    #[test]
    fn test_system_time_before_epoch_formats_as_epoch_in_tz() {
        let t = std::time::UNIX_EPOCH - std::time::Duration::from_secs(10);
        assert_eq!(
            format_system_time_in(&t, &est(), "%Y-%m-%d %H:%M:%S"),
            "1969-12-31 19:00:00"
        );
    }

    #[test]
    fn test_format_datetime_invalid_falls_back_to_original() {
        assert_eq!(format_datetime("invalid"), "invalid");
    }

    // ========================================
    // DEVICE NAMES
    // ========================================

    fn device(id: &str, name: &str) -> crate::api::Device {
        crate::api::Device {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn cluster() -> Vec<crate::api::Device> {
        vec![
            device(
                "MLCMR7B-GJX2AQF-FNDZ2ZZ-MQOQUK6-SH7GYF4-U4VXXOT-D3CPGA4-BNBNOQ2",
                "laptop",
            ),
            device(
                "3QFSJG3-7K2LR5B-DRDC4JV-OTJNFIQ-3BJ7XBI-ZUIIZXR-WGBITZK-2DLWAQJ",
                "nas",
            ),
        ]
    }

    #[test]
    fn test_device_name_by_short_id_matches_first_group() {
        assert_eq!(device_name_by_short_id(&cluster(), "3QFSJG3"), Some("nas"));
        assert_eq!(
            device_name_by_short_id(&cluster(), "MLCMR7B"),
            Some("laptop")
        );
    }

    #[test]
    fn test_device_name_by_short_id_accepts_full_id() {
        let full = "3QFSJG3-7K2LR5B-DRDC4JV-OTJNFIQ-3BJ7XBI-ZUIIZXR-WGBITZK-2DLWAQJ";
        assert_eq!(device_name_by_short_id(&cluster(), full), Some("nas"));
    }

    #[test]
    fn test_device_name_by_short_id_unknown_or_empty_is_none() {
        assert_eq!(device_name_by_short_id(&cluster(), "ZZZZZZZ"), None);
        assert_eq!(device_name_by_short_id(&cluster(), ""), None);
    }

    #[test]
    fn test_device_name_by_short_id_ambiguous_is_none() {
        let devices = vec![
            device(
                "3QFSJG3-AAAAAAA-AAAAAAA-AAAAAAA-AAAAAAA-AAAAAAA-AAAAAAA-AAAAAAA",
                "one",
            ),
            device(
                "3QFSJG3-BBBBBBB-BBBBBBB-BBBBBBB-BBBBBBB-BBBBBBB-BBBBBBB-BBBBBBB",
                "two",
            ),
        ];
        assert_eq!(device_name_by_short_id(&devices, "3QFSJG3"), None);
    }
}
