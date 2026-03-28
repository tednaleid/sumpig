use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn treesum() -> Command {
    Command::cargo_bin("treesum").unwrap()
}

/// Create a small test tree inside a "tree" subdirectory:
/// tree/
///   file_a.txt (content: "aaa")
///   dir1/
///     file_b.txt (content: "bbb")
///   dir2/
///     file_c.txt (content: "ccc")
///
/// Returns the path to the "tree" subdirectory.
/// Output files should go in the parent TempDir to avoid polluting the scanned tree.
fn create_test_tree(dir: &TempDir) -> std::path::PathBuf {
    let tree = dir.path().join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("file_a.txt"), "aaa").unwrap();
    fs::create_dir(tree.join("dir1")).unwrap();
    fs::write(tree.join("dir1/file_b.txt"), "bbb").unwrap();
    fs::create_dir(tree.join("dir2")).unwrap();
    fs::write(tree.join("dir2/file_c.txt"), "ccc").unwrap();
    tree
}

/// Extract non-date header lines and all data lines from manifest content.
/// Filters out the "# date:" line for determinism comparison.
fn content_lines(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .filter(|line| !line.starts_with("# date:"))
        .collect()
}

/// Extract the root hash from the "# root:" header line.
fn extract_root_hash(manifest: &str) -> &str {
    manifest
        .lines()
        .find(|line| line.starts_with("# root:"))
        .and_then(|line| line.strip_prefix("# root: "))
        .expect("manifest should have root hash")
}

#[test]
fn fingerprint_produces_valid_manifest() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.starts_with("# treesum fingerprint\n"));
    assert!(manifest.contains("# version: 1\n"));
    assert!(manifest.contains("# host: "));
    assert!(manifest.contains("# depth: 6\n"));
    assert!(manifest.contains("# total_files: 3\n"));
}

#[test]
fn fingerprint_has_correct_entries() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();

    // Should have entries for root, dir1, dir2, and the 3 files.
    assert!(manifest.contains("  ./\n"));
    assert!(manifest.contains("  ./dir1/\n"));
    assert!(manifest.contains("  ./dir2/\n"));
    assert!(manifest.contains("  ./file_a.txt\n"));
    assert!(manifest.contains("  ./dir1/file_b.txt\n"));
    assert!(manifest.contains("  ./dir2/file_c.txt\n"));
}

#[test]
fn fingerprint_deterministic() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out1 = dir.path().join("run1.txt");
    let out2 = dir.path().join("run2.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out1.to_string_lossy(),
        ])
        .assert()
        .success();
    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out2.to_string_lossy(),
        ])
        .assert()
        .success();

    let m1 = fs::read_to_string(&out1).unwrap();
    let m2 = fs::read_to_string(&out2).unwrap();

    // Content lines (excluding date) should be identical.
    assert_eq!(content_lines(&m1), content_lines(&m2));
}

#[test]
fn modify_file_changes_root_hash() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out1 = dir.path().join("before.txt");
    let out2 = dir.path().join("after.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out1.to_string_lossy(),
        ])
        .assert()
        .success();

    // Modify one file.
    fs::write(tree.join("file_a.txt"), "modified").unwrap();

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out2.to_string_lossy(),
        ])
        .assert()
        .success();

    let m1 = fs::read_to_string(&out1).unwrap();
    let m2 = fs::read_to_string(&out2).unwrap();

    assert_ne!(extract_root_hash(&m1), extract_root_hash(&m2));
}

#[test]
fn depth_one_fewer_entries_same_root() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out_deep = dir.path().join("deep.txt");
    let out_shallow = dir.path().join("shallow.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_deep.to_string_lossy(),
            "--depth",
            "6",
        ])
        .assert()
        .success();
    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_shallow.to_string_lossy(),
            "--depth",
            "1",
        ])
        .assert()
        .success();

    let m_deep = fs::read_to_string(&out_deep).unwrap();
    let m_shallow = fs::read_to_string(&out_shallow).unwrap();

    // Same root hash.
    assert_eq!(extract_root_hash(&m_deep), extract_root_hash(&m_shallow));

    // Shallow has fewer data lines.
    let data_lines = |m: &str| {
        m.lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .count()
    };
    assert!(data_lines(&m_shallow) < data_lines(&m_deep));
}

#[test]
fn output_flag_writes_to_specified_path() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let custom_output = dir.path().join("custom/output/manifest.txt");
    fs::create_dir_all(custom_output.parent().unwrap()).unwrap();

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &custom_output.to_string_lossy(),
        ])
        .assert()
        .success();

    assert!(custom_output.exists());
}

#[test]
fn default_output_goes_to_sync_fingerprints() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);

    treesum()
        .args(["fingerprint", &tree.to_string_lossy()])
        .assert()
        .success();

    let sync_dir = tree.join(".sync-fingerprints");
    assert!(sync_dir.exists());
    assert!(sync_dir.is_dir());

    // Should contain exactly one file named <hostname>.txt.
    let files: Vec<_> = fs::read_dir(&sync_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let filename = files[0].as_ref().unwrap().file_name();
    assert!(filename.to_string_lossy().ends_with(".txt"));
}

#[test]
fn no_skip_includes_node_modules() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    fs::create_dir(tree.join("node_modules")).unwrap();
    fs::write(tree.join("node_modules/pkg.json"), "{}").unwrap();

    let out_skip = dir.path().join("with_skip.txt");
    let out_noskip = dir.path().join("no_skip.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_skip.to_string_lossy(),
        ])
        .assert()
        .success();
    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_noskip.to_string_lossy(),
            "--no-skip",
        ])
        .assert()
        .success();

    let m_skip = fs::read_to_string(&out_skip).unwrap();
    let m_noskip = fs::read_to_string(&out_noskip).unwrap();

    assert!(!m_skip.contains("node_modules"));
    assert!(m_noskip.contains("node_modules"));
}

#[test]
fn jobs_one_same_output_as_default() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out_default = dir.path().join("default.txt");
    let out_single = dir.path().join("single.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_default.to_string_lossy(),
        ])
        .assert()
        .success();
    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_single.to_string_lossy(),
            "--jobs",
            "1",
        ])
        .assert()
        .success();

    let m1 = fs::read_to_string(&out_default).unwrap();
    let m2 = fs::read_to_string(&out_single).unwrap();

    assert_eq!(content_lines(&m1), content_lines(&m2));
}

#[test]
fn summary_goes_to_stderr() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    let assert = treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    // Stderr should contain the summary.
    assert
        .stderr(predicate::str::contains("files"))
        .stderr(predicate::str::contains("root:"));
}

#[test]
fn nonexistent_path_exits_with_error() {
    treesum()
        .args(["fingerprint", "/nonexistent/path/that/does/not/exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn fingerprint_empty_directory() {
    let dir = TempDir::new().unwrap();
    let tree = dir.path().join("empty_tree");
    fs::create_dir(&tree).unwrap();
    let output_file = dir.path().join("manifest.txt");

    treesum()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.contains("# total_files: 0\n"));
    assert!(manifest.contains("  ./\n")); // Root entry should still exist.
}
