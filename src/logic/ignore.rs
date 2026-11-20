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
        if let Ok(pattern_obj) = glob::Pattern::new(pattern_without_slash)
            && pattern_obj.matches(file_path.trim_start_matches('/'))
        {
            return true;
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

/// One positive ignore pattern, compiled for `may_be_ignored`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompiledIgnore {
    rooted: bool,
    glob: glob::Pattern,
}

/// Ignore patterns compiled once per fetch for `may_be_ignored`
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IgnoreMatcher {
    Patterns(Vec<CompiledIgnore>),
    /// Syntax we don't model; every path counts as possibly ignored
    MatchAll,
}

/// Compile .stignore lines (preferably the `expanded` list) for `may_be_ignored`.
///
/// Negations are dropped and prefixes stripped. Unsupported syntax (`#include`, `#escape`,
/// `{a,b}`, backslash escapes, invalid globs) yields `MatchAll`.
pub fn compile_ignore_patterns(patterns: &[String]) -> IgnoreMatcher {
    let mut compiled = Vec::new();
    for line in patterns {
        let mut pattern = line.trim();
        if pattern.is_empty() || pattern.starts_with("//") {
            continue;
        }
        if pattern.starts_with("#include")
            || pattern.starts_with("#escape")
            || pattern.contains('{')
            || pattern.contains('\\')
        {
            return IgnoreMatcher::MatchAll;
        }
        loop {
            let stripped = pattern
                .strip_prefix("(?d)")
                .or_else(|| pattern.strip_prefix("(?i)"));
            match stripped {
                Some(rest) => pattern = rest,
                None => break,
            }
        }
        if pattern.starts_with('!') {
            continue;
        }
        let body = pattern.trim_matches('/').to_lowercase();
        if body.is_empty() {
            return IgnoreMatcher::MatchAll;
        }
        let Ok(glob) = glob::Pattern::new(&body) else {
            return IgnoreMatcher::MatchAll;
        };
        compiled.push(CompiledIgnore {
            rooted: pattern.starts_with('/'),
            glob,
        });
    }
    IgnoreMatcher::Patterns(compiled)
}

/// Whether any pattern could ignore `path` (folder-relative, no leading slash).
///
/// Over-matches on purpose: false must mean "certainly not ignored". Negations are skipped,
/// matching is case-insensitive, ancestors are checked, unrooted patterns are tried at every
/// depth, and unsupported syntax makes every path match.
pub fn may_be_ignored(matcher: &IgnoreMatcher, path: &str) -> bool {
    let patterns = match matcher {
        IgnoreMatcher::MatchAll => return true,
        IgnoreMatcher::Patterns(patterns) if patterns.is_empty() => return false,
        IgnoreMatcher::Patterns(patterns) => patterns,
    };
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let path = path.trim_matches('/').to_lowercase();
    let components: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();

    // Every ancestor (and the path itself), as a slice of components
    let mut candidates: Vec<(usize, String)> = Vec::new();
    for start in 0..components.len() {
        for end in (start + 1)..=components.len() {
            candidates.push((start, components[start..end].join("/")));
        }
    }
    patterns.iter().any(|p| {
        candidates
            .iter()
            .filter(|(start, _)| !p.rooted || *start == 0)
            .any(|(_, candidate)| p.glob.matches_with(candidate, options))
    })
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

/// Which part of a folder to scan after an ignore change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanScope {
    /// Folder-relative paths, no leading or trailing slash.
    Paths(Vec<String>),
    Full,
}

impl ScanScope {
    /// Paths for `SyncthingClient::rescan_paths`; empty means a full scan.
    pub fn subs(&self) -> &[String] {
        match self {
            ScanScope::Paths(paths) => paths,
            ScanScope::Full => &[],
        }
    }
}

/// Scope of the scan needed after adding or removing `pattern` for the item at `item_path`
/// (folder-relative).
///
/// Only a rooted literal path (leading `/`) that is the item or one of its ancestors is
/// scoped. Everything else gets a full scan: wildcards, `**`, escapes, unrooted patterns
/// (they match at any depth), and `(?i)`/`(?d)`/`!`/`#` lines, which never start with `/`.
pub fn scan_scope_for_pattern(pattern: &str, item_path: &str) -> ScanScope {
    let pattern = pattern.trim();
    if !pattern.starts_with('/') || pattern.contains(['*', '?', '[', '{', '\\']) {
        return ScanScope::Full;
    }

    let target = pattern.trim_matches('/');
    let item = item_path.trim_matches('/');
    if target.is_empty() || item.is_empty() {
        return ScanScope::Full;
    }

    let covers = item == target
        || item
            .strip_prefix(target)
            .is_some_and(|rest| rest.starts_with('/'));
    if covers {
        ScanScope::Paths(vec![target.to_string()])
    } else {
        ScanScope::Full
    }
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

    // ========================================
    // MAY BE IGNORED (conservative)
    // ========================================

    fn pats(list: &[&str]) -> IgnoreMatcher {
        let lines: Vec<String> = list.iter().map(|p| p.to_string()).collect();
        compile_ignore_patterns(&lines)
    }

    #[test]
    fn test_may_be_ignored_no_patterns() {
        assert!(!may_be_ignored(&pats(&[]), "a/b.txt"));
        assert!(!may_be_ignored(&pats(&["", "// comment"]), "a/b.txt"));
    }

    #[test]
    fn test_may_be_ignored_rooted_dir_covers_children() {
        let p = pats(&["/website"]);
        assert!(may_be_ignored(&p, "website"));
        assert!(may_be_ignored(&p, "website/build.log"));
        assert!(!may_be_ignored(&p, "websites"));
        assert!(!may_be_ignored(&p, "other/website"));
    }

    #[test]
    fn test_may_be_ignored_unrooted_matches_at_any_depth() {
        let p = pats(&["scratch.txt", "build/out"]);
        assert!(may_be_ignored(&p, "notes/scratch.txt"));
        assert!(may_be_ignored(&p, "x/build/out/y.o"));
        assert!(!may_be_ignored(&p, "notes"));
        assert!(!may_be_ignored(&p, "notes/todo.txt"));
    }

    #[test]
    fn test_may_be_ignored_globs_and_prefixes() {
        let p = pats(&["(?d)(?i)*.LOG", "!keep.log"]);
        assert!(may_be_ignored(&p, "a/debug.log"));
        // Negations are skipped, so a re-included file still counts as possibly ignored
        assert!(may_be_ignored(&p, "keep.log"));
        assert!(!may_be_ignored(&p, "a/readme.md"));
        assert!(may_be_ignored(&pats(&["/cache/**"]), "cache/x/y"));
        assert!(may_be_ignored(&pats(&["/trailing/"]), "trailing/x"));
    }

    #[test]
    fn test_may_be_ignored_unsupported_syntax_matches_everything() {
        assert!(may_be_ignored(&pats(&["#include other"]), "a.txt"));
        assert!(may_be_ignored(&pats(&["*.{jpg,png}"]), "a.txt"));
        assert!(may_be_ignored(&pats(&["a**b"]), "a.txt"));
    }

    #[test]
    fn test_may_be_ignored_escapes_match_everything() {
        assert!(may_be_ignored(&pats(&["foo\\*bar"]), "a.txt"));
        assert!(may_be_ignored(&pats(&["#escape=|", "x"]), "a.txt"));
    }

    // ========================================
    // SCAN SCOPE
    // ========================================

    fn paths(p: &[&str]) -> ScanScope {
        ScanScope::Paths(p.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn test_scan_scope_rooted_pattern_uses_relative_path() {
        assert_eq!(
            scan_scope_for_pattern("/notes/a.txt", "notes/a.txt"),
            paths(&["notes/a.txt"])
        );
        assert_eq!(scan_scope_for_pattern("/a.txt", "a.txt"), paths(&["a.txt"]));
    }

    #[test]
    fn test_scan_scope_item_path_with_leading_slash() {
        assert_eq!(
            scan_scope_for_pattern("/notes/a.txt", "/notes/a.txt"),
            paths(&["notes/a.txt"])
        );
    }

    #[test]
    fn test_scan_scope_directory_pattern() {
        assert_eq!(
            scan_scope_for_pattern("/cache/", "cache"),
            paths(&["cache"])
        );
        // Unrooted matches at any depth
        assert_eq!(scan_scope_for_pattern("cache/", "cache"), ScanScope::Full);
    }

    #[test]
    fn test_scan_scope_ancestor_pattern_scans_the_pattern_path() {
        assert_eq!(
            scan_scope_for_pattern("/cache", "cache/deep/x.bin"),
            paths(&["cache"])
        );
    }

    #[test]
    fn test_scan_scope_globs_are_full() {
        for p in ["*.log", "/notes/*", "a?.txt", "[ab].txt", "*.{jpg,png}"] {
            assert_eq!(
                scan_scope_for_pattern(p, "notes/a.txt"),
                ScanScope::Full,
                "{}",
                p
            );
        }
    }

    #[test]
    fn test_scan_scope_double_star_is_full() {
        assert_eq!(scan_scope_for_pattern("**/build", "build"), ScanScope::Full);
        assert_eq!(
            scan_scope_for_pattern("/cache/**", "cache"),
            ScanScope::Full
        );
    }

    #[test]
    fn test_scan_scope_prefixes_are_full() {
        let prefixed = [
            "(?i)Foo",
            "(?d)Foo",
            "(?d)(?i)Foo",
            "(?i)/Foo",
            "!keep",
            "!/Foo",
            "#/Foo",
        ];
        for p in prefixed {
            assert_eq!(scan_scope_for_pattern(p, "Foo"), ScanScope::Full, "{}", p);
        }
    }

    #[test]
    fn test_scan_scope_pattern_for_other_path_is_full() {
        assert_eq!(
            scan_scope_for_pattern("/other.txt", "a.txt"),
            ScanScope::Full
        );
        assert_eq!(
            scan_scope_for_pattern("/cache", "cache2/x"),
            ScanScope::Full
        );
    }

    #[test]
    fn test_scan_scope_unrooted_literal_is_full() {
        assert_eq!(scan_scope_for_pattern("build", "build"), ScanScope::Full);
        assert_eq!(
            scan_scope_for_pattern("notes/a.txt", "notes/a.txt"),
            ScanScope::Full
        );
    }

    #[test]
    fn test_scan_scope_degenerate_patterns_are_full() {
        assert_eq!(scan_scope_for_pattern("", "a.txt"), ScanScope::Full);
        assert_eq!(scan_scope_for_pattern("/", "a.txt"), ScanScope::Full);
        assert_eq!(
            scan_scope_for_pattern("#include x", "a.txt"),
            ScanScope::Full
        );
        assert_eq!(scan_scope_for_pattern("/a.txt", ""), ScanScope::Full);
    }

    #[test]
    fn test_scan_scope_escapes_are_full() {
        assert_eq!(
            scan_scope_for_pattern("/foo\\*bar", "foo*bar"),
            ScanScope::Full
        );
    }

    #[test]
    fn test_scan_scope_subs() {
        assert!(ScanScope::Full.subs().is_empty());
        assert_eq!(
            paths(&["a", "b"]).subs(),
            ["a".to_string(), "b".to_string()]
        );
    }
}
