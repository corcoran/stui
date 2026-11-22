//! Plain gzip files (not tarballs) for the file preview: the inner file is decompressed
//! and previewed under its name without `.gz`.
use crate::logic::archive::archive_kind;
use flate2::read::MultiGzDecoder;
use std::io::Read;

/// What the preview says about a decompressed gzip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GzipNote {
    /// The file name without `.gz`; it picks the decoding and the rendered view.
    pub inner_name: String,
    /// Output stopped at the cap.
    pub truncated: bool,
    /// The stream was damaged or cut off after some output.
    pub incomplete: bool,
}

/// Decompressed output stops here; `GzipNote::label` names it.
pub const MAX_GUNZIP_BYTES: usize = 20 * 1024 * 1024;

impl GzipNote {
    /// "gzip", plus "first 20 MB" and "incomplete" when they apply.
    pub fn label(&self) -> String {
        let mut label = String::from("gzip");
        if self.truncated {
            label.push_str(", first 20 MB");
        }
        if self.incomplete {
            label.push_str(", incomplete");
        }
        label
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gunzipped {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub incomplete: bool,
}

/// `head` is the start of the file (up to 8 KB); a tarball is not plain gzip.
pub fn is_plain_gzip(head: &[u8]) -> bool {
    head.starts_with(&[0x1f, 0x8b]) && archive_kind(head).is_none()
}

pub fn inner_name(path: &str) -> &str {
    let cut = path.len().saturating_sub(3);
    match path.get(cut..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".gz") => &path[..cut],
        _ => path,
    }
}

/// Decompresses every member up to `cap` bytes of output. A damaged or cut-off stream keeps
/// what it decoded and is marked incomplete; it is an error only when nothing decoded.
pub fn gunzip_limited<R: Read>(reader: R, cap: usize) -> Result<Gunzipped, String> {
    let mut decoder = MultiGzDecoder::new(reader).take(cap as u64 + 1);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let mut incomplete = false;
    loop {
        match decoder.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if bytes.is_empty() => return Err(e.to_string()),
            Err(_) => {
                incomplete = true;
                break;
            }
        }
    }
    let truncated = bytes.len() > cap;
    bytes.truncate(cap);
    Ok(Gunzipped {
        bytes,
        truncated,
        incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    fn tar_gz() -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_ustar();
        header.set_size(5);
        header.set_cksum();
        builder
            .append_data(&mut header, "a.txt", &b"hello"[..])
            .unwrap();
        gzip(&builder.into_inner().unwrap())
    }

    // ========================================
    // DETECTION
    // ========================================

    #[test]
    fn test_plain_gzip_is_detected() {
        assert!(is_plain_gzip(&gzip(b"log line\n")));
    }

    #[test]
    fn test_tar_gz_is_not_plain_gzip() {
        assert!(!is_plain_gzip(&tar_gz()));
    }

    #[test]
    fn test_other_bytes_are_not_gzip() {
        assert!(!is_plain_gzip(b"plain text"));
        assert!(!is_plain_gzip(&[0x1f]));
        assert!(!is_plain_gzip(b""));
    }

    // ========================================
    // INNER NAME
    // ========================================

    #[test]
    fn test_inner_name_strips_gz() {
        assert_eq!(inner_name("logs/config.json.gz"), "logs/config.json");
        assert_eq!(inner_name("SYSLOG.1.GZ"), "SYSLOG.1");
    }

    #[test]
    fn test_inner_name_without_gz_suffix_is_unchanged() {
        assert_eq!(inner_name("blob"), "blob");
        assert_eq!(inner_name("drawing.svgz"), "drawing.svgz");
    }

    // ========================================
    // DECOMPRESSION
    // ========================================

    #[test]
    fn test_gunzip_returns_inner_bytes() {
        let out = gunzip_limited(&gzip(b"hello world")[..], 1024).unwrap();
        assert_eq!(
            out,
            Gunzipped {
                bytes: b"hello world".to_vec(),
                truncated: false,
                incomplete: false
            }
        );
    }

    #[test]
    fn test_gunzip_stops_at_cap() {
        let out = gunzip_limited(&gzip(&[b'x'; 100])[..], 40).unwrap();
        assert_eq!(out.bytes.len(), 40);
        assert!(out.truncated);
        assert!(!out.incomplete);
    }

    #[test]
    fn test_gunzip_output_exactly_at_cap_is_not_truncated() {
        let out = gunzip_limited(&gzip(&[b'x'; 40])[..], 40).unwrap();
        assert_eq!(out.bytes.len(), 40);
        assert!(!out.truncated);
    }

    #[test]
    fn test_gunzip_reads_every_member() {
        let mut bytes = gzip(b"first\n");
        bytes.extend(gzip(b"second\n"));
        let out = gunzip_limited(&bytes[..], 1024).unwrap();
        assert_eq!(out.bytes, b"first\nsecond\n");
    }

    #[test]
    fn test_cut_off_stream_keeps_partial_output() {
        let text: Vec<u8> = (0..20_000).map(|i| b"abcdefghij"[i % 10]).collect();
        let whole = gzip(&text);
        let out = gunzip_limited(&whole[..whole.len() - 12], 1 << 20).unwrap();
        assert!(out.incomplete);
        assert!(!out.bytes.is_empty());
        assert!(text.starts_with(&out.bytes));
    }

    #[test]
    fn test_garbage_after_magic_is_an_error() {
        let bytes = [
            0x1f, 0x8b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ];
        assert!(gunzip_limited(&bytes[..], 1024).is_err());
    }
}
