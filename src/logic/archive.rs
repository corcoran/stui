//! Archive listings (zip, tar, tar.gz) for the file preview.

#![allow(dead_code)]

use crate::utils::format_bytes;
use flate2::read::GzDecoder;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use std::cell::Cell;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
}

impl ArchiveKind {
    pub fn label(self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::Tar => "tar",
            ArchiveKind::TarGz => "tar.gz",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: String,
    pub size: u64,
    pub modified: Option<String>,
    pub is_dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveListing {
    pub kind: ArchiveKind,
    /// Sorted by path.
    pub entries: Vec<ArchiveEntry>,
    /// Non-directory entries.
    pub total_files: u64,
    pub total_size: u64,
    /// Zip only: sum of compressed sizes.
    pub packed_size: Option<u64>,
    /// Note shown when the list is incomplete.
    pub stopped_early: Option<String>,
}

pub const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 10_000;

/// `head` is the start of the file (up to 8 KB). A gzip counts as `TarGz` only when its
/// decompressed start carries a tar header.
pub fn archive_kind(head: &[u8]) -> Option<ArchiveKind> {
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Some(ArchiveKind::Zip);
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        let mut buf = Vec::new();
        // A truncated stream errors after yielding what it decoded; the partial bytes suffice.
        let _ = GzDecoder::new(head).take(512).read_to_end(&mut buf);
        return has_ustar(&buf).then_some(ArchiveKind::TarGz);
    }
    has_ustar(head).then_some(ArchiveKind::Tar)
}

fn has_ustar(bytes: &[u8]) -> bool {
    bytes.get(257..262) == Some(b"ustar".as_slice())
}

pub fn read_archive(path: &Path, kind: ArchiveKind) -> Result<ArchiveListing, String> {
    read_archive_with_limits(path, kind, MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES)
}

pub fn read_archive_with_limits(
    path: &Path,
    kind: ArchiveKind,
    max_bytes: u64,
    max_entries: usize,
) -> Result<ArchiveListing, String> {
    match kind {
        ArchiveKind::Zip => read_zip(path, max_entries),
        ArchiveKind::Tar | ArchiveKind::TarGz => read_tar(path, kind, max_bytes, max_entries),
    }
}

/// Reports end of file after `limit` bytes and records that it did, so a cut-off stream can
/// be told apart from a corrupt one.
struct LimitedReader<R> {
    inner: R,
    left: u64,
    hit: Rc<Cell<bool>>,
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.left == 0 {
            self.hit.set(true);
            return Ok(0);
        }
        let want = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        self.left -= n as u64;
        Ok(n)
    }
}

/// The limit counts raw file bytes, so it also stops a read inside one large entry.
fn read_tar(
    path: &Path,
    kind: ArchiveKind,
    max_bytes: u64,
    max_entries: usize,
) -> Result<ArchiveListing, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let hit = Rc::new(Cell::new(false));
    let limited = LimitedReader {
        inner: file,
        left: max_bytes,
        hit: hit.clone(),
    };
    let reader: Box<dyn Read> = if kind == ArchiveKind::TarGz {
        Box::new(GzDecoder::new(limited))
    } else {
        Box::new(limited)
    };
    let mut archive = tar::Archive::new(reader);
    let iter = archive.entries().map_err(|e| e.to_string())?;
    let mut entries: Vec<ArchiveEntry> = Vec::new();
    let mut total_files = 0u64;
    let mut total_size = 0u64;
    let mut stopped_early = None;
    for entry in iter {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) if hit.get() => break,
            Err(err) => return Err(err.to_string()),
        };
        if entries.len() >= max_entries {
            stopped_early = Some(format!("Showing the first {} entries", entries.len()));
            break;
        }
        let header = entry.header();
        let is_dir = header.entry_type().is_dir();
        let size = header.size().unwrap_or(0);
        if !is_dir {
            total_files += 1;
            total_size = total_size.saturating_add(size);
        }
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        entries.push(ArchiveEntry {
            path: name.trim_end_matches('/').to_string(),
            size,
            modified: header.mtime().ok().and_then(format_unix_date),
            is_dir,
        });
    }
    if stopped_early.is_none() && hit.get() {
        stopped_early = Some(format!(
            "Showing the first {} entries (stopped after {})",
            entries.len(),
            format_bytes(max_bytes)
        ));
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ArchiveListing {
        kind,
        entries,
        total_files,
        total_size,
        packed_size: None,
        stopped_early,
    })
}

fn format_unix_date(secs: u64) -> Option<String> {
    let utc = chrono::DateTime::from_timestamp(i64::try_from(secs).ok()?, 0)?;
    Some(
        utc.with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    )
}

fn read_zip(path: &Path, max_entries: usize) -> Result<ArchiveListing, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entries = Vec::with_capacity(archive.len().min(max_entries.saturating_add(1)));
    let mut total_files = 0u64;
    let mut total_size = 0u64;
    let mut packed = 0u64;
    for index in 0..archive.len() {
        // The raw entry skips decompression and decryption, so encrypted entries list too.
        let entry = archive.by_index_raw(index).map_err(|e| e.to_string())?;
        let name = entry
            .name()
            .map(|n| n.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(entry.name_raw()).into_owned());
        let is_dir = name.ends_with('/');
        packed = packed.saturating_add(entry.compressed_size());
        if !is_dir {
            total_files += 1;
            total_size = total_size.saturating_add(entry.size());
        }
        entries.push(ArchiveEntry {
            path: name.trim_end_matches('/').to_string(),
            size: entry.size(),
            modified: entry.last_modified().and_then(format_dos_date),
            is_dir,
        });
    }
    let packed_size = (!entries.is_empty()).then_some(packed);
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let stopped_early = (entries.len() > max_entries).then(|| {
        format!(
            "Showing the first {} of {} entries",
            group_digits(max_entries as u64),
            group_digits(entries.len() as u64)
        )
    });
    entries.truncate(max_entries);
    Ok(ArchiveListing {
        kind: ArchiveKind::Zip,
        entries,
        total_files,
        total_size,
        packed_size,
        stopped_early,
    })
}

fn format_dos_date(dt: zip::DateTime) -> Option<String> {
    if !dt.is_valid() {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute()
    ))
}

fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn listing_text(listing: &ArchiveListing) -> Text<'static> {
    let files = if listing.total_files == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", listing.total_files)
    };
    let mut summary = format!(
        "{} · {} · {}",
        listing.kind.label(),
        files,
        format_bytes(listing.total_size)
    );
    if let Some(packed) = listing.packed_size {
        summary.push_str(&format!(" ({} packed)", format_bytes(packed)));
    }
    let mut lines = vec![Line::from(Span::styled(
        summary,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))];
    if let Some(note) = &listing.stopped_early {
        lines.push(Line::from(Span::styled(
            note.clone(),
            Style::default().fg(Color::Yellow),
        )));
    }
    lines.push(Line::default());
    for entry in &listing.entries {
        let date = entry.modified.as_deref().unwrap_or("");
        let size = if entry.is_dir {
            "-".to_string()
        } else {
            format_bytes(entry.size)
        };
        let suffix = if entry.is_dir { "/" } else { "" };
        lines.push(Line::from(format!(
            "{:<16}  {:>9}  {}{}",
            date, size, entry.path, suffix
        )));
    }
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use std::io::Write;
    use std::path::PathBuf;

    fn tar_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(bytes).unwrap();
        enc.finish().unwrap()
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("stui-archive-{}-{}", std::process::id(), name))
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            if name.ends_with('/') {
                writer.add_directory(*name, options).unwrap();
            } else {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path = temp_path(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn lines(text: &Text<'_>) -> Vec<String> {
        text.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn test_detect_kinds() {
        assert_eq!(archive_kind(b"PK\x03\x04rest"), Some(ArchiveKind::Zip));
        assert_eq!(archive_kind(b"PK\x05\x06\0\0"), Some(ArchiveKind::Zip));
        assert_eq!(
            archive_kind(&tar_bytes(&[("a.txt", b"hi")])),
            Some(ArchiveKind::Tar)
        );
        assert_eq!(
            archive_kind(&gzip(&tar_bytes(&[("a.txt", b"hi")]))),
            Some(ArchiveKind::TarGz)
        );
        assert_eq!(archive_kind(b"hello world"), None);
    }

    #[test]
    fn test_plain_gzip_is_not_an_archive() {
        assert_eq!(archive_kind(&gzip(b"just a log line\n")), None);
    }

    #[test]
    fn test_zip_listing() {
        let path = write_temp(
            "listing.zip",
            &zip_bytes(&[("b.txt", b"hello"), ("a/", b""), ("a/c.txt", b"abc")]),
        );
        let l = read_archive(&path, ArchiveKind::Zip).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            l.entries
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["a", "a/c.txt", "b.txt"]
        );
        assert!(l.entries[0].is_dir);
        assert_eq!((l.total_files, l.total_size), (2, 8));
        assert_eq!(l.packed_size, Some(8));
        assert!(
            l.entries[2]
                .modified
                .as_deref()
                .is_some_and(|d| d.len() == 16)
        );
        assert_eq!(l.stopped_early, None);
    }

    #[test]
    fn test_zip_directory_names() {
        let path = write_temp(
            "names.zip",
            &zip_bytes(&[("dir/", b""), ("dir/naïve\\name.txt", b"x")]),
        );
        let l = read_archive(&path, ArchiveKind::Zip).unwrap();
        std::fs::remove_file(&path).unwrap();
        let text = lines(&listing_text(&l));
        assert!(text.iter().any(|r| r.ends_with("  dir/")));
        assert!(text.iter().any(|r| r.ends_with("dir/naïve\\name.txt")));
    }

    #[test]
    fn test_empty_zip() {
        let path = write_temp("empty.zip", &zip_bytes(&[]));
        let l = read_archive(&path, ArchiveKind::Zip).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(lines(&listing_text(&l)), ["zip · 0 files · 0 B", ""]);
    }

    #[test]
    fn test_zip_entry_limit_keeps_totals() {
        let names = ["1", "2", "3", "4", "5", "6"];
        let entries: Vec<(&str, &[u8])> = names.iter().map(|n| (*n, &b"x"[..])).collect();
        let path = write_temp("limit.zip", &zip_bytes(&entries));
        let l = read_archive_with_limits(&path, ArchiveKind::Zip, MAX_ARCHIVE_BYTES, 4).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(l.entries.len(), 4);
        assert_eq!(l.total_files, 6);
        assert_eq!(
            l.stopped_early.as_deref(),
            Some("Showing the first 4 of 6 entries")
        );
    }

    #[test]
    fn test_corrupt_zip_is_an_error() {
        let path = write_temp("corrupt.zip", b"PK\x03\x04garbage");
        let result = read_archive(&path, ArchiveKind::Zip);
        std::fs::remove_file(&path).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypted_zip_lists_names() {
        let mut bytes = zip_bytes(&[("secret.txt", b"data")]);
        let local = bytes.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
        bytes[local + 6] |= 1;
        let central = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        bytes[central + 8] |= 1;
        let path = write_temp("encrypted.zip", &bytes);
        let l = read_archive(&path, ArchiveKind::Zip).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(l.entries[0].path, "secret.txt");
    }

    #[test]
    fn test_listing_text_rows() {
        let l = ArchiveListing {
            kind: ArchiveKind::Zip,
            total_files: 1,
            total_size: 2048,
            packed_size: Some(1024),
            stopped_early: Some("Showing the first 1 of 2 entries".into()),
            entries: vec![
                ArchiveEntry {
                    path: "docs".into(),
                    size: 0,
                    modified: None,
                    is_dir: true,
                },
                ArchiveEntry {
                    path: "docs/a.txt".into(),
                    size: 2048,
                    modified: Some("2026-10-02 12:01".into()),
                    is_dir: false,
                },
            ],
        };
        let t = listing_text(&l);
        assert_eq!(
            lines(&t),
            [
                "zip · 1 file · 2.00 KB (1.00 KB packed)",
                "Showing the first 1 of 2 entries",
                "",
                "                          -  docs/",
                "2026-10-02 12:01    2.00 KB  docs/a.txt",
            ]
        );
        assert_eq!(t.lines[0].spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(t.lines[1].spans[0].style.fg, Some(Color::Yellow));
    }

    // ========================================
    // TAR AND TAR.GZ
    // ========================================

    fn tar_with_dir() -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "z.txt", &b"abc"[..])
            .unwrap();
        let mut dir = tar::Header::new_gnu();
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_size(0);
        dir.set_mode(0o755);
        dir.set_cksum();
        builder.append_data(&mut dir, "a/", &b""[..]).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "a/b.txt", &b"abcd"[..])
            .unwrap();
        builder.into_inner().unwrap()
    }

    fn random_bytes(len: usize) -> Vec<u8> {
        random_bytes_seeded(len, 0)
    }

    fn random_bytes_seeded(len: usize, seed: u64) -> Vec<u8> {
        let mut state = 0x2545_F491_4F6C_DD1Du64 ^ seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn test_tar_listing() {
        let path = write_temp("listing.tar", &tar_with_dir());
        let l = read_archive(&path, ArchiveKind::Tar).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            l.entries
                .iter()
                .map(|e| e.path.as_str())
                .collect::<Vec<_>>(),
            ["a", "a/b.txt", "z.txt"]
        );
        assert!(l.entries[0].is_dir);
        assert_eq!((l.total_files, l.total_size, l.packed_size), (2, 7, None));
        assert!(l.entries[1].modified.is_some());
    }

    #[test]
    fn test_tar_gz_listing() {
        let path = write_temp("listing.tar.gz", &gzip(&tar_with_dir()));
        let l = read_archive(&path, ArchiveKind::TarGz).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(l.kind, ArchiveKind::TarGz);
        assert_eq!(l.total_files, 2);
        assert_eq!(lines(&listing_text(&l))[0], "tar.gz · 2 files · 7 B");
    }

    #[test]
    fn test_tar_entry_limit() {
        let names = ["1", "2", "3", "4", "5", "6"];
        let files: Vec<(&str, &[u8])> = names.iter().map(|n| (*n, &b"x"[..])).collect();
        let path = write_temp("limit.tar", &tar_bytes(&files));
        let l = read_archive_with_limits(&path, ArchiveKind::Tar, MAX_ARCHIVE_BYTES, 5).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(l.entries.len(), 5);
        assert_eq!(
            l.stopped_early.as_deref(),
            Some("Showing the first 5 entries")
        );
    }

    #[test]
    fn test_tar_byte_limit_between_entries() {
        let data: Vec<Vec<u8>> = (0..20).map(|i| random_bytes_seeded(4096, i + 1)).collect();
        let names: Vec<String> = (0..20).map(|i| format!("f{i:02}")).collect();
        let files: Vec<(&str, &[u8])> = names
            .iter()
            .zip(&data)
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        let path = write_temp("between.tar.gz", &gzip(&tar_bytes(&files)));
        let l = read_archive_with_limits(&path, ArchiveKind::TarGz, 8 * 1024, MAX_ARCHIVE_ENTRIES)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(l.entries.len() < 20);
        assert!(
            l.stopped_early
                .as_deref()
                .unwrap()
                .ends_with("entries (stopped after 8.00 KB)")
        );
    }

    #[test]
    fn test_tar_byte_limit_applies_inside_an_entry() {
        let data = random_bytes(2 * 1024 * 1024);
        let path = write_temp("inside.tar.gz", &gzip(&tar_bytes(&[("big.bin", &data)])));
        let l = read_archive_with_limits(&path, ArchiveKind::TarGz, 64 * 1024, MAX_ARCHIVE_ENTRIES)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(l.stopped_early.is_some());
    }

    #[test]
    fn test_truncated_tar_gz_before_limit_is_an_error() {
        let bytes = gzip(&tar_bytes(&[("a.bin", &random_bytes(64 * 1024))]));
        let path = write_temp("truncated.tar.gz", &bytes[..bytes.len() / 2]);
        let result = read_archive(&path, ArchiveKind::TarGz);
        std::fs::remove_file(&path).unwrap();
        assert!(result.is_err());
    }
}
