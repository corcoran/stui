//! Path Mapping Utilities
//!
//! Handles translation between container paths (inside Docker) and host paths.

use std::collections::HashMap;

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
}
