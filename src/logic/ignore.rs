//! Ignore Pattern Matching Logic
//!
//! This module contains pure functions for matching files against Syncthing .stignore patterns.
//! Patterns follow similar rules to .gitignore:
//! - Patterns starting with `/` are relative to folder root
//! - Patterns without `/` match anywhere in the path
//! - Supports glob patterns (e.g., `*.tmp`, `temp*`, `**/cache`)

/// Check if a file path matches a Syncthing ignore pattern
///
/// # Pattern Rules
/// - Patterns starting with `/` match from the folder root
/// - Patterns without `/` match anywhere in the path (including just the filename)
/// - Supports glob patterns using the `glob` crate
///
/// # Examples
/// ```
/// use synctui::logic::ignore::pattern_matches;
///
/// assert!(pattern_matches("/foo/bar.txt", "/foo/bar.txt"));
/// assert!(pattern_matches("*.tmp", "/some/path/file.tmp"));
/// assert!(pattern_matches("cache", "/foo/cache/data"));
/// ```
pub fn pattern_matches(pattern: &str, file_path: &str) -> bool {
    // Syncthing ignore patterns are similar to .gitignore
    // Patterns starting with / are relative to folder root
    // Patterns without / match anywhere in the path

    let pattern = pattern.trim();

    // Exact match
    if pattern == file_path {
        return true;
    }

    // Pattern starts with / - match from root
    if let Some(pattern_without_slash) = pattern.strip_prefix('/') {
        // Exact match without leading slash
        if pattern_without_slash == file_path.trim_start_matches('/') {
            return true;
        }

        // Try glob matching
        if let Ok(pattern_obj) = glob::Pattern::new(pattern_without_slash) {
            if pattern_obj.matches(file_path.trim_start_matches('/')) {
                return true;
            }
        }
    } else {
        // Pattern without / - match anywhere
        // Try matching the full path
        if let Ok(pattern_obj) = glob::Pattern::new(pattern) {
            if pattern_obj.matches(file_path.trim_start_matches('/')) {
                return true;
            }

            // Also try matching just the filename
            if let Some(filename) = file_path.split('/').last() {
                if pattern_obj.matches(filename) {
                    return true;
                }
            }
        }
    }

    false
}

/// Find all patterns that match a given file path
///
/// Returns a list of patterns from the input that match the file path.
///
/// # Examples
/// ```
/// use synctui::logic::ignore::find_matching_patterns;
///
/// let patterns = vec![
///     "*.tmp".to_string(),
///     "/cache/".to_string(),
///     "*.log".to_string(),
/// ];
///
/// let matches = find_matching_patterns(&patterns, "/foo/bar.tmp");
/// assert_eq!(matches, vec!["*.tmp"]);
/// ```
pub fn find_matching_patterns(patterns: &[String], file_path: &str) -> Vec<String> {
    patterns
        .iter()
        .filter(|p| pattern_matches(p, file_path))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(pattern_matches("/foo/bar.txt", "/foo/bar.txt"));
        assert!(!pattern_matches("/foo/bar.txt", "/foo/baz.txt"));
    }

    #[test]
    fn test_root_pattern() {
        // Pattern starting with / matches from root
        assert!(pattern_matches("/cache", "/cache"));
        assert!(pattern_matches("/cache", "cache"));
        assert!(!pattern_matches("/cache", "/foo/cache"));
    }

    #[test]
    fn test_wildcard_pattern() {
        // Pattern without / matches anywhere
        assert!(pattern_matches("*.tmp", "/foo/bar.tmp"));
        assert!(pattern_matches("*.tmp", "/a/b/c.tmp"));
        assert!(!pattern_matches("*.tmp", "/foo/bar.txt"));
    }

    #[test]
    fn test_pattern_matches_filename() {
        // Pattern should match just the filename
        assert!(pattern_matches("cache", "/foo/cache"));
        assert!(pattern_matches("cache", "/foo/bar/cache"));
    }

    #[test]
    fn test_glob_patterns() {
        assert!(pattern_matches("temp*", "/foo/temporary.txt"));
        assert!(pattern_matches("*cache*", "/foo/mycache.db"));
        assert!(pattern_matches("test?.txt", "/foo/test1.txt"));
    }

    #[test]
    fn test_find_matching_patterns() {
        let patterns = vec![
            "*.tmp".to_string(),
            "/cache/".to_string(),
            "*.log".to_string(),
            "specific.txt".to_string(),
        ];

        let matches = find_matching_patterns(&patterns, "/foo/bar.tmp");
        assert_eq!(matches, vec!["*.tmp"]);

        let matches = find_matching_patterns(&patterns, "/cache/data");
        assert!(matches.is_empty()); // "/cache/" pattern won't match "/cache/data" exactly

        let matches = find_matching_patterns(&patterns, "/foo/bar.log");
        assert_eq!(matches, vec!["*.log"]);
    }

    #[test]
    fn test_multiple_matches() {
        let patterns = vec![
            "*.tmp".to_string(),
            "temp*".to_string(),
            "/foo/bar.tmp".to_string(),
        ];

        let matches = find_matching_patterns(&patterns, "/foo/bar.tmp");
        // Should match two patterns: *.tmp and /foo/bar.tmp
        // (temp* doesn't match because filename is bar.tmp, not temp*)
        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&"*.tmp".to_string()));
        assert!(matches.contains(&"/foo/bar.tmp".to_string()));
    }
}
