use std::collections::{HashMap, HashSet};

use crate::manifest::ManifestEntry;

/// Result of comparing two manifests.
pub struct CompareResult {
    pub identical: bool,
    pub host1: String,
    pub host2: String,
    pub changed_dirs: Vec<ChangedEntry>,
    pub changed_files: Vec<ChangedEntry>,
    pub only_in_first: Vec<String>,
    pub only_in_second: Vec<String>,
    pub dataless_warnings: Vec<String>,
    pub error_warnings: Vec<String>,
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
) -> CompareResult {
    let map1: HashMap<&str, &ManifestEntry> =
        entries1.iter().map(|e| (e.path.as_str(), e)).collect();
    let map2: HashMap<&str, &ManifestEntry> =
        entries2.iter().map(|e| (e.path.as_str(), e)).collect();

    // Collect directories with matching hashes (Merkle skip set).
    let mut skipped_prefixes: HashSet<&str> = HashSet::new();
    for e1 in entries1 {
        if e1.entry_type == "dir"
            && let Some(e2) = map2.get(e1.path.as_str())
            && e2.entry_type == "dir"
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
    let mut dataless_warnings = Vec::new();
    let mut error_warnings = Vec::new();

    // Walk entries from manifest 1.
    for e1 in entries1 {
        if is_skipped(&e1.path) {
            continue;
        }

        // Collect warnings for dataless/error entries.
        if e1.entry_type == "dataless" && !dataless_warnings.contains(&e1.path) {
            dataless_warnings.push(e1.path.clone());
        }
        if e1.entry_type == "error" && !error_warnings.contains(&e1.path) {
            error_warnings.push(e1.path.clone());
        }

        match map2.get(e1.path.as_str()) {
            Some(e2) => {
                // Collect warnings for dataless/error on the other side.
                if e2.entry_type == "dataless" && !dataless_warnings.contains(&e2.path) {
                    dataless_warnings.push(e2.path.clone());
                }
                if e2.entry_type == "error" && !error_warnings.contains(&e2.path) {
                    error_warnings.push(e2.path.clone());
                }

                // Compare values.
                if e1.value != e2.value || e1.entry_type != e2.entry_type {
                    let changed = ChangedEntry {
                        path: e1.path.clone(),
                        value1: e1.value.clone(),
                        value2: e2.value.clone(),
                    };
                    if e1.entry_type == "dir" || e2.entry_type == "dir" {
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

            if e2.entry_type == "dataless" && !dataless_warnings.contains(&e2.path) {
                dataless_warnings.push(e2.path.clone());
            }
            if e2.entry_type == "error" && !error_warnings.contains(&e2.path) {
                error_warnings.push(e2.path.clone());
            }
        }
    }

    let identical = changed_dirs.is_empty()
        && changed_files.is_empty()
        && only_in_first.is_empty()
        && only_in_second.is_empty();

    CompareResult {
        identical,
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

/// Format a CompareResult for terminal output.
pub fn format_report(result: &CompareResult) -> String {
    let mut out = String::new();

    if result.identical {
        out.push_str("Trees are identical.\n");
        return out;
    }

    out.push_str("Root hashes differ.\n");

    if !result.changed_dirs.is_empty() {
        out.push_str("\nChanged directories:\n");
        for d in &result.changed_dirs {
            out.push_str(&format!(
                "  {}    {}:{}  {}:{}\n",
                d.path, result.host1, d.value1, result.host2, d.value2,
            ));
        }
    }

    if !result.changed_files.is_empty() {
        out.push_str("\nChanged files:\n");
        for f in &result.changed_files {
            out.push_str(&format!(
                "  {}    {}:{}  {}:{}\n",
                f.path, result.host1, f.value1, result.host2, f.value2,
            ));
        }
    }

    if !result.only_in_first.is_empty() {
        out.push_str(&format!("\nOnly in {}:\n", result.host1));
        for p in &result.only_in_first {
            out.push_str(&format!("  {p}\n"));
        }
    }

    if !result.only_in_second.is_empty() {
        out.push_str(&format!("\nOnly in {}:\n", result.host2));
        for p in &result.only_in_second {
            out.push_str(&format!("  {p}\n"));
        }
    }

    if !result.dataless_warnings.is_empty() {
        out.push_str("\nDataless warnings (content not verified):\n");
        for p in &result.dataless_warnings {
            out.push_str(&format!("  {p}\n"));
        }
    }

    if !result.error_warnings.is_empty() {
        out.push_str("\nError warnings (could not read):\n");
        for p in &result.error_warnings {
            out.push_str(&format!("  {p}\n"));
        }
    }

    let summary = format!(
        "\nSummary: {} files differ, {} dirs differ, {} only in {}, {} only in {}\n",
        result.changed_files.len(),
        result.changed_dirs.len(),
        result.only_in_first.len(),
        result.host1,
        result.only_in_second.len(),
        result.host2,
    );
    out.push_str(&summary);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(entry_type: &str, value: &str, path: &str) -> ManifestEntry {
        ManifestEntry {
            entry_type: entry_type.to_string(),
            value: value.to_string(),
            path: path.to_string(),
        }
    }

    fn dir(value: &str, path: &str) -> ManifestEntry {
        entry("dir", value, path)
    }

    fn file(value: &str, path: &str) -> ManifestEntry {
        entry("blake3", value, path)
    }

    #[test]
    fn identical_manifests() {
        let entries = vec![dir("aaa", "./"), file("bbb", "./file.txt")];

        let result = compare_manifests(&entries, &entries, "host1", "host2");
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

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
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

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
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

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
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

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
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

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
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
        let entries2 = vec![dir("root2", "./"), entry("dataless", "12345", "./file.txt")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
        assert!(!result.dataless_warnings.is_empty());
        assert!(result.dataless_warnings.contains(&"./file.txt".to_string()));
    }

    #[test]
    fn dataless_both_sides_still_warns() {
        let entries1 = vec![dir("root1", "./"), entry("dataless", "12345", "./file.txt")];
        let entries2 = vec![dir("root2", "./"), entry("dataless", "12345", "./file.txt")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
        assert!(!result.dataless_warnings.is_empty());
    }

    #[test]
    fn error_entry_produces_warning() {
        let entries1 = vec![
            dir("root1", "./"),
            entry("error", "permission denied", "./locked.db"),
        ];
        let entries2 = vec![dir("root2", "./"), file("aaa", "./locked.db")];

        let result = compare_manifests(&entries1, &entries2, "h1", "h2");
        assert!(!result.error_warnings.is_empty());
        assert!(result.error_warnings.contains(&"./locked.db".to_string()));
    }

    #[test]
    fn format_report_identical() {
        let result = CompareResult {
            identical: true,
            host1: "mac1".to_string(),
            host2: "mac2".to_string(),
            changed_dirs: vec![],
            changed_files: vec![],
            only_in_first: vec![],
            only_in_second: vec![],
            dataless_warnings: vec![],
            error_warnings: vec![],
        };

        let report = format_report(&result);
        assert!(report.contains("identical"));
    }

    #[test]
    fn format_report_with_differences() {
        let result = CompareResult {
            identical: false,
            host1: "cardinal".to_string(),
            host2: "macstudio".to_string(),
            changed_dirs: vec![],
            changed_files: vec![ChangedEntry {
                path: "./file.txt".to_string(),
                value1: "aaa".to_string(),
                value2: "bbb".to_string(),
            }],
            only_in_first: vec!["./gone.txt".to_string()],
            only_in_second: vec![],
            dataless_warnings: vec![],
            error_warnings: vec![],
        };

        let report = format_report(&result);
        assert!(report.contains("./file.txt"));
        assert!(report.contains("cardinal"));
        assert!(report.contains("macstudio"));
        assert!(report.contains("./gone.txt"));
    }
}
