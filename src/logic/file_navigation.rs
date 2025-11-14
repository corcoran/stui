//! File navigation logic
//!
//! Pure functions for parsing file paths and navigating to them.

/// Parse a file path into directory components and filename
///
/// # Arguments
/// * `path` - Full file path like "dir1/dir2/file.txt"
///
/// # Returns
/// Tuple of (directory_components, filename)
/// - directory_components: Vec of directory names to traverse
/// - filename: Final filename to select
///
/// # Examples
/// ```
/// use stui::logic::file_navigation::parse_file_path;
///
/// let (dirs, file) = parse_file_path("projects/2024/report.txt");
/// assert_eq!(dirs, vec!["projects", "2024"]);
/// assert_eq!(file, "report.txt");
/// ```
pub fn parse_file_path(path: &str) -> (Vec<String>, String) {
    if path.is_empty() {
        return (vec![], String::new());
    }

    let components: Vec<&str> = path.split('/').collect();

    if components.len() == 1 {
        // Root-level file (no directories)
        return (vec![], components[0].to_string());
    }

    // All components except last are directories
    let dirs = components[..components.len() - 1]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let filename = components.last().unwrap().to_string();

    (dirs, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // PATH PARSING
    // ========================================

    #[test]
    fn test_parse_file_path_deep() {
        let (dirs, file) = parse_file_path("projects/2024/Q3/report.txt");
        assert_eq!(dirs, vec!["projects", "2024", "Q3"]);
        assert_eq!(file, "report.txt");
    }

    #[test]
    fn test_parse_file_path_root_level() {
        let (dirs, file) = parse_file_path("readme.txt");
        assert_eq!(dirs.len(), 0);
        assert_eq!(file, "readme.txt");
    }

    #[test]
    fn test_parse_file_path_single_dir() {
        let (dirs, file) = parse_file_path("docs/readme.txt");
        assert_eq!(dirs, vec!["docs"]);
        assert_eq!(file, "readme.txt");
    }

    #[test]
    fn test_parse_file_path_with_spaces() {
        let (dirs, file) = parse_file_path("My Documents/2024 Reports/final report.txt");
        assert_eq!(dirs, vec!["My Documents", "2024 Reports"]);
        assert_eq!(file, "final report.txt");
    }

    #[test]
    fn test_parse_file_path_empty() {
        let (dirs, file) = parse_file_path("");
        assert_eq!(dirs.len(), 0);
        assert_eq!(file, "");
    }
}
