//! Path Mapping Utilities
//!
//! Handles translation between container paths (inside Docker) and host paths.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Translate a container path to a host path using the path mapping configuration
///
/// This is needed because Syncthing runs in Docker with different paths than the host.
///
/// # Arguments
/// * `folder_path` - The folder's base path in the container
/// * `relative_path` - The relative path within the folder
/// * `path_map` - Mapping from container path prefixes to host path prefixes
///
/// # Returns
/// The translated host path, or the original container path if no mapping matches
///
/// # Example
/// ```ignore
/// let path_map = HashMap::from([
///     ("/data".to_string(), "/mnt/storage".to_string()),
/// ]);
/// let result = translate_path("/data/media", "movies/video.mp4", &path_map);
/// assert_eq!(result, "/mnt/storage/media/movies/video.mp4");
/// ```
pub fn translate_path(
    folder_path: &str,
    relative_path: &str,
    path_map: &HashMap<String, String>,
) -> String {
    // Get the full container path
    let container_path = format!(
        "{}/{}",
        folder_path.trim_end_matches('/'),
        relative_path
    );

    // Try to map container path to host path using path_map
    for (container_prefix, host_prefix) in path_map {
        let normalized_prefix = container_prefix.trim_end_matches('/');
        if container_path.starts_with(normalized_prefix) {
            let remainder = container_path
                .strip_prefix(normalized_prefix)
                .unwrap_or("");
            return format!("{}{}", host_prefix.trim_end_matches('/'), remainder);
        }
    }

    // If no mapping found, return container path
    container_path
}

/// Check if a path or any of its parent directories are in the given set
///
/// This is used to validate whether a file operation is blocked by a pending operation
/// on the path itself or any of its parent directories.
///
/// # Arguments
/// * `pending_paths` - Set of paths that are currently pending (e.g., pending deletion)
/// * `path` - The path to check
///
/// # Returns
/// * `Some(PathBuf)` - The pending path that blocks this operation (either exact match or parent)
/// * `None` - No blocking path found
///
/// # Example
/// ```
/// use std::collections::HashSet;
/// use std::path::PathBuf;
/// use synctui::logic::path::is_path_or_parent_in_set;
///
/// let mut pending = HashSet::new();
/// pending.insert(PathBuf::from("/foo/bar"));
///
/// // Exact match
/// assert!(is_path_or_parent_in_set(&pending, &PathBuf::from("/foo/bar")).is_some());
///
/// // Child path is blocked by parent
/// assert!(is_path_or_parent_in_set(&pending, &PathBuf::from("/foo/bar/baz")).is_some());
///
/// // Unrelated path
/// assert!(is_path_or_parent_in_set(&pending, &PathBuf::from("/other")).is_none());
/// ```
pub fn is_path_or_parent_in_set(pending_paths: &HashSet<PathBuf>, path: &PathBuf) -> Option<PathBuf> {
    // First check for exact match
    if pending_paths.contains(path) {
        return Some(path.clone());
    }

    // Check if any parent directory is pending
    // For example, if "/foo/bar" is pending, block "/foo/bar/baz"
    for pending_path in pending_paths {
        if path.starts_with(pending_path) {
            return Some(pending_path.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_path_with_mapping() {
        let path_map = HashMap::from([("/data".to_string(), "/mnt/storage".to_string())]);

        let result = translate_path("/data/media", "movies/video.mp4", &path_map);
        assert_eq!(result, "/mnt/storage/media/movies/video.mp4");
    }

    #[test]
    fn test_translate_path_no_mapping() {
        let path_map = HashMap::new();

        let result = translate_path("/data/media", "movies/video.mp4", &path_map);
        assert_eq!(result, "/data/media/movies/video.mp4");
    }

    #[test]
    fn test_translate_path_multiple_mappings() {
        let path_map = HashMap::from([
            ("/data".to_string(), "/mnt/storage".to_string()),
            ("/config".to_string(), "/etc/app".to_string()),
        ]);

        let result1 = translate_path("/data/files", "test.txt", &path_map);
        assert_eq!(result1, "/mnt/storage/files/test.txt");

        let result2 = translate_path("/config/app", "settings.yaml", &path_map);
        assert_eq!(result2, "/etc/app/app/settings.yaml");
    }

    #[test]
    fn test_translate_path_trailing_slashes() {
        let path_map = HashMap::from([("/data/".to_string(), "/mnt/storage/".to_string())]);

        let result = translate_path("/data/media/", "file.txt", &path_map);
        assert_eq!(result, "/mnt/storage/media/file.txt");
    }

    #[test]
    fn test_translate_path_no_relative() {
        let path_map = HashMap::from([("/data".to_string(), "/mnt/storage".to_string())]);

        let result = translate_path("/data/media", "", &path_map);
        assert_eq!(result, "/mnt/storage/media/");
    }

    // Tests for is_path_or_parent_in_set (TDD - written before implementation)

    #[test]
    fn test_is_path_or_parent_in_set_exact_match() {
        use std::collections::HashSet;
        use std::path::PathBuf;

        let mut pending_paths = HashSet::new();
        pending_paths.insert(PathBuf::from("/foo/bar"));
        pending_paths.insert(PathBuf::from("/baz/qux"));

        let path = PathBuf::from("/foo/bar");
        let result = is_path_or_parent_in_set(&pending_paths, &path);

        assert_eq!(result, Some(PathBuf::from("/foo/bar")),
            "Should find exact match in pending set");
    }

    #[test]
    fn test_is_path_or_parent_in_set_parent_match() {
        use std::collections::HashSet;
        use std::path::PathBuf;

        let mut pending_paths = HashSet::new();
        pending_paths.insert(PathBuf::from("/foo/bar"));

        // Check if child path is blocked by pending parent
        let child_path = PathBuf::from("/foo/bar/baz/file.txt");
        let result = is_path_or_parent_in_set(&pending_paths, &child_path);

        assert_eq!(result, Some(PathBuf::from("/foo/bar")),
            "Should find parent directory in pending set");
    }

    #[test]
    fn test_is_path_or_parent_in_set_no_match() {
        use std::collections::HashSet;
        use std::path::PathBuf;

        let mut pending_paths = HashSet::new();
        pending_paths.insert(PathBuf::from("/foo/bar"));
        pending_paths.insert(PathBuf::from("/baz/qux"));

        let unrelated_path = PathBuf::from("/completely/different/path");
        let result = is_path_or_parent_in_set(&pending_paths, &unrelated_path);

        assert_eq!(result, None,
            "Should return None when path has no match or parent match");
    }

    #[test]
    fn test_is_path_or_parent_in_set_empty_set() {
        use std::collections::HashSet;
        use std::path::PathBuf;

        let pending_paths: HashSet<PathBuf> = HashSet::new();
        let path = PathBuf::from("/any/path");
        let result = is_path_or_parent_in_set(&pending_paths, &path);

        assert_eq!(result, None,
            "Should return None when pending set is empty");
    }

    #[test]
    fn test_is_path_or_parent_in_set_multiple_pending() {
        use std::collections::HashSet;
        use std::path::PathBuf;

        let mut pending_paths = HashSet::new();
        pending_paths.insert(PathBuf::from("/foo"));
        pending_paths.insert(PathBuf::from("/foo/bar"));
        pending_paths.insert(PathBuf::from("/baz"));

        // Path with multiple ancestors in pending set - should return one of them
        let deep_path = PathBuf::from("/foo/bar/baz/qux");
        let result = is_path_or_parent_in_set(&pending_paths, &deep_path);

        assert!(result.is_some(), "Should find at least one matching parent");
        // Should match either /foo or /foo/bar (both are parents)
        let matched = result.unwrap();
        assert!(matched == PathBuf::from("/foo") || matched == PathBuf::from("/foo/bar"),
            "Should match one of the parent paths");
    }
}
