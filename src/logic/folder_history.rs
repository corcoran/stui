//! Folder update history processing logic
//!
//! Pure functions for processing file lists into folder history entries.
//! Handles sorting by modification time and limiting results.

use crate::model::types::FolderHistoryEntry;
use std::time::SystemTime;

/// Process file list into folder history entries
///
/// Takes a flat list of files with metadata, sorts by modification time (newest first),
/// and limits to specified count.
///
/// **NOTE**: This function is deprecated in favor of sorting files once and using
/// `extract_batch_from_sorted()` for pagination. Kept for backwards compatibility
/// and existing tests.
///
/// # Arguments
/// * `files` - List of (path, mod_time, size) tuples from recursive browse
/// * `limit` - Maximum number of files to return
///
/// # Returns
/// Vec of FolderHistoryEntry, sorted newest-first, limited to `limit` entries
#[cfg(test)]
pub fn process_files_for_history(
    files: Vec<(String, SystemTime, u64)>,
    limit: usize,
) -> Vec<FolderHistoryEntry> {
    let mut entries: Vec<FolderHistoryEntry> = files
        .into_iter()
        .map(|(path, timestamp, size)| FolderHistoryEntry {
            timestamp,
            event_type: "Modified".to_string(), // Not from events, use generic label
            file_path: path,
            file_size: Some(size),
        })
        .collect();

    // Sort by modification time (newest first)
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Limit to requested count
    entries.truncate(limit);

    entries
}

/// Extract a batch of files from a pre-sorted file list
///
/// Returns a slice of files starting at `offset` with up to `limit` entries.
/// Used for pagination - assumes files are already sorted newest-first.
///
/// # Arguments
/// * `sorted_files` - Pre-sorted list of (path, mod_time, size) tuples (newest first)
/// * `offset` - Starting index in the sorted list
/// * `limit` - Maximum number of files to return
///
/// # Returns
/// Vec of FolderHistoryEntry extracted from the sorted list, maintaining sort order
pub fn extract_batch_from_sorted(
    sorted_files: &[(String, SystemTime, u64)],
    offset: usize,
    limit: usize,
) -> Vec<FolderHistoryEntry> {
    sorted_files
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(path, timestamp, size)| FolderHistoryEntry {
            timestamp: *timestamp,
            event_type: "Modified".to_string(),
            file_path: path.clone(),
            file_size: Some(*size),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    // ========================================
    // FILE HISTORY PROCESSING
    // ========================================

    #[test]
    fn test_process_empty_files() {
        let files = vec![];
        let result = process_files_for_history(files, 100);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_process_flat_files() {
        let files = vec![
            ("file1.txt".to_string(), unix_time(1000), 100),
            ("file2.txt".to_string(), unix_time(2000), 200),
            ("file3.txt".to_string(), unix_time(3000), 300),
        ];

        let result = process_files_for_history(files, 100);

        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|e| e.file_path == "file1.txt"));
        assert!(result.iter().any(|e| e.file_path == "file2.txt"));
        assert!(result.iter().any(|e| e.file_path == "file3.txt"));
    }

    #[test]
    fn test_sort_newest_first() {
        let files = vec![
            ("old.txt".to_string(), unix_time(1000), 100),
            ("new.txt".to_string(), unix_time(3000), 300),
            ("medium.txt".to_string(), unix_time(2000), 200),
        ];

        let result = process_files_for_history(files, 100);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].file_path, "new.txt", "Newest should be first");
        assert_eq!(result[1].file_path, "medium.txt", "Medium should be second");
        assert_eq!(result[2].file_path, "old.txt", "Oldest should be last");
    }

    #[test]
    fn test_limit_to_100() {
        let mut files = vec![];
        for i in 0..150 {
            files.push((format!("file{}.txt", i), unix_time(i), 100));
        }

        let result = process_files_for_history(files, 100);

        assert_eq!(result.len(), 100, "Should limit to 100 files");
    }

    #[test]
    fn test_limit_less_than_100() {
        let files = vec![
            ("file1.txt".to_string(), unix_time(1000), 100),
            ("file2.txt".to_string(), unix_time(2000), 200),
        ];

        let result = process_files_for_history(files, 100);

        assert_eq!(
            result.len(),
            2,
            "Should return all files when less than limit"
        );
    }

    #[test]
    fn test_file_size_included() {
        let files = vec![("file.txt".to_string(), unix_time(1000), 2048)];

        let result = process_files_for_history(files, 100);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_size, Some(2048));
    }

    #[test]
    fn test_nested_paths() {
        let files = vec![
            ("root.txt".to_string(), unix_time(1000), 100),
            ("subdir/nested1.txt".to_string(), unix_time(2000), 200),
            (
                "level1/level2/level3/deep.txt".to_string(),
                unix_time(3000),
                300,
            ),
        ];

        let result = process_files_for_history(files, 100);

        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|e| e.file_path == "root.txt"));
        assert!(result.iter().any(|e| e.file_path == "subdir/nested1.txt"));
        assert!(result
            .iter()
            .any(|e| e.file_path == "level1/level2/level3/deep.txt"));
    }

    // ========================================
    // HELPER FUNCTIONS
    // ========================================

    fn unix_time(secs: u64) -> SystemTime {
        UNIX_EPOCH + std::time::Duration::from_secs(secs)
    }

    // ========================================
    // BATCHED FILE EXTRACTION
    // ========================================

    #[test]
    fn test_extract_batch_with_offset_and_limit() {
        // Create sorted file list (newest first)
        let files = vec![
            ("file9.txt".to_string(), unix_time(9000), 900),
            ("file8.txt".to_string(), unix_time(8000), 800),
            ("file7.txt".to_string(), unix_time(7000), 700),
            ("file6.txt".to_string(), unix_time(6000), 600),
            ("file5.txt".to_string(), unix_time(5000), 500),
            ("file4.txt".to_string(), unix_time(4000), 400),
            ("file3.txt".to_string(), unix_time(3000), 300),
            ("file2.txt".to_string(), unix_time(2000), 200),
            ("file1.txt".to_string(), unix_time(1000), 100),
        ];

        // Extract second batch (offset=3, limit=3)
        let result = extract_batch_from_sorted(&files, 3, 3);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].file_path, "file6.txt");
        assert_eq!(result[1].file_path, "file5.txt");
        assert_eq!(result[2].file_path, "file4.txt");
    }

    #[test]
    fn test_extract_batch_at_end() {
        let files = vec![
            ("file3.txt".to_string(), unix_time(3000), 300),
            ("file2.txt".to_string(), unix_time(2000), 200),
            ("file1.txt".to_string(), unix_time(1000), 100),
        ];

        // Try to extract beyond available files
        let result = extract_batch_from_sorted(&files, 2, 10);

        assert_eq!(result.len(), 1); // Only 1 file left after offset 2
        assert_eq!(result[0].file_path, "file1.txt");
    }

    #[test]
    fn test_extract_batch_offset_beyond_end() {
        let files = vec![("file1.txt".to_string(), unix_time(1000), 100)];

        // Offset beyond available files
        let result = extract_batch_from_sorted(&files, 10, 10);

        assert_eq!(result.len(), 0); // No files available
    }

    #[test]
    fn test_extract_batch_preserves_sort_order() {
        let files = vec![
            ("newest.txt".to_string(), unix_time(5000), 500),
            ("newer.txt".to_string(), unix_time(4000), 400),
            ("old.txt".to_string(), unix_time(3000), 300),
            ("older.txt".to_string(), unix_time(2000), 200),
            ("oldest.txt".to_string(), unix_time(1000), 100),
        ];

        // Extract middle batch
        let result = extract_batch_from_sorted(&files, 1, 3);

        assert_eq!(result.len(), 3);
        // Should maintain newest-first order
        assert_eq!(result[0].file_path, "newer.txt");
        assert_eq!(result[1].file_path, "old.txt");
        assert_eq!(result[2].file_path, "older.txt");
        assert!(result[0].timestamp > result[1].timestamp);
        assert!(result[1].timestamp > result[2].timestamp);
    }
}
