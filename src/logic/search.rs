//! Search Logic
//!
//! Pure functions for filtering files and directories by search queries.
//! Supports wildcard patterns using the glob crate.

use crate::api::BrowseItem;

/// Match a search query against a file path using wildcard patterns
///
/// # Pattern Rules
/// - "*" matches any sequence of characters within a path component
/// - Matches are case-insensitive
/// - Matches file name or any part of the path
///
/// # Examples
/// ```
/// use synctui::logic::search::search_matches;
///
/// assert!(search_matches("jeff", "jeff-1.txt"));
/// assert!(search_matches("*jeff*", "my-jeff-file.txt"));
/// assert!(search_matches("photos", "/Movies/Photos/jeff-1.txt"));
/// ```
pub fn search_matches(query: &str, file_path: &str) -> bool {
    if query.is_empty() {
        return true; // Empty query matches everything
    }

    let query_lower = query.to_lowercase();
    let path_lower = file_path.to_lowercase();

    // Try glob pattern matching first
    if let Ok(pattern) = glob::Pattern::new(&query_lower) {
        // Match against full path
        if pattern.matches(&path_lower) {
            return true;
        }

        // Match against each path component
        for component in path_lower.split('/') {
            if pattern.matches(component) {
                return true;
            }
        }
    }

    // Fallback: simple substring match (if glob pattern is invalid)
    path_lower.contains(&query_lower)
}

/// Filter a list of BrowseItems by search query
///
/// # Arguments
/// - `items`: List of items to filter
/// - `query`: Search query with optional wildcards
/// - `prefix`: Optional path prefix for building full paths
///
/// # Returns
/// Filtered list containing only matching items
pub fn filter_items(
    items: &[BrowseItem],
    query: &str,
    prefix: Option<&str>,
) -> Vec<BrowseItem> {
    if query.is_empty() {
        return items.to_vec();
    }

    items
        .iter()
        .filter(|item| {
            let full_path = match prefix {
                Some(p) => format!("{}/{}", p.trim_matches('/'), item.name),
                None => item.name.clone(),
            };
            search_matches(query, &full_path)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_matches_all() {
        assert!(search_matches("", "any-file.txt"));
    }

    #[test]
    fn test_exact_match() {
        assert!(search_matches("jeff", "jeff-1.txt"));
        assert!(!search_matches("jeff", "john.txt"));
    }

    #[test]
    fn test_wildcard_prefix() {
        assert!(search_matches("jeff*", "jeff-1.txt"));
        assert!(search_matches("jeff*", "jeff.txt"));
        assert!(!search_matches("jeff*", "my-jeff.txt"));
    }

    #[test]
    fn test_wildcard_suffix() {
        assert!(search_matches("*jeff", "my-jeff"));
        assert!(search_matches("*.txt", "file.txt"));
    }

    #[test]
    fn test_wildcard_contains() {
        assert!(search_matches("*jeff*", "my-jeff-file.txt"));
        assert!(search_matches("*jeff*", "jeff.txt"));
        assert!(search_matches("*jeff*", "file-jeff"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(search_matches("JEFF", "jeff-1.txt"));
        assert!(search_matches("jeff", "JEFF-1.TXT"));
        assert!(search_matches("JeFf", "jEfF.txt"));
    }

    #[test]
    fn test_path_component_matching() {
        assert!(search_matches("photos", "/Movies/Photos/jeff-1.txt"));
        assert!(search_matches("movies", "/Movies/Photos/jeff-1.txt"));
        assert!(search_matches("*photos*", "/Movies/Photos/jeff-1.txt"));
    }

    #[test]
    fn test_substring_fallback() {
        // If glob pattern fails, fall back to substring
        assert!(search_matches("jeff", "my-jeff-file.txt"));
    }

    #[test]
    fn test_filter_items_empty_query() {
        let items = vec![
            BrowseItem {
                name: "file1.txt".to_string(),
                item_type: "file".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
            BrowseItem {
                name: "file2.txt".to_string(),
                item_type: "file".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
        ];

        let filtered = filter_items(&items, "", None);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_items_with_query() {
        let items = vec![
            BrowseItem {
                name: "jeff-1.txt".to_string(),
                item_type: "file".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
            BrowseItem {
                name: "john-1.txt".to_string(),
                item_type: "file".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
            BrowseItem {
                name: "jeff-2.txt".to_string(),
                item_type: "file".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
        ];

        let filtered = filter_items(&items, "jeff", None);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].name, "jeff-1.txt");
        assert_eq!(filtered[1].name, "jeff-2.txt");
    }

    #[test]
    fn test_filter_items_with_prefix() {
        let items = vec![
            BrowseItem {
                name: "Photos".to_string(),
                item_type: "directory".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
            BrowseItem {
                name: "Videos".to_string(),
                item_type: "directory".to_string(),
                mod_time: "".to_string(),
                size: 0,
            },
        ];

        let filtered = filter_items(&items, "photos", Some("Movies"));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Photos");
    }


    #[test]
    fn test_wildcard_multiple_stars() {
        assert!(search_matches("*jeff*.txt", "my-jeff-file.txt"));
        assert!(search_matches("*jeff*.txt", "jeff.txt"));
        assert!(!search_matches("*jeff*.txt", "jeff.md"));
    }

    #[test]
    fn test_single_char_name() {
        assert!(search_matches("a", "a"));
        assert!(search_matches("*a*", "a"));
        assert!(search_matches("a*", "abc"));
    }
}
