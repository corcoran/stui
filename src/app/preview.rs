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
                    art: None,
                    archive: None,
                    gzip: None,
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
            art: None,
            archive: None,
            gzip: None,
            scroll_offset: 0,
            raw_view: false,
        });

        // 1. Fetch file details from API
        let file_details = self.client.get_file_info(&folder_id, &file_path).await.ok();

        // 2. If image, spawn background loading; otherwise read as text
        let mut art: Option<logic::file::ArtStyle> = None;
        let mut archive: Option<logic::archive::ArchiveListing> = None;
        let mut gzip: Option<logic::gzip::GzipNote> = None;
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
            let read = Self::read_file_content_static(&self.path_map, &folder, &file_path).await;
            art = read.art;
            archive = read.archive;
            gzip = read.gzip;
            (read.content, read.exists, read.is_binary, None)
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

        let file_content_ok = file_content.is_ok();

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
            art: art.filter(|_| file_content_ok),
            archive: archive.filter(|_| file_content_ok),
            gzip: gzip.filter(|_| file_content_ok),
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

    /// Lists the file when its first bytes say it is an archive; `None` for anything else.
    async fn read_archive_listing(host_path: &str) -> Option<TextRead> {
        use tokio::io::AsyncReadExt;

        let mut head = vec![0u8; 8192];
        let mut file = tokio::fs::File::open(host_path).await.ok()?;
        let n = file.read(&mut head).await.ok()?;
        head.truncate(n);
        let kind = logic::archive::archive_kind(&head)?;

        let path = std::path::PathBuf::from(host_path);
        let listed = tokio::task::spawn_blocking(move || logic::archive::read_archive(&path, kind))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        Some(match listed {
            Ok(listing) => TextRead {
                content: Ok(String::new()),
                exists: true,
                is_binary: false,
                art: None,
                archive: Some(listing),
                gzip: None,
            },
            Err(e) => TextRead {
                content: Err(format!("Couldn't read archive: {e}")),
                exists: true,
                is_binary: false,
                art: None,
                archive: None,
                gzip: None,
            },
        })
    }

    /// Decompresses a plain gzip and decodes the inner file under its name without `.gz`;
    /// `None` when the file is not a plain gzip.
    async fn read_gzip(host_path: &str, relative_path: &str) -> Option<TextRead> {
        use tokio::io::AsyncReadExt;

        let mut head = vec![0u8; 8192];
        let mut file = tokio::fs::File::open(host_path).await.ok()?;
        let n = file.read(&mut head).await.ok()?;
        head.truncate(n);
        if !logic::gzip::is_plain_gzip(&head) {
            return None;
        }

        let path = std::path::PathBuf::from(host_path);
        let unpacked = tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            logic::gzip::gunzip_limited(
                std::io::BufReader::new(file),
                logic::gzip::MAX_GUNZIP_BYTES,
            )
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        Some(match unpacked {
            Ok(out) => {
                let inner_name = logic::gzip::inner_name(relative_path).to_string();
                let decoded = logic::file::decode_preview_bytes(&inner_name, &out.bytes);
                TextRead {
                    content: Ok(decoded.text),
                    exists: true,
                    is_binary: decoded.is_binary,
                    art: decoded.art,
                    archive: None,
                    gzip: Some(logic::gzip::GzipNote {
                        inner_name,
                        truncated: out.truncated,
                        incomplete: out.incomplete,
                    }),
                }
            }
            Err(e) => TextRead {
                content: Err(format!("Couldn't decompress: {e}")),
                exists: true,
                is_binary: false,
                art: None,
                archive: None,
                gzip: None,
            },
        })
    }

    async fn read_file_content_static(
        path_map: &HashMap<String, String>,
        folder: &Folder,
        relative_path: &str,
    ) -> TextRead {
        const MAX_SIZE: u64 = 20 * 1024 * 1024; // 20MB

        let failed = |message: String, exists: bool| TextRead {
            content: Err(message),
            exists,
            is_binary: false,
            art: None,
            archive: None,
            gzip: None,
        };

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

        let metadata = match tokio::fs::metadata(&host_path).await {
            Ok(m) => m,
            Err(_) => return failed("File not found on disk".to_string(), false),
        };

        if metadata.is_dir() {
            return TextRead {
                content: Ok("[Directory]".to_string()),
                exists: true,
                is_binary: false,
                art: None,
                archive: None,
                gzip: None,
            };
        }

        if let Some(read) = Self::read_archive_listing(&host_path).await {
            return read;
        }
        if let Some(read) = Self::read_gzip(&host_path, relative_path).await {
            return read;
        }

        if metadata.len() > MAX_SIZE {
            return failed(
                format!(
                    "File too large ({}) - max 20MB",
                    crate::utils::format_bytes(metadata.len())
                ),
                true,
            );
        }

        match tokio::fs::read(&host_path).await {
            Ok(bytes) => {
                let decoded = logic::file::decode_preview_bytes(&host_path, &bytes);
                TextRead {
                    content: Ok(decoded.text),
                    exists: true,
                    is_binary: decoded.is_binary,
                    art: decoded.art,
                    archive: None,
                    gzip: None,
                }
            }
            Err(e) => failed(format!("Failed to read file: {}", e), true),
        }
    }
}

/// What reading a file for the text preview produced.
struct TextRead {
    content: Result<String, String>,
    exists: bool,
    is_binary: bool,
    art: Option<logic::file::ArtStyle>,
    archive: Option<logic::archive::ArchiveListing>,
    gzip: Option<logic::gzip::GzipNote>,
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

    #[tokio::test]
    async fn test_archive_over_20mb_still_lists() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "a.txt", &b"abc"[..])
            .unwrap();
        let dir = std::env::temp_dir().join(format!("stui-bigtar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.tar");
        std::fs::write(&path, builder.into_inner().unwrap()).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(21 * 1024 * 1024)
            .unwrap();
        let folder = Folder {
            id: "f".into(),
            label: None,
            path: dir.to_string_lossy().into_owned(),
            paused: false,
            folder_type: "sendreceive".into(),
            devices: vec![],
            group: None,
        };
        let read = App::read_file_content_static(&HashMap::new(), &folder, "big.tar").await;
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read.content.is_ok(), "{:?}", read.content);
        assert_eq!(read.archive.map(|a| a.total_files), Some(1));
    }

    fn gzip_bytes(bytes: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    /// Reads `name` holding `bytes` from a fresh folder.
    async fn read_one(name: &str, bytes: &[u8]) -> TextRead {
        let dir = std::env::temp_dir().join(format!(
            "stui-gz-{}-{}",
            std::process::id(),
            name.replace('.', "-")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
        let folder = Folder {
            id: "f".into(),
            label: None,
            path: dir.to_string_lossy().into_owned(),
            paused: false,
            folder_type: "sendreceive".into(),
            devices: vec![],
            group: None,
        };
        let read = App::read_file_content_static(&HashMap::new(), &folder, name).await;
        let _ = std::fs::remove_dir_all(&dir);
        read
    }

    #[tokio::test]
    async fn test_gzip_previews_the_inner_file() {
        let read = read_one("config.json.gz", &gzip_bytes(br#"{"a":1}"#)).await;
        assert_eq!(read.content, Ok(r#"{"a":1}"#.to_string()));
        assert!(!read.is_binary);
        assert!(read.archive.is_none());
        assert_eq!(
            read.gzip,
            Some(logic::gzip::GzipNote {
                inner_name: "config.json".into(),
                truncated: false,
                incomplete: false,
            })
        );
    }

    #[tokio::test]
    async fn test_gzip_inner_art_name_renders_art() {
        let read = read_one("logo.ans.gz", &gzip_bytes(b"\x1b[31mX\x1b[0m")).await;
        assert!(read.art.is_some());
    }

    #[tokio::test]
    async fn test_gzip_inner_output_stops_at_20mb() {
        let read = read_one("zeros.txt.gz", &gzip_bytes(&vec![b'a'; 21 * 1024 * 1024])).await;
        assert_eq!(read.content.map(|c| c.len()), Ok(20 * 1024 * 1024));
        assert!(read.gzip.is_some_and(|g| g.truncated));
    }

    #[tokio::test]
    async fn test_damaged_gzip_shows_error() {
        let read = read_one("bad.gz", &[0x1f, 0x8b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]).await;
        assert!(
            read.content
                .as_ref()
                .is_err_and(|e| e.starts_with("Couldn't decompress")),
            "{:?}",
            read.content
        );
    }

    #[tokio::test]
    async fn test_tar_gz_still_lists() {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_ustar();
        header.set_size(3);
        header.set_cksum();
        builder
            .append_data(&mut header, "a.txt", &b"abc"[..])
            .unwrap();
        let read = read_one("s.tar.gz", &gzip_bytes(&builder.into_inner().unwrap())).await;
        assert!(read.archive.is_some());
        assert!(read.gzip.is_none());
    }
}
