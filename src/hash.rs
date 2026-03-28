use std::fs;
use std::io::Read;
use std::path::Path;

/// Buffer size for file reading (64KB, matching ripgrep's default).
const READ_BUFFER_SIZE: usize = 64 * 1024;

/// File size threshold for multi-threaded hashing with blake3's update_rayon().
const LARGE_FILE_THRESHOLD: u64 = 1024 * 1024; // 1MB

/// Result of hashing a single file.
#[derive(Debug)]
pub enum FileHash {
    /// Successful BLAKE3 hash of file content.
    Blake3([u8; 32]),
    /// macOS dataless file (iCloud-evicted), stores file size.
    Dataless(u64),
    /// File could not be read, stores error description.
    Error(String),
    /// Symbolic link, stores target path.
    Symlink(String),
}

/// Hash a file at the given path.
///
/// Checks (in order):
/// 1. Is it a symlink? -> return Symlink(target)
/// 2. Is it a macOS dataless file? -> return Dataless(size)
/// 3. Read and hash with BLAKE3 -> return Blake3(hash)
/// 4. On any I/O error -> return Error(description)
pub fn hash_file(path: &Path) -> FileHash {
    // Use symlink_metadata (lstat) to check symlink status without following.
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return FileHash::Error(e.to_string()),
    };

    if metadata.is_symlink() {
        return match fs::read_link(path) {
            Ok(target) => FileHash::Symlink(target.to_string_lossy().into_owned()),
            Err(e) => FileHash::Error(e.to_string()),
        };
    }

    // Check for macOS dataless flag (iCloud-evicted files).
    #[cfg(target_os = "macos")]
    {
        if let Some(size) = check_dataless(path) {
            return FileHash::Dataless(size);
        }
    }

    // Hash the file contents with BLAKE3.
    match hash_file_contents(path, metadata.len()) {
        Ok(hash) => FileHash::Blake3(hash),
        Err(e) => FileHash::Error(e.to_string()),
    }
}

/// Check if a file has the macOS SF_DATALESS flag set.
/// Returns Some(file_size) if dataless, None otherwise.
#[cfg(target_os = "macos")]
fn check_dataless(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    const SF_DATALESS: u32 = 0x40000000;

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat_buf: libc::stat = unsafe { std::mem::zeroed() };

    let ret = unsafe { libc::lstat(c_path.as_ptr(), &mut stat_buf) };
    if ret != 0 {
        return None;
    }

    if stat_buf.st_flags & SF_DATALESS != 0 {
        Some(stat_buf.st_size as u64)
    } else {
        None
    }
}

/// Read and hash file contents with BLAKE3.
/// Uses buffered reads for small files and update_rayon() for large files.
fn hash_file_contents(path: &Path, file_size: u64) -> std::io::Result<[u8; 32]> {
    if file_size > LARGE_FILE_THRESHOLD {
        // Large file: read into memory and use multi-threaded hashing.
        let data = fs::read(path)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update_rayon(&data);
        Ok(*hasher.finalize().as_bytes())
    } else {
        // Small file: buffered single-threaded read.
        let mut file = fs::File::open(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0u8; READ_BUFFER_SIZE];
        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        Ok(*hasher.finalize().as_bytes())
    }
}

/// Format a 32-byte hash as truncated hex (32 hex chars = 128 bits).
pub fn hash_to_hex(hash: &[u8; 32]) -> String {
    let mut hex = String::with_capacity(32);
    for byte in &hash[..16] {
        use std::fmt::Write;
        write!(hex, "{byte:02x}").unwrap();
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs as unix_fs;
    use tempfile::TempDir;

    #[test]
    fn hash_known_content() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("hello.txt");
        fs::write(&file, b"hello world").unwrap();

        let expected = *blake3::hash(b"hello world").as_bytes();

        match hash_file(&file) {
            FileHash::Blake3(hash) => assert_eq!(hash, expected),
            other => panic!("expected Blake3, got {other:?}"),
        }
    }

    #[test]
    fn hash_empty_file() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("empty.txt");
        fs::write(&file, b"").unwrap();

        let expected = *blake3::hash(b"").as_bytes();

        match hash_file(&file) {
            FileHash::Blake3(hash) => assert_eq!(hash, expected),
            other => panic!("expected Blake3, got {other:?}"),
        }
    }

    #[test]
    fn hash_symlink_returns_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target.txt");
        fs::write(&target, b"content").unwrap();
        let link = dir.path().join("link.txt");
        unix_fs::symlink(&target, &link).unwrap();

        match hash_file(&link) {
            FileHash::Symlink(t) => assert_eq!(t, target.to_string_lossy()),
            other => panic!("expected Symlink, got {other:?}"),
        }
    }

    #[test]
    fn hash_nonexistent_returns_error() {
        let path = Path::new("/nonexistent/file.txt");
        match hash_file(path) {
            FileHash::Error(_) => {}
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn hash_to_hex_format() {
        let hash = *blake3::hash(b"test").as_bytes();
        let hex = hash_to_hex(&hash);
        assert_eq!(hex.len(), 32, "should be 32 hex chars (128 bits)");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(hex.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn hash_to_hex_known_value() {
        // blake3("") has a known hash; verify our truncation matches.
        let hash = *blake3::hash(b"").as_bytes();
        let hex = hash_to_hex(&hash);
        let full_hex = blake3::hash(b"").to_hex();
        assert_eq!(hex, &full_hex.as_str()[..32]);
    }

    #[test]
    fn internal_hash_is_full_256_bits() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("data.txt");
        fs::write(&file, b"some content").unwrap();

        match hash_file(&file) {
            FileHash::Blake3(hash) => assert_eq!(hash.len(), 32, "internal hash must be 32 bytes"),
            other => panic!("expected Blake3, got {other:?}"),
        }
    }

    #[test]
    fn hash_dangling_symlink_returns_symlink() {
        let dir = TempDir::new().unwrap();
        let link = dir.path().join("dangling");
        unix_fs::symlink("/nonexistent/target", &link).unwrap();

        match hash_file(&link) {
            FileHash::Symlink(t) => assert_eq!(t, "/nonexistent/target"),
            other => panic!("expected Symlink, got {other:?}"),
        }
    }
}
