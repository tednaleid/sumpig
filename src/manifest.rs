use std::io;

use crate::merkle::{EntryType, FlatEntry};

pub struct ManifestHeader {
    pub host: String,
    pub path: String,
    pub depth: usize,
    pub date: String,
    pub total_files: usize,
    pub total_dirs: usize,
    pub root_hash: String,
}

/// A parsed manifest entry (from a fingerprint file).
#[derive(Debug, PartialEq)]
pub struct ManifestEntry {
    pub entry_type: String,
    pub value: String,
    pub path: String,
}

/// Write a manifest header and entries to a writer.
pub fn write_manifest<W: io::Write>(
    writer: &mut W,
    header: &ManifestHeader,
    entries: &[FlatEntry],
) -> io::Result<()> {
    writeln!(writer, "# sumpig fingerprint")?;
    writeln!(writer, "# version: 1")?;
    writeln!(writer, "# host: {}", header.host)?;
    writeln!(writer, "# path: {}", header.path)?;
    writeln!(writer, "# depth: {}", header.depth)?;
    writeln!(writer, "# date: {}", header.date)?;
    writeln!(writer, "# total_files: {}", header.total_files)?;
    writeln!(writer, "# total_dirs: {}", header.total_dirs)?;
    writeln!(writer, "# root: {}", header.root_hash)?;
    for entry in entries {
        let type_tag = match entry.entry_type {
            EntryType::Blake3 => "blake3",
            EntryType::Dataless => "dataless",
            EntryType::Error => "error",
            EntryType::Symlink => "symlink",
            EntryType::Dir => "dir",
        };
        writeln!(writer, "{type_tag}:{}  {}", entry.value, entry.path)?;
    }
    Ok(())
}

/// Parse a manifest file into header + entries.
pub fn parse_manifest<R: io::BufRead>(
    reader: R,
) -> Result<(ManifestHeader, Vec<ManifestEntry>), ParseError> {
    let mut header = ManifestHeader {
        host: String::new(),
        path: String::new(),
        depth: 0,
        date: String::new(),
        total_files: 0,
        total_dirs: 0,
        root_hash: String::new(),
    };
    let mut entries = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim_end();

        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("# ") {
            // Header comment line.
            if let Some((key, value)) = rest.split_once(": ") {
                match key {
                    "host" => header.host = value.to_string(),
                    "path" => header.path = value.to_string(),
                    "depth" => {
                        header.depth = value
                            .parse()
                            .map_err(|_| ParseError::Format(format!("invalid depth: {value}")))?;
                    }
                    "date" => header.date = value.to_string(),
                    "total_files" => {
                        header.total_files = value.parse().map_err(|_| {
                            ParseError::Format(format!("invalid total_files: {value}"))
                        })?;
                    }
                    "total_dirs" => {
                        header.total_dirs = value.parse().map_err(|_| {
                            ParseError::Format(format!("invalid total_dirs: {value}"))
                        })?;
                    }
                    "root" => header.root_hash = value.to_string(),
                    _ => {} // Unknown header fields are ignored.
                }
            }
            // Lines like "# sumpig fingerprint" (no ": ") are silently skipped.
            continue;
        }

        // Data line: "type:value  path"
        let Some((type_value, path)) = line.split_once("  ") else {
            return Err(ParseError::Format(format!("invalid data line: {line}")));
        };
        let Some((entry_type, value)) = type_value.split_once(':') else {
            return Err(ParseError::Format(format!(
                "invalid type:value in data line: {line}"
            )));
        };

        entries.push(ManifestEntry {
            entry_type: entry_type.to_string(),
            value: value.to_string(),
            path: path.to_string(),
        });
    }

    Ok((header, entries))
}

/// Get the hostname of the current machine.
pub fn get_hostname() -> String {
    let mut buf = [0u8; 256];
    let ret = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if ret != 0 {
        return "unknown".to_string();
    }
    // Find the null terminator.
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

/// Get the current date/time in ISO 8601 format (YYYY-MM-DDTHH:MM:SS).
pub fn get_iso_date() -> String {
    use std::time::SystemTime;

    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Convert epoch seconds to calendar date/time.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since 1970-01-01 to (year, month, day).
    // Using the civil_from_days algorithm (Howard Hinnant).
    let (year, month, day) = civil_from_days(days as i64);

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}")
}

/// Convert days since 1970-01-01 to (year, month, day).
/// Based on Howard Hinnant's civil_from_days algorithm.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[derive(Debug)]
pub enum ParseError {
    Io(io::Error),
    Format(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "I/O error: {e}"),
            ParseError::Format(msg) => write!(f, "format error: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<io::Error> for ParseError {
    fn from(e: io::Error) -> Self {
        ParseError::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle::{EntryType, FlatEntry};

    fn sample_header() -> ManifestHeader {
        ManifestHeader {
            host: "testhost".to_string(),
            path: "/tmp/test".to_string(),
            depth: 6,
            date: "2026-03-28T15:30:00".to_string(),
            total_files: 3,
            total_dirs: 2,
            root_hash: "a1b2c3d4e5f67890a1b2c3d4e5f67890".to_string(),
        }
    }

    fn sample_entries() -> Vec<FlatEntry> {
        vec![
            FlatEntry {
                entry_type: EntryType::Dir,
                value: "a1b2c3d4e5f67890a1b2c3d4e5f67890".to_string(),
                path: "./".to_string(),
            },
            FlatEntry {
                entry_type: EntryType::Blake3,
                value: "deadbeefdeadbeefdeadbeefdeadbeef".to_string(),
                path: "./file.txt".to_string(),
            },
            FlatEntry {
                entry_type: EntryType::Dir,
                value: "cafebabecafebabecafebabecafebabe".to_string(),
                path: "./subdir/".to_string(),
            },
        ]
    }

    #[test]
    fn write_then_parse_round_trip() {
        let header = sample_header();
        let entries = sample_entries();

        let mut buf = Vec::new();
        write_manifest(&mut buf, &header, &entries).unwrap();

        let cursor = io::Cursor::new(buf);
        let (parsed_header, parsed_entries) = parse_manifest(io::BufReader::new(cursor)).unwrap();

        assert_eq!(parsed_header.host, "testhost");
        assert_eq!(parsed_header.path, "/tmp/test");
        assert_eq!(parsed_header.depth, 6);
        assert_eq!(parsed_header.total_files, 3);
        assert_eq!(parsed_header.total_dirs, 2);
        assert_eq!(parsed_header.root_hash, "a1b2c3d4e5f67890a1b2c3d4e5f67890");

        assert_eq!(parsed_entries.len(), 3);
    }

    #[test]
    fn parse_blake3_entry() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 1\n# total_dirs: 0\n# root: abc123\nblake3:deadbeefdeadbeefdeadbeefdeadbeef  ./file.txt\n";

        let (_, entries) = parse_manifest(io::BufReader::new(manifest.as_bytes())).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, "blake3");
        assert_eq!(entries[0].value, "deadbeefdeadbeefdeadbeefdeadbeef");
        assert_eq!(entries[0].path, "./file.txt");
    }

    #[test]
    fn parse_dataless_entry() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 1\n# total_dirs: 0\n# root: abc123\ndataless:12345  ./evicted.dat\n";

        let (_, entries) = parse_manifest(io::BufReader::new(manifest.as_bytes())).unwrap();
        assert_eq!(entries[0].entry_type, "dataless");
        assert_eq!(entries[0].value, "12345");
    }

    #[test]
    fn parse_error_entry() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 1\n# total_dirs: 0\n# root: abc123\nerror:permission denied  ./locked.db\n";

        let (_, entries) = parse_manifest(io::BufReader::new(manifest.as_bytes())).unwrap();
        assert_eq!(entries[0].entry_type, "error");
        assert_eq!(entries[0].value, "permission denied");
    }

    #[test]
    fn parse_symlink_entry() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 1\n# total_dirs: 0\n# root: abc123\nsymlink:/target/path  ./link\n";

        let (_, entries) = parse_manifest(io::BufReader::new(manifest.as_bytes())).unwrap();
        assert_eq!(entries[0].entry_type, "symlink");
        assert_eq!(entries[0].value, "/target/path");
    }

    #[test]
    fn parse_rejects_malformed_data_line() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 0\n# total_dirs: 0\n# root: abc\nthis is not valid\n";

        let result = parse_manifest(io::BufReader::new(manifest.as_bytes()));
        assert!(result.is_err());
    }

    #[test]
    fn parse_ignores_unknown_header_fields() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 0\n# total_dirs: 0\n# root: abc\n# unknown_field: whatever\n";

        let result = parse_manifest(io::BufReader::new(manifest.as_bytes()));
        assert!(result.is_ok());
    }

    #[test]
    fn parse_empty_manifest_header_only() {
        let manifest = "# sumpig fingerprint\n# version: 1\n# host: h\n# path: /p\n# depth: 6\n# date: 2026-01-01T00:00:00\n# total_files: 0\n# total_dirs: 0\n# root: abc\n";

        let (header, entries) = parse_manifest(io::BufReader::new(manifest.as_bytes())).unwrap();
        assert_eq!(header.host, "h");
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn write_manifest_format() {
        let header = sample_header();
        let entries = sample_entries();

        let mut buf = Vec::new();
        write_manifest(&mut buf, &header, &entries).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // Should start with header comments.
        assert!(output.starts_with("# sumpig fingerprint\n"));
        assert!(output.contains("# host: testhost\n"));
        assert!(output.contains("# depth: 6\n"));

        // Data lines use two-space separator.
        assert!(output.contains("blake3:deadbeefdeadbeefdeadbeefdeadbeef  ./file.txt\n"));
        assert!(output.contains("dir:cafebabecafebabecafebabecafebabe  ./subdir/\n"));
    }

    #[test]
    fn get_hostname_returns_nonempty() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }

    #[test]
    fn get_iso_date_format() {
        let date = get_iso_date();
        // Should look like 2026-03-28T15:30:00 (19 chars).
        assert_eq!(date.len(), 19, "ISO date should be 19 chars: {date}");
        assert_eq!(&date[4..5], "-");
        assert_eq!(&date[7..8], "-");
        assert_eq!(&date[10..11], "T");
        assert_eq!(&date[13..14], ":");
        assert_eq!(&date[16..17], ":");
    }
}
