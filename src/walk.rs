use std::path::{Path, PathBuf};

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

/// Walk a directory tree, returning sorted entries.
/// Applies default ignore list unless options.use_default_ignores is false.
pub fn walk_directory(root: &Path, options: &WalkOptions) -> Vec<WalkEntry> {
    let ignore = options.use_default_ignores;

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
            if ignore {
                children.retain(|entry_result| {
                    let Ok(entry) = entry_result else {
                        return false;
                    };
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
            }
        });

    let mut entries: Vec<WalkEntry> = Vec::new();

    for entry in walker {
        let Ok(entry) = entry else { continue };
        let path = entry.path();

        // Skip the root directory itself.
        if path == root {
            continue;
        }

        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };

        let file_type = entry.file_type;
        let is_symlink = file_type.is_symlink();
        let is_dir = file_type.is_dir();

        entries.push(WalkEntry {
            path: relative.to_path_buf(),
            is_dir,
            is_symlink,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
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

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

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

        let entries = walk_directory(dir.path(), &default_options());

        let file_entry = entries
            .iter()
            .find(|e| e.path.to_str() == Some("a.txt"))
            .unwrap();
        assert!(!file_entry.is_dir);

        let dir_entry = entries
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

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_ds_store() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join(".DS_Store"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_nosync_extension() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("data.nosync"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_ignores_sumpig_fingerprints() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".sumpig-fingerprints")).unwrap();
        fs::write(dir.path().join(".sumpig-fingerprints/host.txt"), "").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn walk_includes_git_directories() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".git/objects")).unwrap();
        fs::write(dir.path().join(".git/objects/abc"), "data").unwrap();
        fs::write(dir.path().join("keep.txt"), "keep").unwrap();

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

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

        let entries = walk_directory(dir.path(), &no_ignore_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

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

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        // link_dir should appear but link_dir/file.txt should NOT.
        assert!(paths.contains(&"link_dir"));
        assert!(!paths.contains(&"link_dir/file.txt"));

        let link_entry = entries
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

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert!(paths.contains(&"empty"));
        let empty_entry = entries
            .iter()
            .find(|e| e.path.to_str() == Some("empty"))
            .unwrap();
        assert!(empty_entry.is_dir);
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

        let entries = walk_directory(dir.path(), &default_options());
        let paths: Vec<&str> = entries.iter().map(|e| e.path.to_str().unwrap()).collect();

        assert_eq!(paths, vec!["keep.txt"]);
    }
}
