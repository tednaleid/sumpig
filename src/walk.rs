use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct WalkOptions {
    /// If true, apply the default ignore list. If false (--no-ignore), hash everything.
    pub use_default_ignores: bool,
    /// Number of threads for parallel walking. 0 means use rayon default (num CPUs).
    pub num_threads: usize,
}

pub struct WalkEntry {
    /// Path relative to the walk root.
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// An error encountered during directory walking.
#[derive(Clone)]
pub struct WalkError {
    /// Path relative to the walk root (if known).
    pub path: PathBuf,
    /// Human-readable error description.
    pub reason: String,
}

/// Result of walking a directory tree.
pub struct WalkResult {
    pub entries: Vec<WalkEntry>,
    pub errors: Vec<WalkError>,
}

/// Directories to ignore (not hashed, not listed).
pub const IGNORE_DIRS: &[&str] = &[
    "node_modules",
    ".venv",
    "venv",
    "target",
    "__pycache__",
    "build",
    "dist",
    ".Trash",
    ".sumpig-fingerprints",
];

/// Files to ignore.
pub const IGNORE_FILES: &[&str] = &[".DS_Store", ".localized"];

/// File extensions to ignore.
pub const IGNORE_EXTENSIONS: &[&str] = &["nosync"];

/// Walk a directory tree, returning sorted entries and any errors encountered.
/// Applies default ignore list unless options.use_default_ignores is false.
pub fn walk_directory(root: &Path, options: &WalkOptions) -> WalkResult {
    let ignore = options.use_default_ignores;
    let root_buf = root.to_path_buf();
    let callback_errors: Arc<Mutex<Vec<WalkError>>> = Arc::new(Mutex::new(Vec::new()));
    let callback_errors_ref = Arc::clone(&callback_errors);

    let parallelism = if options.num_threads == 1 {
        jwalk::Parallelism::Serial
    } else if options.num_threads == 0 {
        jwalk::Parallelism::RayonDefaultPool {
            busy_timeout: std::time::Duration::from_secs(1),
        }
    } else {
        jwalk::Parallelism::RayonNewPool(options.num_threads)
    };

    let walker = jwalk::WalkDir::new(root)
        .parallelism(parallelism)
        .skip_hidden(false)
        .follow_links(false)
        .process_read_dir(move |_depth, _path, _state, children| {
            // Capture errors from directory reading before any filtering.
            let root = &root_buf;
            let mut errs = callback_errors_ref.lock().unwrap();
            children.retain(|entry_result| {
                let Ok(entry) = entry_result else {
                    // Extract error info before dropping.
                    let err = entry_result.as_ref().unwrap_err();
                    if let Some(abs_path) = err.path() {
                        let rel = abs_path
                            .strip_prefix(root)
                            .unwrap_or(abs_path)
                            .to_path_buf();
                        errs.push(WalkError {
                            path: rel,
                            reason: err.to_string(),
                        });
                    }
                    return false;
                };

                if !ignore {
                    return true;
                }

                let name = entry.file_name().to_string_lossy();
                let ft = entry.file_type();

                // Ignore directories by name.
                if ft.is_dir() && IGNORE_DIRS.contains(&name.as_ref()) {
                    return false;
                }

                // Ignore files by name or extension.
                if ft.is_file() {
                    if IGNORE_FILES.contains(&name.as_ref()) {
                        return false;
                    }
                    if let Some(ext) = Path::new(name.as_ref()).extension()
                        && IGNORE_EXTENSIONS.contains(&ext.to_string_lossy().as_ref())
                    {
                        return false;
                    }
                }

                true
            });
        });

    let mut entries: Vec<WalkEntry> = Vec::new();
    let mut loop_errors: Vec<WalkError> = Vec::new();

    for entry in walker {
        match entry {
            Err(err) => {
                if let Some(abs_path) = err.path() {
                    let rel = abs_path
                        .strip_prefix(root)
                        .unwrap_or(abs_path)
                        .to_path_buf();
                    loop_errors.push(WalkError {
                        path: rel,
                        reason: err.to_string(),
                    });
                }
            }
            Ok(entry) => {
                let path = entry.path();

                // Skip the root directory itself.
                if path == root {
                    continue;
                }

                let Ok(relative) = path.strip_prefix(root) else {
                    continue;
                };

                // Check for readdir errors (e.g., permission denied reading
                // a directory's contents). jwalk stores these on the DirEntry
                // rather than yielding them as Err values.
                if let Some(ref err) = entry.read_children_error {
                    loop_errors.push(WalkError {
                        path: relative.to_path_buf(),
                        reason: err.to_string(),
                    });
                }

                let file_type = entry.file_type;
                let is_symlink = file_type.is_symlink();
                let is_dir = file_type.is_dir();

                entries.push(WalkEntry {
                    path: relative.to_path_buf(),
                    is_dir,
                    is_symlink,
                });
            }
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));

    // Merge errors from callback and main loop.
    let mut errors = Arc::try_unwrap(callback_errors)
        .map(|mutex| mutex.into_inner().unwrap())
        .unwrap_or_else(|arc| arc.lock().unwrap().clone());
    errors.append(&mut loop_errors);
    errors.sort_by(|a, b| a.path.cmp(&b.path));

    WalkResult { entries, errors }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs as unix_fs;
    use tempfile::TempDir;

    fn default_options() -> WalkOptions {
        WalkOptions {
            use_default_ignores: true,
            num_threads: 1,
        }
    }

    fn no_ignore_options() -> WalkOptions {
        WalkOptions {
            use_default_ignores: false,
            num_threads: 1,
        }
    }

    /// Create a basic tree for testing:
    /// root/
    ///   a.txt
    ///   dir1/
    ///     b.txt
    ///   dir2/
    ///     c.txt
    fn create_basic_tree(dir: &TempDir) {
        fs::write(dir.path().join("a.txt"), "a").unwrap();
        fs::create_dir(dir.path().join("dir1")).unwrap();
        fs::write(dir.path().join("dir1/b.txt"), "b").unwrap();
        fs::create_dir(dir.path().join("dir2")).unwrap();
        fs::write(dir.path().join("dir2/c.txt"), "c").unwrap();
    }

    #[test]
    fn walk_basic_tree_sorted() {
        let dir = TempDir::new().unwrap();
        create_basic_tree(&dir);

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        // Should be sorted and include both files and directories.
        assert_eq!(
            paths,
            vec!["a.txt", "dir1", "dir1/b.txt", "dir2", "dir2/c.txt"]
        );
    }

    #[test]
    fn walk_identifies_dirs_and_files() {
        let dir = TempDir::new().unwrap();
        create_basic_tree(&dir);

        let result = walk_directory(dir.path(), &default_options());

        let file_entry = result
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("a.txt"))
            .unwrap();
        assert!(!file_entry.is_dir);

        let dir_entry = result
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("dir1"))
            .unwrap();
        assert!(dir_entry.is_dir);
    }

    #[test]
    fn walk_ignores_node_modules() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/package.json"), "{}").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_ds_store() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".DS_Store"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_nosync_extension() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("data.nosync"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_sumpig_fingerprints() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".sumpig-fingerprints")).unwrap();
        fs::write(dir.path().join(".sumpig-fingerprints/host.txt"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_includes_git_directories() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/objects/abc"), "data").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert!(paths.contains(&".git"));
        assert!(paths.contains(&".git/objects"));
        assert!(paths.contains(&".git/objects/abc"));
    }

    #[test]
    fn walk_no_ignore_includes_everything() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        fs::write(dir.path().join("node_modules/pkg.json"), "{}").unwrap();
        fs::write(dir.path().join(".DS_Store"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &no_ignore_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert!(paths.contains(&"node_modules"));
        assert!(paths.contains(&"node_modules/pkg.json"));
        assert!(paths.contains(&".DS_Store"));
        assert!(paths.contains(&"keep.txt"));
    }

    #[test]
    fn walk_symlinks_not_followed() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("real_dir")).unwrap();
        fs::write(dir.path().join("real_dir/file.txt"), "content").unwrap();

        // Symlink to a directory -- should appear as an entry but NOT be traversed.
        unix_fs::symlink(dir.path().join("real_dir"), dir.path().join("link_dir")).unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        // link_dir should appear but link_dir/file.txt should NOT.
        assert!(paths.contains(&"link_dir"));
        assert!(!paths.contains(&"link_dir/file.txt"));

        let link_entry = result
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("link_dir"))
            .unwrap();
        assert!(link_entry.is_symlink);
    }

    #[test]
    fn walk_empty_directory_included() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("empty")).unwrap();
        fs::write(dir.path().join("file.txt"), "content").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert!(paths.contains(&"empty"));
        let empty_entry = result
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("empty"))
            .unwrap();
        assert!(empty_entry.is_dir);
    }

    #[test]
    fn walk_unreadable_directory_produces_error() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("readable.txt"), "ok").unwrap();
        let forbidden = dir.path().join("forbidden");
        fs::create_dir(&forbidden).unwrap();
        fs::write(forbidden.join("secret.txt"), "hidden").unwrap();

        // Remove read+execute permission on the directory.
        fs::set_permissions(&forbidden, fs::Permissions::from_mode(0o000)).unwrap();

        let result = walk_directory(dir.path(), &default_options());

        // Restore permissions so TempDir cleanup works.
        fs::set_permissions(&forbidden, fs::Permissions::from_mode(0o755)).unwrap();

        // The forbidden directory itself should appear as an entry (we can stat it),
        // but reading its contents should produce an error.
        assert!(
            !result.errors.is_empty(),
            "expected walk errors for unreadable directory, got none"
        );
        // The readable file should still be found.
        assert!(
            result
                .entries
                .iter()
                .any(|e| e.path.to_str() == Some("readable.txt"))
        );
    }

    #[test]
    fn walk_ignores_multiple_default_dirs() {
        let dir = TempDir::new().unwrap();
        for skip_dir in &[
            "node_modules",
            ".venv",
            "target",
            "__pycache__",
            "build",
            "dist",
        ] {
            fs::create_dir(dir.path().join(skip_dir)).unwrap();
            fs::write(dir.path().join(skip_dir).join("file.txt"), "").unwrap();
        }
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let result = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = result
            .entries
            .iter()
            .map(|e| e.path.to_str().unwrap())
            .collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }
}
