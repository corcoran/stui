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
}
