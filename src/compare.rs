use std::collections::{HashMap, HashSet};

use crate::manifest::ManifestEntry;
use crate::merkle::EntryType;

/// Result of comparing two manifests.
pub struct CompareResult {
    pub identical: bool,
    pub depth: usize,
    pub host1: String,
    pub host2: String,
    pub changed_dirs: Vec<ChangedEntry>,
    pub changed_files: Vec<ChangedEntry>,
    pub only_in_first: Vec<String>,
    pub only_in_second: Vec<String>,
    pub dataless_warnings: HashSet<String>,
    pub error_warnings: HashSet<String>,
}

pub struct ChangedEntry {
    pub path: String,
    pub value1: String,
    pub value2: String,
}

/// Compare two sets of manifest entries using the Merkle tree property.
/// When a directory hash matches in both manifests, all children are skipped.
pub fn compare_manifests(
    entries1: &[ManifestEntry],
    entries2: &[ManifestEntry],
    host1: &str,
    host2: &str,
    depth: usize,
) -> CompareResult {
    let map1: HashMap<&str, &ManifestEntry> =
        entries1.iter().map(|e| (e.path.as_str(), e)).collect();
    let map2: HashMap<&str, &ManifestEntry> =
        entries2.iter().map(|e| (e.path.as_str(), e)).collect();

    // Collect directories with matching hashes (Merkle skip set).
    let mut skipped_prefixes: HashSet<&str> = HashSet::new();
    for e1 in entries1 {
        if e1.entry_type == EntryType::Dir
            && let Some(e2) = map2.get(e1.path.as_str())
            && e2.entry_type == EntryType::Dir
            && e1.value == e2.value
        {
            skipped_prefixes.insert(&e1.path);
        }
    }

    let is_skipped = |path: &str| -> bool {
        skipped_prefixes
            .iter()
            .any(|prefix| path != *prefix && path.starts_with(*prefix))
    };

    let mut changed_dirs = Vec::new();
    let mut changed_files = Vec::new();
    let mut only_in_first = Vec::new();
    let mut only_in_second = Vec::new();
    let mut dataless_warnings = HashSet::new();
    let mut error_warnings = HashSet::new();

    // Walk entries from manifest 1.
    for e1 in entries1 {
        if is_skipped(&e1.path) {
            continue;
        }

        // Collect warnings for dataless/error entries.
        if e1.entry_type == EntryType::Dataless {
            dataless_warnings.insert(e1.path.clone());
        }
        if e1.entry_type == EntryType::Error {
            error_warnings.insert(e1.path.clone());
        }

        match map2.get(e1.path.as_str()) {
            Some(e2) => {
                // Collect warnings for dataless/error on the other side.
                if e2.entry_type == EntryType::Dataless {
                    dataless_warnings.insert(e2.path.clone());
                }
                if e2.entry_type == EntryType::Error {
                    error_warnings.insert(e2.path.clone());
                }

                // Compare values.
                if e1.value != e2.value || e1.entry_type != e2.entry_type {
                    let changed = ChangedEntry {
                        path: e1.path.clone(),
                        value1: e1.value.clone(),
                        value2: e2.value.clone(),
                    };
                    if e1.entry_type == EntryType::Dir || e2.entry_type == EntryType::Dir {
                        changed_dirs.push(changed);
                    } else {
                        changed_files.push(changed);
                    }
                }
            }
            None => {
                only_in_first.push(e1.path.clone());
            }
        }
    }

    // Find entries only in manifest 2.
    for e2 in entries2 {
        if is_skipped(&e2.path) {
            continue;
        }
        if !map1.contains_key(e2.path.as_str()) {
            only_in_second.push(e2.path.clone());

            if e2.entry_type == EntryType::Dataless {
                dataless_warnings.insert(e2.path.clone());
            }
            if e2.entry_type == EntryType::Error {
                error_warnings.insert(e2.path.clone());
            }
        }
    }

    let identical = changed_dirs.is_empty()
        && changed_files.is_empty()
        && only_in_first.is_empty()
        && only_in_second.is_empty();

    CompareResult {
        identical,
        depth,
        host1: host1.to_string(),
        host2: host2.to_string(),
        changed_dirs,
        changed_files,
        only_in_first,
        only_in_second,
        dataless_warnings,
        error_warnings,
    }
}

/// Formatted comparison output split into stdout (data) and stderr (informational).
pub struct CompareReport {
    /// Prefixed path lines: `!` changed, `<` only-in-first, `>` only-in-second.
    pub stdout: String,
    /// Summary, warnings, and status messages.
    pub stderr: String,
}

/// Compute the depth of a path (number of components after stripping `./` prefix and trailing `/`).
fn path_depth(path: &str) -> usize {
    let trimmed = path
        .strip_prefix("./")
        .unwrap_or(path)
        .trim_end_matches('/');
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.split('/').count()
}

/// Format a CompareResult for terminal output.
///
/// Files always appear on stdout. Directories appear only when they are at the
/// manifest depth boundary (where individual files are not available).
///
/// Data lines go to `stdout` (pipeable); informational output goes to `stderr`.
pub fn format_report(result: &CompareResult) -> CompareReport {
    let mut stdout = String::new();
    let mut stderr = String::new();

    if result.identical {
        stderr.push_str("Trees are identical.\n");
        return CompareReport { stdout, stderr };
    }

    for d in &result.changed_dirs {
        if path_depth(&d.path) == result.depth {
            stdout.push_str(&format!("!\t{}\n", d.path));
        }
    }

    for f in &result.changed_files {
        stdout.push_str(&format!("!\t{}\n", f.path));
    }

    for p in &result.only_in_first {
        let is_dir = p.ends_with('/');
        if !is_dir || path_depth(p) == result.depth {
            stdout.push_str(&format!("<\t{p}\n"));
        }
    }

    for p in &result.only_in_second {
        let is_dir = p.ends_with('/');
        if !is_dir || path_depth(p) == result.depth {
            stdout.push_str(&format!(">\t{p}\n"));
        }
    }

    if !result.dataless_warnings.is_empty() {
        stderr.push_str("\nDataless warnings (content not verified):\n");
        let mut sorted: Vec<&String> = result.dataless_warnings.iter().collect();
        sorted.sort();
        for p in sorted {
            stderr.push_str(&format!("  {p}\n"));
        }
    }

    if !result.error_warnings.is_empty() {
        stderr.push_str("\nError warnings (could not read):\n");
        let mut sorted: Vec<&String> = result.error_warnings.iter().collect();
        sorted.sort();
        for p in sorted {
            stderr.push_str(&format!("  {p}\n"));
        }
    }

    let boundary_dirs = result
        .changed_dirs
        .iter()
        .filter(|d| path_depth(&d.path) == result.depth)
        .count();
    let summary = format!(
        "\nSummary: {} files differ, {} dirs differ, {} only in {}, {} only in {}\n",
        result.changed_files.len(),
        boundary_dirs,
        result.only_in_first.len(),
        result.host1,
        result.only_in_second.len(),
        result.host2,
    );
    stderr.push_str(&summary);

    CompareReport { stdout, stderr }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(entry_type: EntryType, value: &str, path: &str) -> ManifestEntry {
        ManifestEntry {
            entry_type,
            value: value.to_string(),
            path: path.to_string(),
        }
    }

    fn dir(value: &str, path: &str) -> ManifestEntry {
        entry(EntryType::Dir, value, path)
    }

    fn file(value: &str, path: &str) -> ManifestEntry {
        entry(EntryType::Blake3, value, path)
    }

    #[test]
    fn identical_manifests() {
        let entries = vec![dir("aaa", "./"), file("bbb", "./file.txt")];

        let result = compare_manifests(&entries, &entries, "host1", "host2", 6);
        assert!(result.identical);
        assert!(result.changed_dirs.is_empty());
        assert!(result.changed_files.is_empty());
        assert!(result.only_in_first.is_empty());
        assert!(result.only_in_second.is_empty());
    }

    #[test]
    fn one_file_differs() {
        let entries1 = vec![
            dir("root1", "./"),
            dir("dir1", "./dir/"),
            file("aaa", "./dir/file.txt"),
        ];
        let entries2 = vec![
            dir("root2", "./"),
            dir("dir2", "./dir/"),
            file("bbb", "./dir/file.txt"),
        ];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.identical);
        assert_eq!(result.changed_files.len(), 1);
        assert_eq!(result.changed_files[0].path, "./dir/file.txt");
        assert_eq!(result.changed_files[0].value1, "aaa");
        assert_eq!(result.changed_files[0].value2, "bbb");
        // Parent dirs with different hashes should appear in changed_dirs.
        assert!(result.changed_dirs.iter().any(|d| d.path == "./dir/"));
    }

    #[test]
    fn file_only_in_first() {
        let entries1 = vec![
            dir("root1", "./"),
            file("aaa", "./exists.txt"),
            file("bbb", "./gone.txt"),
        ];
        let entries2 = vec![dir("root2", "./"), file("aaa", "./exists.txt")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.identical);
        assert!(result.only_in_first.contains(&"./gone.txt".to_string()));
    }

    #[test]
    fn file_only_in_second() {
        let entries1 = vec![dir("root1", "./"), file("aaa", "./exists.txt")];
        let entries2 = vec![
            dir("root2", "./"),
            file("aaa", "./exists.txt"),
            file("ccc", "./added.txt"),
        ];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.identical);
        assert!(result.only_in_second.contains(&"./added.txt".to_string()));
    }

    #[test]
    fn directory_only_in_one() {
        let entries1 = vec![
            dir("root1", "./"),
            dir("d1", "./extra_dir/"),
            file("f1", "./extra_dir/file.txt"),
        ];
        let entries2 = vec![dir("root2", "./")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.identical);
        assert!(result.only_in_first.contains(&"./extra_dir/".to_string()));
        assert!(
            result
                .only_in_first
                .contains(&"./extra_dir/file.txt".to_string())
        );
    }

    #[test]
    fn merkle_skip_matching_directory() {
        // Both manifests have dir/ with the same hash but different file entries listed.
        // The Merkle skip should mean dir/'s children are NOT compared.
        let entries1 = vec![
            dir("root1", "./"),
            dir("same_hash", "./dir/"),
            file("aaa", "./dir/file.txt"),
            file("xxx", "./other.txt"),
        ];
        let entries2 = vec![
            dir("root2", "./"),
            dir("same_hash", "./dir/"),
            file("bbb", "./dir/file.txt"), // Different file hash, but dir hash matches!
            file("yyy", "./other.txt"),
        ];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        // dir/ hashes match, so children should be skipped.
        // Only other.txt should show as changed.
        assert!(!result.identical);
        assert!(
            !result
                .changed_files
                .iter()
                .any(|f| f.path == "./dir/file.txt"),
            "dir/file.txt should be skipped because dir/ hashes match"
        );
        assert!(result.changed_files.iter().any(|f| f.path == "./other.txt"));
    }

    #[test]
    fn dataless_entry_produces_warning() {
        let entries1 = vec![dir("root1", "./"), file("aaa", "./file.txt")];
        let entries2 = vec![
            dir("root2", "./"),
            entry(EntryType::Dataless, "12345", "./file.txt"),
        ];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.dataless_warnings.is_empty());
        assert!(result.dataless_warnings.contains("./file.txt"));
    }

    #[test]
    fn dataless_both_sides_still_warns() {
        let entries1 = vec![
            dir("root1", "./"),
            entry(EntryType::Dataless, "12345", "./file.txt"),
        ];
        let entries2 = vec![
            dir("root2", "./"),
            entry(EntryType::Dataless, "12345", "./file.txt"),
        ];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.dataless_warnings.is_empty());
    }

    #[test]
    fn error_entry_produces_warning() {
        let entries1 = vec![
            dir("root1", "./"),
            entry(EntryType::Error, "permission denied", "./locked.db"),
        ];
        let entries2 = vec![dir("root2", "./"), file("aaa", "./locked.db")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2", 6);
        assert!(!result.error_warnings.is_empty());
        assert!(result.error_warnings.contains("./locked.db"));
    }

    fn result_with_depth(depth: usize) -> CompareResult {
        CompareResult {
            identical: false,
            depth,
            host1: "h1".to_string(),
            host2: "h2".to_string(),
            changed_dirs: vec![],
            changed_files: vec![],
            only_in_first: vec![],
            only_in_second: vec![],
            dataless_warnings: HashSet::new(),
            error_warnings: HashSet::new(),
        }
    }

    #[test]
    fn compact_identical_stdout_empty() {
        let mut result = result_with_depth(6);
        result.identical = true;

        let report = format_report(&result);
        assert!(report.stdout.is_empty());
        assert!(report.stderr.contains("identical"));
    }

    #[test]
    fn compact_changed_file_uses_bang_prefix() {
        let mut result = result_with_depth(6);
        result.changed_dirs.push(ChangedEntry {
            path: "./".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });
        result.changed_files.push(ChangedEntry {
            path: "./file.txt".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });

        let report = format_report(&result);
        assert!(report.stdout.contains("!\t./file.txt\n"));
        // Root dir (depth 0) should NOT appear -- not at depth boundary.
        assert!(!report.stdout.contains("./\n"));
    }

    #[test]
    fn compact_only_in_first_uses_less_than_prefix() {
        let mut result = result_with_depth(6);
        result.only_in_first.push("./gone.txt".to_string());

        let report = format_report(&result);
        assert!(report.stdout.contains("<\t./gone.txt\n"));
    }

    #[test]
    fn compact_only_in_second_uses_greater_than_prefix() {
        let mut result = result_with_depth(6);
        result.only_in_second.push("./added.txt".to_string());

        let report = format_report(&result);
        assert!(report.stdout.contains(">\t./added.txt\n"));
    }

    #[test]
    fn compact_summary_on_stderr() {
        let mut result = result_with_depth(6);
        result.changed_files.push(ChangedEntry {
            path: "./file.txt".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });
        result.only_in_first.push("./gone.txt".to_string());

        let report = format_report(&result);
        assert!(report.stderr.contains("Summary:"));
        assert!(!report.stdout.contains("Summary"));
    }

    #[test]
    fn ancestor_dirs_not_in_stdout() {
        // depth=2, changed dirs at depth 0 and 1 should NOT appear
        let mut result = result_with_depth(2);
        result.changed_dirs.push(ChangedEntry {
            path: "./".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });
        result.changed_dirs.push(ChangedEntry {
            path: "./subdir/".to_string(),
            value1: "ccc".to_string(),
            value2: "ddd".to_string(),
        });
        result.changed_files.push(ChangedEntry {
            path: "./subdir/file.txt".to_string(),
            value1: "eee".to_string(),
            value2: "fff".to_string(),
        });

        let report = format_report(&result);
        // Only the file should be on stdout -- dirs are ancestors, not at boundary.
        assert_eq!(report.stdout, "!\t./subdir/file.txt\n");
    }

    #[test]
    fn depth_boundary_dir_appears_on_stdout() {
        // depth=2, a changed dir at depth 2 should appear (it's the boundary)
        let mut result = result_with_depth(2);
        result.changed_dirs.push(ChangedEntry {
            path: "./".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });
        result.changed_dirs.push(ChangedEntry {
            path: "./a/".to_string(),
            value1: "ccc".to_string(),
            value2: "ddd".to_string(),
        });
        result.changed_dirs.push(ChangedEntry {
            path: "./a/b/".to_string(),
            value1: "eee".to_string(),
            value2: "fff".to_string(),
        });

        let report = format_report(&result);
        // Only ./a/b/ (depth 2) should appear -- it's at the boundary.
        assert!(report.stdout.contains("!\t./a/b/\n"));
        assert!(!report.stdout.contains("!\t./\n"));
        assert!(!report.stdout.contains("!\t./a/\n"));
        // Summary should count only the 1 boundary dir, not all 3.
        assert!(
            report.stderr.contains("1 dirs differ"),
            "summary should count only boundary dirs, got: {}",
            report.stderr
        );
    }

    #[test]
    fn only_in_one_side_dir_at_boundary_appears() {
        let mut result = result_with_depth(2);
        result.only_in_first.push("./a/deep_dir/".to_string());
        // Shallow dir (depth 1) should NOT appear.
        result.only_in_first.push("./shallow_dir/".to_string());

        let report = format_report(&result);
        assert!(report.stdout.contains("<\t./a/deep_dir/\n"));
        assert!(!report.stdout.contains("shallow_dir"));
    }

    #[test]
    fn only_in_one_side_dir_at_boundary_second() {
        let mut result = result_with_depth(2);
        result.only_in_second.push("./a/deep_dir/".to_string());

        let report = format_report(&result);
        assert!(report.stdout.contains(">\t./a/deep_dir/\n"));
    }

    #[test]
    fn large_depth_no_boundary_dirs() {
        // depth=100, no dirs will be at the boundary
        let mut result = result_with_depth(100);
        result.changed_dirs.push(ChangedEntry {
            path: "./".to_string(),
            value1: "aaa".to_string(),
            value2: "bbb".to_string(),
        });
        result.changed_dirs.push(ChangedEntry {
            path: "./subdir/".to_string(),
            value1: "ccc".to_string(),
            value2: "ddd".to_string(),
        });
        result.changed_files.push(ChangedEntry {
            path: "./subdir/file.txt".to_string(),
            value1: "eee".to_string(),
            value2: "fff".to_string(),
        });

        let report = format_report(&result);
        // Only the file -- dirs are well below the boundary.
        assert_eq!(report.stdout, "!\t./subdir/file.txt\n");
    }

    #[test]
    fn compact_warnings_on_stderr() {
        let mut result = result_with_depth(6);
        result.dataless_warnings.insert("./cloud.txt".to_string());
        result.error_warnings.insert("./locked.db".to_string());

        let report = format_report(&result);
        assert!(report.stderr.contains("cloud.txt"));
        assert!(report.stderr.contains("locked.db"));
        assert!(!report.stdout.contains("cloud.txt"));
        assert!(!report.stdout.contains("locked.db"));
    }
}
