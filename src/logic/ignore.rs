//! Ignore Pattern Matching and Validation Logic
//!
//! This module contains pure functions for matching files against Syncthing .stignore patterns
//! and validating pattern syntax.
//!
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
/// use stui::logic::ignore::pattern_matches;
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
        // Pattern without / - match anywhere (path components or filename)
        if let Ok(pattern_obj) = glob::Pattern::new(pattern) {
            // Try matching the full path
            if pattern_obj.matches(file_path.trim_start_matches('/')) {
                return true;
            }

            // Try matching each path component (directories and filename)
            for component in file_path.split('/') {
                if !component.is_empty() && pattern_obj.matches(component) {
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
/// use stui::logic::ignore::find_matching_patterns;
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

/// Validate an ignore pattern for common syntax errors
///
/// Checks for common pattern syntax errors before sending to the Syncthing API.
/// This helps provide immediate feedback to users about invalid patterns.
///
/// # Validation Rules
/// - Pattern cannot be empty or whitespace-only
/// - Pattern cannot contain newlines (each pattern should be on its own line)
/// - Brackets must be properly balanced (e.g., `[ab]` is valid, `[ab` is not)
///
/// # Arguments
/// * `pattern` - The pattern string to validate
///
/// # Returns
/// * `Ok(())` if the pattern is valid
/// * `Err(message)` with a helpful error message if invalid
///
/// # Examples
/// ```
/// use stui::logic::ignore::validate_pattern;
///
/// // Valid patterns
/// assert!(validate_pattern("*.jpg").is_ok());
/// assert!(validate_pattern("temp/[ab].txt").is_ok());
/// assert!(validate_pattern("/cache/**/*.tmp").is_ok());
///
/// // Invalid patterns
/// assert!(validate_pattern("").is_err());
/// assert!(validate_pattern("  ").is_err());
/// assert!(validate_pattern("test[.txt").is_err());
/// assert!(validate_pattern("line1\nline2").is_err());
/// ```
///
/// Note: Currently unused but available for pattern input validation dialogs
#[allow(dead_code)]
pub fn validate_pattern(pattern: &str) -> Result<(), String> {
    // Check for empty pattern
    if pattern.trim().is_empty() {
        return Err("Pattern cannot be empty".to_string());
    }

    // Check for newlines (patterns should be single-line)
    if pattern.contains('\n') {
        return Err("Pattern cannot contain newlines".to_string());
    }

    // Check for balanced brackets (common glob syntax error)
    let open_brackets = pattern.matches('[').count();
    let close_brackets = pattern.matches(']').count();
    if open_brackets != close_brackets {
        return Err("Unclosed bracket in pattern".to_string());
    }

    // Check for balanced braces (used in glob patterns like {a,b})
    let open_braces = pattern.matches('{').count();
    let close_braces = pattern.matches('}').count();
    if open_braces != close_braces {
        return Err("Unclosed brace in pattern".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================
    // PATTERN MATCHING
    // ========================================

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

    // ========================================
    // FIND MATCHING PATTERNS
    // ========================================

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

    // ========================================
    // PATTERN VALIDATION - VALID
    // ========================================

    #[test]
    fn test_validate_pattern_valid() {
        // Valid simple patterns
        assert!(validate_pattern("*.jpg").is_ok());
        assert!(validate_pattern("*.tmp").is_ok());
        assert!(validate_pattern("cache").is_ok());

        // Valid patterns with path separators
        assert!(validate_pattern("/cache/data").is_ok());
        assert!(validate_pattern("temp/data/*.log").is_ok());

        // Valid patterns with brackets
        assert!(validate_pattern("temp/[ab].txt").is_ok());
        assert!(validate_pattern("file[0-9].dat").is_ok());

        // Valid patterns with braces
        assert!(validate_pattern("*.{jpg,png,gif}").is_ok());
        assert!(validate_pattern("{cache,temp}/**").is_ok());

        // Valid patterns with wildcards
        assert!(validate_pattern("/cache/**/*.tmp").is_ok());
        assert!(validate_pattern("**/node_modules/**").is_ok());
    }

    // ========================================
    // PATTERN VALIDATION - INVALID
    // ========================================

    #[test]
    fn test_validate_pattern_empty() {
        assert!(validate_pattern("").is_err());
        assert!(validate_pattern("  ").is_err());
        assert!(validate_pattern("\t").is_err());
        assert!(validate_pattern("   \t  ").is_err());

        if let Err(msg) = validate_pattern("") {
            assert!(msg.contains("empty"));
        }
    }

    #[test]
    fn test_validate_pattern_newlines() {
        assert!(validate_pattern("line1\nline2").is_err());
        assert!(validate_pattern("pattern\n").is_err());
        assert!(validate_pattern("\npattern").is_err());

        if let Err(msg) = validate_pattern("test\npattern") {
            assert!(msg.contains("newline"));
        }
    }

    #[test]
    fn test_validate_pattern_unbalanced_brackets() {
        assert!(validate_pattern("test[.txt").is_err());
        assert!(validate_pattern("test].txt").is_err());
        assert!(validate_pattern("test[ab][cd.txt").is_err());

        if let Err(msg) = validate_pattern("test[.txt") {
            assert!(msg.contains("bracket"));
        }
    }

    #[test]
    fn test_validate_pattern_unbalanced_braces() {
        assert!(validate_pattern("test{.txt").is_err());
        assert!(validate_pattern("test}.txt").is_err());
        assert!(validate_pattern("*.{jpg,png").is_err());

        if let Err(msg) = validate_pattern("*.{jpg,png") {
            assert!(msg.contains("brace"));
        }
    }

    // ========================================
    // PATTERN VALIDATION - EDGE CASES
    // ========================================

    #[test]
    fn test_validate_pattern_edge_cases() {
        // Single character patterns are valid
        assert!(validate_pattern("*").is_ok());
        assert!(validate_pattern("?").is_ok());
        assert!(validate_pattern("/").is_ok());

        // Patterns with special characters
        assert!(validate_pattern("file-name.txt").is_ok());
        assert!(validate_pattern("file_name.txt").is_ok());
        assert!(validate_pattern("file.name.txt").is_ok());
    }
}
