//! File type detection and utilities
//!
//! Pure functions for identifying file types and properties.

/// Check if a file path represents an image file based on extension
///
/// Supported formats: PNG, JPG/JPEG, GIF, BMP, WEBP, TIFF/TIF
///
/// # Arguments
/// * `path` - File path or name to check
///
/// # Returns
/// `true` if the file has an image extension, `false` otherwise
///
/// # Examples
/// ```
/// use synctui::logic::file::is_image_file;
///
/// assert!(is_image_file("photo.jpg"));
/// assert!(is_image_file("image.PNG"));  // Case insensitive
/// assert!(is_image_file("/path/to/pic.webp"));
/// assert!(!is_image_file("document.pdf"));
/// assert!(!is_image_file("file.txt"));
/// ```
pub fn is_image_file(path: &str) -> bool {
    let path_lower = path.to_lowercase();
    path_lower.ends_with(".png")
        || path_lower.ends_with(".jpg")
        || path_lower.ends_with(".jpeg")
        || path_lower.ends_with(".gif")
        || path_lower.ends_with(".bmp")
        || path_lower.ends_with(".webp")
        || path_lower.ends_with(".tiff")
        || path_lower.ends_with(".tif")
}

/// Check if file content is binary based on presence of null bytes
///
/// A file is considered binary if it contains any null bytes (0x00) in the
/// first portion of the content. This is a heuristic similar to how Unix
/// tools like `file` detect binary content.
///
/// # Arguments
/// * `bytes` - File content to check (typically first 8KB is sufficient)
///
/// # Returns
/// `true` if the content contains null bytes (binary), `false` otherwise
///
/// # Examples
/// ```
/// use synctui::logic::file::is_binary_content;
///
/// // Text content - no null bytes
/// assert!(!is_binary_content(b"Hello, world!"));
/// assert!(!is_binary_content(b"UTF-8: \xE2\x9C\x93"));
///
/// // Binary content - contains null bytes
/// assert!(is_binary_content(b"Hello\x00World"));
/// assert!(is_binary_content(b"\x00\x01\x02\x03"));
/// ```
pub fn is_binary_content(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_file_png() {
        assert!(is_image_file("photo.png"));
        assert!(is_image_file("photo.PNG"));
        assert!(is_image_file("/path/to/photo.png"));
    }

    #[test]
    fn test_is_image_file_jpg() {
        assert!(is_image_file("photo.jpg"));
        assert!(is_image_file("photo.JPG"));
        assert!(is_image_file("photo.jpeg"));
        assert!(is_image_file("photo.JPEG"));
    }

    #[test]
    fn test_is_image_file_other_formats() {
        assert!(is_image_file("image.gif"));
        assert!(is_image_file("image.bmp"));
        assert!(is_image_file("image.webp"));
        assert!(is_image_file("image.tiff"));
        assert!(is_image_file("image.tif"));
    }

    #[test]
    fn test_is_image_file_case_insensitive() {
        assert!(is_image_file("PHOTO.PNG"));
        assert!(is_image_file("Photo.Jpg"));
        assert!(is_image_file("IMAGE.WebP"));
    }

    #[test]
    fn test_is_image_file_non_images() {
        assert!(!is_image_file("document.pdf"));
        assert!(!is_image_file("file.txt"));
        assert!(!is_image_file("video.mp4"));
        assert!(!is_image_file("archive.zip"));
        assert!(!is_image_file("no_extension"));
    }

    #[test]
    fn test_is_image_file_edge_cases() {
        assert!(!is_image_file(""));
        assert!(is_image_file(".png"));  // Just extension - technically valid image file
        assert!(is_image_file("a.png"));  // Single character filename
        assert!(!is_image_file("png"));  // No extension dot
    }

    #[test]
    fn test_is_binary_content_text() {
        // Plain ASCII text
        assert!(!is_binary_content(b"Hello, world!"));
        assert!(!is_binary_content(b"Line 1\nLine 2\n"));
        assert!(!is_binary_content(b""));  // Empty file is text
    }

    #[test]
    fn test_is_binary_content_utf8() {
        // UTF-8 encoded text (no null bytes)
        assert!(!is_binary_content("UTF-8: ✓".as_bytes()));
        assert!(!is_binary_content("日本語".as_bytes()));
        assert!(!is_binary_content("Emoji: 🦀".as_bytes()));
    }

    #[test]
    fn test_is_binary_content_binary() {
        // Content with null bytes
        assert!(is_binary_content(b"Hello\x00World"));
        assert!(is_binary_content(b"\x00\x01\x02\x03"));
        assert!(is_binary_content(b"\x00"));  // Just a null byte
    }

    #[test]
    fn test_is_binary_content_null_at_start() {
        // Null byte at beginning
        assert!(is_binary_content(b"\x00text after null"));
    }

    #[test]
    fn test_is_binary_content_null_at_end() {
        // Null byte at end
        assert!(is_binary_content(b"text before null\x00"));
    }
}
