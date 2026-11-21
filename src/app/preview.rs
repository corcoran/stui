//! File preview methods
//!
//! Methods for displaying file content in popups:
//! - Text file preview with ANSI art support
//! - Image preview with terminal graphics protocols
//! - Binary file text extraction

use crate::{App, BrowseItem, Folder, ImageMetadata, ImagePreviewState, log_debug, logic, model};
use anyhow::Result;
use std::collections::HashMap;

impl App {
    /// Re-reads the previewed file, keeping the scroll position and raw view
    pub(crate) async fn reload_preview(&mut self) {
        let Some(popup) = self.model.ui.file_info_popup.clone() else {
            return;
        };
        self.fetch_file_info_and_content(popup.folder_id, popup.file_path, popup.browse_item)
            .await;
        if let Some(reloaded) = self.model.ui.file_info_popup.as_mut() {
            reloaded.scroll_offset = popup.scroll_offset;
            reloaded.raw_view = popup.raw_view;
        }
    }

    pub(crate) async fn fetch_file_info_and_content(
        &mut self,
        folder_id: String,
        file_path: String,
        browse_item: BrowseItem,
    ) {
        // Find the folder
        let folder = match self
            .model
            .syncthing
            .folders
            .iter()
            .find(|f| f.id == folder_id)
        {
            Some(f) => f.clone(),
            None => {
                self.model.ui.file_info_popup = Some(model::FileInfoPopupState {
                    folder_id,
                    file_path,
                    browse_item,
                    file_details: None,
                    file_content: Err("Folder not found".to_string()),
                    exists_on_disk: false,
                    is_binary: false,
                    is_image: false,
                    scroll_offset: 0,
                    raw_view: false,
                });
                return;
            }
        };

        // Check if file is an image
        let is_image = Self::is_image_file(&file_path);

        // Clear any old image state for this path to prevent rendering artifacts
        self.image_state_map.remove(&file_path);

        // Initialize popup state with loading message first
        self.model.ui.file_info_popup = Some(model::FileInfoPopupState {
            folder_id: folder_id.clone(),
            file_path: file_path.clone(),
            browse_item: browse_item.clone(),
            file_details: None,
            file_content: Err("Loading...".to_string()),
            exists_on_disk: false,
            is_binary: false,
            is_image,
            scroll_offset: 0,
            raw_view: false,
        });

        // 1. Fetch file details from API
        let file_details = self.client.get_file_info(&folder_id, &file_path).await.ok();

        // 2. If image, spawn background loading; otherwise read as text
        let (file_content, exists_on_disk, is_binary, _image_state) = if is_image {
            // Translate path for image loading
            let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), file_path);
            let mut host_path = container_path.clone();
            for (container_prefix, host_prefix) in &self.path_map {
                if let Some(suffix) = container_path.strip_prefix(container_prefix) {
                    host_path = format!("{}{}", host_prefix, suffix);
                    break;
                }
            }

            let host_path_buf = std::path::PathBuf::from(&host_path);
            let exists = tokio::fs::metadata(&host_path_buf).await.is_ok();

            if exists && let Some(picker) = self.image_picker.as_ref() {
                // Spawn background task to load image
                let picker = picker.clone();
                let image_tx = self.image_update_tx.clone();
                let image_file_path = file_path.clone();

                tokio::spawn(async move {
                    log_debug(&format!("Background: Loading image {}", image_file_path));
                    match Self::load_image_preview(host_path_buf, picker).await {
                        Ok((protocol, metadata)) => {
                            log_debug(&format!(
                                "Background: Image loaded successfully {}",
                                image_file_path
                            ));
                            let _ = image_tx.send((
                                image_file_path,
                                ImagePreviewState::Ready {
                                    protocol: Box::new(protocol),
                                    metadata,
                                },
                            ));
                        }
                        Err(metadata) => {
                            log_debug(&format!(
                                "Background: Image load failed {}",
                                image_file_path
                            ));
                            let _ = image_tx
                                .send((image_file_path, ImagePreviewState::Failed { metadata }));
                        }
                    }
                });

                // Return loading state immediately
                (
                    Ok("Loading image preview...".to_string()),
                    true,
                    true,
                    Some(ImagePreviewState::Loading),
                )
            } else if !exists {
                (
                    Err("File not found on disk".to_string()),
                    false,
                    false,
                    Some(ImagePreviewState::Failed {
                        metadata: ImageMetadata {
                            dimensions: None,
                            format: Some("File not found".to_string()),
                            file_size: 0,
                        },
                    }),
                )
            } else {
                (
                    Err("Image preview disabled".to_string()),
                    true,
                    true,
                    Some(ImagePreviewState::Failed {
                        metadata: ImageMetadata {
                            dimensions: None,
                            format: Some("Image preview disabled in config".to_string()),
                            file_size: 0,
                        },
                    }),
                )
            }
        } else {
            // Read as text
            let (content, exists, binary) =
                Self::read_file_content_static(&self.path_map, &folder, &file_path).await;
            (content, exists, binary, None)
        };

        let file_content = if !exists_on_disk
            && logic::display::item_display(&logic::display::ItemInput {
                base: None,
                item: &BrowseItem {
                    name: file_path.clone(),
                    ..browse_item.clone()
                },
                prefix: None,
                syncing: self.model.syncthing.syncing_files.get(&folder_id),
                index: self.model.syncthing.need_index.get(&folder_id),
                ignored_exists: None,
                pending: self
                    .model
                    .pending
                    .get(&folder_id, &file_path)
                    .map(|c| &c.kind),
            }) == Some(logic::display::ItemDisplay::LocallyDeleted)
        {
            Err(logic::sync_states::locally_deleted_message().to_string())
        } else {
            file_content
        };

        // 3. Update popup state with results
        self.model.ui.file_info_popup = Some(model::FileInfoPopupState {
            folder_id,
            file_path,
            browse_item,
            file_details,
            file_content,
            exists_on_disk,
            is_binary,
            // The image pane has nothing to draw for a missing file and would show
            // "Loading image..." forever; the text pane shows why it is missing
            is_image: is_image && exists_on_disk,
            scroll_offset: 0,
            raw_view: false,
        });
    }

    /// Moves the focused level's selection to the adjacent file in display order and
    /// previews it. Returns false, changing nothing, when there is no file that way.
    pub(crate) async fn preview_adjacent_file(&mut self, forward: bool) -> bool {
        let Some(level) = self
            .model
            .navigation
            .focus_level
            .checked_sub(1)
            .and_then(|i| self.model.navigation.breadcrumb_trail.get_mut(i))
        else {
            return false;
        };
        let Some(current) = level.selected_index else {
            return false;
        };
        let Some(next) =
            logic::navigation::adjacent_file_index(level.display_items(), current, forward)
        else {
            return false;
        };

        level.selected_index = Some(next);
        let item = level.display_items()[next].clone();
        let file_path = format!("{}{}", level.prefix.as_deref().unwrap_or(""), item.name);
        let folder_id = level.folder_id.clone();
        self.fetch_file_info_and_content(folder_id, file_path, item)
            .await;
        true
    }

    fn is_image_file(path: &str) -> bool {
        logic::file::is_image_file(path)
    }

    async fn load_image_preview(
        host_path: std::path::PathBuf,
        picker: ratatui_image::picker::Picker,
    ) -> Result<(ratatui_image::protocol::StatefulProtocol, ImageMetadata), ImageMetadata> {
        let max_size_bytes = 20 * 1024 * 1024; // 20MB limit

        // Check file size
        let metadata = match tokio::fs::metadata(&host_path).await {
            Ok(m) => m,
            Err(_e) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: None,
                    file_size: 0,
                });
            }
        };

        let file_size = metadata.len();
        if file_size > max_size_bytes {
            return Err(ImageMetadata {
                dimensions: None,
                format: Some("Too large".to_string()),
                file_size,
            });
        }

        // Load image
        let img_result = tokio::task::spawn_blocking(move || open_upright(&host_path)).await;

        let img = match img_result {
            Ok(Ok(img)) => img,
            Ok(Err(e)) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: Some(format!("Load error: {}", e)),
                    file_size,
                });
            }
            Err(e) => {
                return Err(ImageMetadata {
                    dimensions: None,
                    format: Some(format!("Task error: {}", e)),
                    file_size,
                });
            }
        };

        // Extract metadata (original dimensions)
        let dimensions = (img.width(), img.height());
        let format = match img.color() {
            image::ColorType::L8 => "Grayscale 8-bit",
            image::ColorType::La8 => "Grayscale+Alpha 8-bit",
            image::ColorType::Rgb8 => "RGB 8-bit",
            image::ColorType::Rgba8 => "RGBA 8-bit",
            image::ColorType::L16 => "Grayscale 16-bit",
            image::ColorType::La16 => "Grayscale+Alpha 16-bit",
            image::ColorType::Rgb16 => "RGB 16-bit",
            image::ColorType::Rgba16 => "RGBA 16-bit",
            image::ColorType::Rgb32F => "RGB 32-bit float",
            image::ColorType::Rgba32F => "RGBA 32-bit float",
            _ => "Unknown",
        };

        let load_start = std::time::Instant::now();
        log_debug(&format!(
            "Loading image: {}x{} pixels",
            img.width(),
            img.height()
        ));

        // Pre-downscale large images with adaptive quality/performance balance
        let font_size = picker.font_size();

        // Estimate maximum reasonable size: ~200 cells × ~60 cells (typical large terminal)
        // Use 1.25x headroom for quality (balanced for performance)
        let max_reasonable_width = 200 * font_size.width as u32 * 5 / 4;
        let max_reasonable_height = 60 * font_size.height as u32 * 5 / 4;

        let processed_img =
            if img.width() > max_reasonable_width || img.height() > max_reasonable_height {
                let scale_factor = (img.width() as f32 / max_reasonable_width as f32)
                    .max(img.height() as f32 / max_reasonable_height as f32);

                log_debug(&format!(
                    "Pre-downscaling {}x{} by {:.2}x to fit {}x{} for better quality",
                    img.width(),
                    img.height(),
                    scale_factor,
                    max_reasonable_width,
                    max_reasonable_height
                ));

                // Adaptive filter selection based on downscale amount
                let filter = if scale_factor > 4.0 {
                    // Extreme downscale (>4x): Use Triangle for speed
                    image::imageops::FilterType::Triangle
                } else if scale_factor > 2.0 {
                    // Large downscale (2-4x): Use CatmullRom for balance
                    image::imageops::FilterType::CatmullRom
                } else {
                    // Moderate downscale (<2x): Use Lanczos3 for quality
                    image::imageops::FilterType::Lanczos3
                };

                log_debug(&format!(
                    "Using {:?} filter for {:.2}x downscale",
                    filter, scale_factor
                ));
                let resize_start = std::time::Instant::now();
                let resized = img.resize(max_reasonable_width, max_reasonable_height, filter);
                log_debug(&format!(
                    "Resize took {:.2}s",
                    resize_start.elapsed().as_secs_f32()
                ));
                resized
            } else {
                img
            };

        log_debug("Creating protocol...");
        let protocol_start = std::time::Instant::now();
        let protocol = picker.new_resize_protocol(processed_img);
        log_debug(&format!(
            "Protocol creation took {:.2}s",
            protocol_start.elapsed().as_secs_f32()
        ));
        log_debug(&format!(
            "Total image load took {:.2}s",
            load_start.elapsed().as_secs_f32()
        ));

        // Return both protocol and metadata
        let metadata = ImageMetadata {
            dimensions: Some(dimensions),
            format: Some(format.to_string()),
            file_size,
        };

        Ok((protocol, metadata))
    }

    async fn read_file_content_static(
        path_map: &HashMap<String, String>,
        folder: &Folder,
        relative_path: &str,
    ) -> (Result<String, String>, bool, bool) {
        const MAX_SIZE: u64 = 20 * 1024 * 1024; // 20MB
        const BINARY_CHECK_SIZE: usize = 8192; // First 8KB

        // Translate container path to host path
        let container_path = format!("{}/{}", folder.path.trim_end_matches('/'), relative_path);
        let mut host_path = container_path.clone();

        // Try to map container path to host path using path_map
        for (container_prefix, host_prefix) in path_map {
            if container_path.starts_with(container_prefix) {
                let remainder = container_path.strip_prefix(container_prefix).unwrap_or("");
                host_path = format!("{}{}", host_prefix.trim_end_matches('/'), remainder);
                break;
            }
        }

        // Check if file exists
        let metadata = match tokio::fs::metadata(&host_path).await {
            Ok(m) => m,
            Err(_) => return (Err("File not found on disk".to_string()), false, false),
        };

        let exists = true;

        // Check if it's a directory
        if metadata.is_dir() {
            return (Ok("[Directory]".to_string()), exists, false);
        }

        // Check file size
        if metadata.len() > MAX_SIZE {
            return (
                Err(format!(
                    "File too large ({}) - max 20MB",
                    crate::utils::format_bytes(metadata.len())
                )),
                exists,
                false,
            );
        }

        // Read file content
        match tokio::fs::read(&host_path).await {
            Ok(bytes) => {
                // Check if binary (null bytes in first 8KB)
                // Exception: .ans/.asc files (ANSI art) can have null bytes but should be treated as text
                let check_size = std::cmp::min(bytes.len(), BINARY_CHECK_SIZE);
                let path_lower = host_path.to_lowercase();
                let has_ansi_extension =
                    path_lower.ends_with(".ans") || path_lower.ends_with(".asc");
                let is_binary =
                    !has_ansi_extension && logic::file::is_binary_content(&bytes[..check_size]);

                if is_binary {
                    // Attempt text extraction (similar to 'strings' command)
                    let extracted = Self::extract_text_from_binary(&bytes);
                    (Ok(extracted), exists, true)
                } else {
                    // Check if content has ANSI codes on raw bytes BEFORE decoding
                    // ANSI art files use CP437 encoding and need special handling
                    let has_ansi_codes = logic::file::contains_ansi_codes(&bytes);
                    let should_use_cp437 = has_ansi_extension || has_ansi_codes;

                    if should_use_cp437 {
                        use codepage_437::{BorrowFromCp437, CP437_CONTROL};
                        // Decode from CP437 to Unicode string
                        // CP437_CONTROL variant preserves control characters (important for ANSI escape codes)
                        let decoded = String::borrow_from_cp437(&bytes, &CP437_CONTROL);
                        (Ok(decoded.to_string()), exists, false)
                    } else {
                        // Try to decode as UTF-8
                        match String::from_utf8(bytes.clone()) {
                            Ok(content) => (Ok(content), exists, false),
                            Err(_) => {
                                // Try lossy conversion
                                let content = String::from_utf8_lossy(&bytes).to_string();
                                (Ok(content), exists, true)
                            }
                        }
                    }
                }
            }
            Err(e) => (Err(format!("Failed to read file: {}", e)), exists, false),
        }
    }

    fn extract_text_from_binary(bytes: &[u8]) -> String {
        logic::file::extract_text_from_binary(bytes)
    }
}

/// Decode the image at `path`, rotated and flipped as its EXIF orientation says. Camera
/// photos often store pixels sideways and rely on that tag.
pub(crate) fn open_upright(path: &std::path::Path) -> image::ImageResult<image::DynamicImage> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2x1 JPEG, optionally carrying an EXIF orientation tag
    fn jpeg_2x1(orientation: Option<u16>) -> Vec<u8> {
        let mut jpeg = Vec::new();
        image::RgbImage::from_pixel(2, 1, image::Rgb([200, 30, 30]))
            .write_to(
                &mut std::io::Cursor::new(&mut jpeg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let Some(orientation) = orientation else {
            return jpeg;
        };
        // APP1 "Exif" segment: big-endian TIFF header, one IFD entry (0x0112 Orientation, SHORT)
        let mut exif = b"Exif\0\0MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
        exif.extend_from_slice(&orientation.to_be_bytes());
        exif.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut out = jpeg[..2].to_vec(); // SOI
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        out.extend_from_slice(&exif);
        out.extend_from_slice(&jpeg[2..]);
        out
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("stui-{}-{}", std::process::id(), name));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn test_open_upright_applies_exif_rotation() {
        let path = write_temp("rotated.jpg", &jpeg_2x1(Some(6)));
        let img = open_upright(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((img.width(), img.height()), (1, 2));
    }

    #[test]
    fn test_open_upright_leaves_untagged_image_alone() {
        let path = write_temp("plain.jpg", &jpeg_2x1(None));
        let img = open_upright(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!((img.width(), img.height()), (2, 1));
    }
}
