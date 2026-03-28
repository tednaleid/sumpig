use std::path::PathBuf;

use crate::hash::{FileHash, hash_to_hex};

/// A flattened entry for manifest output.
#[derive(Debug, Clone)]
pub struct FlatEntry {
    pub entry_type: EntryType,
    /// Hex hash, file size, error reason, or symlink target.
    pub value: String,
    /// Relative path with ./ prefix.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EntryType {
    Blake3,
    Dataless,
    Error,
    Symlink,
    /// Directory with computed Merkle hash.
    Dir,
}

/// Compute Merkle directory hashes and produce manifest entries from sorted file entries.
///
/// Uses a streaming stack-based algorithm: no tree structure is built in memory.
/// Input entries MUST be sorted by path. Only file/symlink entries are provided as
/// input; directory entries and their hashes are synthesized from the path structure.
///
/// Returns (flat_entries sorted by path, root_hash).
pub fn compute_manifest(
    sorted_entries: &[(PathBuf, FileHash)],
    max_depth: usize,
) -> (Vec<FlatEntry>, [u8; 32]) {
    // Stack of (directory_path_components, blake3::Hasher).
    // The root is represented by an empty Vec of components.
    let mut stack: Vec<(Vec<String>, blake3::Hasher)> = vec![(vec![], blake3::Hasher::new())];
    let mut output: Vec<FlatEntry> = Vec::new();

    for (path, file_hash) in sorted_entries {
        let components: Vec<String> = path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();

        // The file name is the last component; the directory is everything before it.
        let (dir_components, file_name) = components.split_at(components.len() - 1);
        let file_name = &file_name[0];

        // Pop directories we've left.
        while stack.len() > 1 && !is_prefix(&stack.last().unwrap().0, dir_components) {
            let (dir_comps, hasher) = stack.pop().unwrap();
            let dir_hash = *hasher.finalize().as_bytes();
            let dir_name = dir_comps.last().unwrap();

            // Feed this directory's hash into its parent.
            let parent = stack.last_mut().unwrap();
            feed_child(&mut parent.1, dir_name, &dir_hash);

            // Emit directory entry if within depth.
            // Directory depth = number of components (root is depth 0).
            if dir_comps.len() <= max_depth {
                output.push(FlatEntry {
                    entry_type: EntryType::Dir,
                    value: hash_to_hex(&dir_hash),
                    path: components_to_path(&dir_comps, true),
                });
            }
        }

        // Push new directories we need to enter.
        let current_depth = stack.len() - 1;
        for i in current_depth..dir_components.len() {
            let new_dir_comps: Vec<String> = dir_components[..=i].to_vec();
            stack.push((new_dir_comps, blake3::Hasher::new()));
        }

        // Compute the effective hash for this entry.
        let effective_hash = match file_hash {
            FileHash::Blake3(h) => *h,
            FileHash::Dataless(size) => synthetic_hash("dataless", &size.to_string()),
            FileHash::Error(reason) => synthetic_hash("error", reason),
            FileHash::Symlink(target) => synthetic_hash("symlink", target),
        };

        // Feed this file's hash into the current directory's hasher.
        let current_dir = stack.last_mut().unwrap();
        feed_child(&mut current_dir.1, file_name, &effective_hash);

        // Emit file entry if within depth.
        // File depth = number of components (e.g., "dir/file.txt" = depth 2).
        let file_depth = components.len();
        if file_depth <= max_depth {
            let (entry_type, value) = match file_hash {
                FileHash::Blake3(h) => (EntryType::Blake3, hash_to_hex(h)),
                FileHash::Dataless(size) => (EntryType::Dataless, size.to_string()),
                FileHash::Error(reason) => (EntryType::Error, reason.clone()),
                FileHash::Symlink(target) => (EntryType::Symlink, target.clone()),
            };
            output.push(FlatEntry {
                entry_type,
                value,
                path: components_to_path(&components, false),
            });
        }
    }

    // Finalize remaining directories on the stack.
    while let Some((dir_comps, hasher)) = stack.pop() {
        let dir_hash = *hasher.finalize().as_bytes();

        if let Some(parent) = stack.last_mut() {
            let dir_name = dir_comps.last().unwrap();
            feed_child(&mut parent.1, dir_name, &dir_hash);
        }

        // Always emit directory entries for remaining stack items, subject to depth.
        if dir_comps.len() <= max_depth || dir_comps.is_empty() {
            output.push(FlatEntry {
                entry_type: EntryType::Dir,
                value: hash_to_hex(&dir_hash),
                path: components_to_path(&dir_comps, true),
            });
        }

        // If this was the root (empty components), we're done.
        if dir_comps.is_empty() {
            // Sort output by path for deterministic ordering.
            output.sort_by(|a, b| a.path.cmp(&b.path));
            return (output, dir_hash);
        }
    }

    // Should not reach here -- the root is always on the stack.
    unreachable!("stack should always contain root")
}

/// Check if `prefix` is a prefix of `full`.
fn is_prefix(prefix: &[String], full: &[String]) -> bool {
    if prefix.len() > full.len() {
        return false;
    }
    prefix.iter().zip(full.iter()).all(|(a, b)| a == b)
}

/// Feed a child entry (name + hash) into a directory hasher.
fn feed_child(hasher: &mut blake3::Hasher, name: &str, hash: &[u8; 32]) {
    hasher.update(name.as_bytes());
    hasher.update(b"\0");
    hasher.update(hash);
}

/// Convert path components to a display path.
/// Empty components = "./"
/// Directories get a trailing "/".
fn components_to_path(components: &[String], is_dir: bool) -> String {
    if components.is_empty() {
        return "./".to_string();
    }
    let joined = components.join("/");
    if is_dir {
        format!("./{joined}/")
    } else {
        format!("./{joined}")
    }
}

/// Compute a synthetic hash for non-Blake3 entries (dataless, error, symlink).
/// Used to include these entries in directory hash computation.
pub fn synthetic_hash(entry_type: &str, value: &str) -> [u8; 32] {
    let input = format!("{entry_type}:{value}");
    *blake3::hash(input.as_bytes()).as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{FileHash, hash_to_hex};

    /// Helper: create a Blake3 FileHash from content bytes.
    fn blake3_hash(content: &[u8]) -> FileHash {
        FileHash::Blake3(*blake3::hash(content).as_bytes())
    }

    #[test]
    fn basic_tree_produces_correct_entries() {
        // Tree: ./a.txt, ./dir/b.txt
        let entries = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"content_a")),
            (PathBuf::from("dir/b.txt"), blake3_hash(b"content_b")),
        ];

        let (flat, root_hash) = compute_manifest(&entries, 10);

        // Should have: ./ (root dir), a.txt, dir/, dir/b.txt
        let paths: Vec<&str> = flat.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"./"));
        assert!(paths.contains(&"./a.txt"));
        assert!(paths.contains(&"./dir/"));
        assert!(paths.contains(&"./dir/b.txt"));
        assert_eq!(flat.len(), 4);

        // Root hash should be non-zero.
        assert_ne!(root_hash, [0u8; 32]);
    }

    #[test]
    fn deterministic_regardless_of_input_order() {
        let entries_a = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"a")),
            (PathBuf::from("b.txt"), blake3_hash(b"b")),
        ];
        let entries_b = vec![
            (PathBuf::from("b.txt"), blake3_hash(b"b")),
            (PathBuf::from("a.txt"), blake3_hash(b"a")),
        ];

        // Both should be sorted before calling, so we sort here.
        let mut entries_b_sorted = entries_b;
        entries_b_sorted.sort_by(|a, b| a.0.cmp(&b.0));

        let (_, hash_a) = compute_manifest(&entries_a, 10);
        let (_, hash_b) = compute_manifest(&entries_b_sorted, 10);
        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn changing_one_file_changes_root_hash() {
        let entries_original = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"original")),
            (PathBuf::from("b.txt"), blake3_hash(b"same")),
        ];
        let entries_modified = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"modified")),
            (PathBuf::from("b.txt"), blake3_hash(b"same")),
        ];

        let (_, hash_original) = compute_manifest(&entries_original, 10);
        let (_, hash_modified) = compute_manifest(&entries_modified, 10);
        assert_ne!(hash_original, hash_modified);
    }

    #[test]
    fn changing_file_changes_parent_but_not_sibling_dir() {
        // Tree: ./dir_a/file.txt, ./dir_b/file.txt
        let entries_original = vec![
            (PathBuf::from("dir_a/file.txt"), blake3_hash(b"original")),
            (PathBuf::from("dir_b/file.txt"), blake3_hash(b"same")),
        ];
        let entries_modified = vec![
            (PathBuf::from("dir_a/file.txt"), blake3_hash(b"modified")),
            (PathBuf::from("dir_b/file.txt"), blake3_hash(b"same")),
        ];

        let (flat_orig, _) = compute_manifest(&entries_original, 10);
        let (flat_mod, _) = compute_manifest(&entries_modified, 10);

        let get_hash = |flat: &[FlatEntry], path: &str| -> String {
            flat.iter().find(|e| e.path == path).unwrap().value.clone()
        };

        // dir_a hash should change.
        assert_ne!(
            get_hash(&flat_orig, "./dir_a/"),
            get_hash(&flat_mod, "./dir_a/")
        );
        // dir_b hash should NOT change.
        assert_eq!(
            get_hash(&flat_orig, "./dir_b/"),
            get_hash(&flat_mod, "./dir_b/")
        );
    }

    #[test]
    fn empty_input_produces_root_only() {
        let entries: Vec<(PathBuf, FileHash)> = vec![];
        let (flat, root_hash) = compute_manifest(&entries, 10);

        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].path, "./");
        assert_ne!(root_hash, [0u8; 32]);
    }

    #[test]
    fn depth_zero_only_root() {
        let entries = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"a")),
            (PathBuf::from("dir/b.txt"), blake3_hash(b"b")),
        ];

        let (flat, _) = compute_manifest(&entries, 0);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].path, "./");
    }

    #[test]
    fn depth_one_root_plus_children() {
        let entries = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"a")),
            (PathBuf::from("dir/b.txt"), blake3_hash(b"b")),
        ];

        let (flat, _) = compute_manifest(&entries, 1);
        let paths: Vec<&str> = flat.iter().map(|e| e.path.as_str()).collect();

        // Should include root, a.txt, dir/ but NOT dir/b.txt
        assert!(paths.contains(&"./"));
        assert!(paths.contains(&"./a.txt"));
        assert!(paths.contains(&"./dir/"));
        assert!(!paths.contains(&"./dir/b.txt"));
    }

    #[test]
    fn same_root_hash_regardless_of_depth() {
        let entries = vec![
            (PathBuf::from("a.txt"), blake3_hash(b"a")),
            (PathBuf::from("dir/b.txt"), blake3_hash(b"b")),
            (PathBuf::from("dir/sub/c.txt"), blake3_hash(b"c")),
        ];

        let (_, hash_depth_0) = compute_manifest(&entries, 0);
        let (_, hash_depth_1) = compute_manifest(&entries, 1);
        let (_, hash_depth_6) = compute_manifest(&entries, 6);
        let (_, hash_depth_100) = compute_manifest(&entries, 100);

        assert_eq!(hash_depth_0, hash_depth_1);
        assert_eq!(hash_depth_1, hash_depth_6);
        assert_eq!(hash_depth_6, hash_depth_100);
    }

    #[test]
    fn dataless_contributes_to_dir_hash() {
        let entries_normal = vec![(PathBuf::from("file.txt"), blake3_hash(b"content"))];
        let entries_dataless = vec![(PathBuf::from("file.txt"), FileHash::Dataless(12345))];

        let (_, hash_normal) = compute_manifest(&entries_normal, 10);
        let (_, hash_dataless) = compute_manifest(&entries_dataless, 10);
        assert_ne!(hash_normal, hash_dataless);
    }

    #[test]
    fn error_contributes_to_dir_hash() {
        let entries_normal = vec![(PathBuf::from("file.txt"), blake3_hash(b"content"))];
        let entries_error = vec![(
            PathBuf::from("file.txt"),
            FileHash::Error("permission denied".to_string()),
        )];

        let (_, hash_normal) = compute_manifest(&entries_normal, 10);
        let (_, hash_error) = compute_manifest(&entries_error, 10);
        assert_ne!(hash_normal, hash_error);
    }

    #[test]
    fn symlink_contributes_to_dir_hash() {
        let entries_normal = vec![(PathBuf::from("link"), blake3_hash(b"content"))];
        let entries_symlink = vec![(
            PathBuf::from("link"),
            FileHash::Symlink("/target/path".to_string()),
        )];

        let (_, hash_normal) = compute_manifest(&entries_normal, 10);
        let (_, hash_symlink) = compute_manifest(&entries_symlink, 10);
        assert_ne!(hash_normal, hash_symlink);
    }

    #[test]
    fn flat_entries_have_correct_types() {
        let entries = vec![
            (
                PathBuf::from("error.txt"),
                FileHash::Error("read error".to_string()),
            ),
            (PathBuf::from("evicted.dat"), FileHash::Dataless(99)),
            (
                PathBuf::from("link"),
                FileHash::Symlink("/target".to_string()),
            ),
            (PathBuf::from("normal.txt"), blake3_hash(b"data")),
        ];

        let (flat, _) = compute_manifest(&entries, 10);

        let find = |path: &str| flat.iter().find(|e| e.path == path).unwrap();

        assert_eq!(find("./normal.txt").entry_type, EntryType::Blake3);
        assert_eq!(find("./evicted.dat").entry_type, EntryType::Dataless);
        assert_eq!(find("./evicted.dat").value, "99");
        assert_eq!(find("./error.txt").entry_type, EntryType::Error);
        assert_eq!(find("./error.txt").value, "read error");
        assert_eq!(find("./link").entry_type, EntryType::Symlink);
        assert_eq!(find("./link").value, "/target");
        assert_eq!(find("./").entry_type, EntryType::Dir);
    }

    #[test]
    fn directory_entries_have_hex_hashes() {
        let entries = vec![(PathBuf::from("file.txt"), blake3_hash(b"content"))];

        let (flat, root_hash) = compute_manifest(&entries, 10);
        let root_entry = flat.iter().find(|e| e.path == "./").unwrap();

        assert_eq!(root_entry.entry_type, EntryType::Dir);
        assert_eq!(root_entry.value, hash_to_hex(&root_hash));
    }
}
