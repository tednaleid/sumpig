use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn sumpig() -> Command {
    Command::cargo_bin("sumpig").unwrap()
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

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.starts_with("# sumpig fingerprint\n"));
    assert!(manifest.contains("# version: 2\n"));
    assert!(manifest.contains("# host: "));
    assert!(manifest.contains("# depth: 6\n"));
    assert!(manifest.contains("# total_files: 3\n"));
    assert!(manifest.contains("# total_bytes: 9\n"));
}

#[test]
fn fingerprint_has_correct_entries() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    sumpig()
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
    assert!(manifest.contains("\t./\n"));
    assert!(manifest.contains("\t./dir1/\n"));
    assert!(manifest.contains("\t./dir2/\n"));
    assert!(manifest.contains("\t./file_a.txt\n"));
    assert!(manifest.contains("\t./dir1/file_b.txt\n"));
    assert!(manifest.contains("\t./dir2/file_c.txt\n"));
}

#[test]
fn fingerprint_deterministic() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out1 = dir.path().join("run1.txt");
    let out2 = dir.path().join("run2.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out1.to_string_lossy(),
        ])
        .assert()
        .success();
    sumpig()
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

    sumpig()
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

    sumpig()
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

    sumpig()
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
    sumpig()
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

    sumpig()
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

    sumpig()
        .args(["fingerprint", &tree.to_string_lossy()])
        .assert()
        .success();

    let sync_dir = tree.join(".sumpig-fingerprints");
    assert!(sync_dir.exists());
    assert!(sync_dir.is_dir());

    // Should contain exactly one file named <hostname>.txt.
    let files: Vec<_> = fs::read_dir(&sync_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let filename = files[0].as_ref().unwrap().file_name();
    assert!(filename.to_string_lossy().ends_with(".txt"));
}

#[test]
fn no_ignore_includes_node_modules() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    fs::create_dir(tree.join("node_modules")).unwrap();
    fs::write(tree.join("node_modules/pkg.json"), "{}").unwrap();

    let out_skip = dir.path().join("with_skip.txt");
    let out_noskip = dir.path().join("no_ignore.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_skip.to_string_lossy(),
        ])
        .assert()
        .success();
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_noskip.to_string_lossy(),
            "--no-ignore",
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

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out_default.to_string_lossy(),
        ])
        .assert()
        .success();
    sumpig()
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

    let assert = sumpig()
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
    sumpig()
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

    sumpig()
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
    assert!(manifest.contains("\t./\n")); // Root entry should still exist.
}

#[test]
fn quiet_flag_suppresses_summary() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());

    // Manifest should still be written correctly.
    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.starts_with("# sumpig fingerprint\n"));
    assert!(manifest.contains("# total_files: 3\n"));
}

// --- Compare integration tests ---

/// Helper: fingerprint a tree and return the manifest path.
fn fingerprint_to(tree: &std::path::Path, output: &std::path::Path) {
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();
}

#[test]
fn compare_identical_manifests() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let manifest = dir.path().join("manifest.txt");
    fingerprint_to(&tree, &manifest);

    let copy = dir.path().join("copy.txt");
    fs::copy(&manifest, &copy).unwrap();

    sumpig()
        .args([
            "compare",
            &manifest.to_string_lossy(),
            &copy.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("identical"));
}

#[test]
fn compare_modified_file_reports_diff() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::write(tree.join("file_a.txt"), "modified content").unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("!\t./file_a.txt"));
}

#[test]
fn compare_added_file_reports_only_in_second() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::write(tree.join("new_file.txt"), "new").unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(">\t./new_file.txt"));
}

#[test]
fn compare_deleted_file_reports_only_in_first() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::remove_file(tree.join("file_a.txt")).unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("<\t./file_a.txt"));
}

#[test]
fn compare_manifest_against_itself() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let manifest = dir.path().join("manifest.txt");
    fingerprint_to(&tree, &manifest);

    sumpig()
        .args([
            "compare",
            &manifest.to_string_lossy(),
            &manifest.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("identical"));
}

#[test]
fn compare_nonexistent_file_exits_2() {
    sumpig()
        .args(["compare", "/nonexistent/a.txt", "/nonexistent/b.txt"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("error"));
}

#[test]
fn compare_depth_mismatch_same_root() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let shallow = dir.path().join("shallow.txt");
    let deep = dir.path().join("deep.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &shallow.to_string_lossy(),
            "--depth",
            "1",
            "--quiet",
        ])
        .assert()
        .success();

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &deep.to_string_lossy(),
            "--depth",
            "6",
            "--quiet",
        ])
        .assert()
        .success();

    // Root hashes match, so comparing the shared entries (root + depth-1 entries) should pass.
    // The depth-6 manifest has extra entries, but they should show as "only in second".
    // However, the root dir hash matches, so Merkle skip covers everything.
    sumpig()
        .args([
            "compare",
            &shallow.to_string_lossy(),
            &deep.to_string_lossy(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("identical"));
}

// --- Compare format integration tests ---

#[test]
fn compare_compact_stdout_only_has_prefixed_lines() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::write(tree.join("file_a.txt"), "modified content").unwrap();
    fs::write(tree.join("new_file.txt"), "new").unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    let output = sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    // Every non-empty line on stdout should start with !, <, or >.
    for line in stdout.lines() {
        assert!(
            line.starts_with("!\t") || line.starts_with("<\t") || line.starts_with(">\t"),
            "unexpected stdout line: {line}"
        );
    }
    assert!(stdout.contains("!\t./file_a.txt\n"));
    assert!(stdout.contains(">\t./new_file.txt\n"));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Summary:"));
}

#[test]
fn compare_show_directories_flag() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::write(tree.join("file_a.txt"), "modified content").unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    // Without -d: no directory lines on stdout.
    let output_no_d = sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .output()
        .unwrap();
    let stdout_no_d = String::from_utf8(output_no_d.stdout).unwrap();
    assert!(
        !stdout_no_d.contains("./\n"),
        "default output should not contain directory entries"
    );

    // With -d: directory lines appear.
    let output_d = sumpig()
        .args([
            "compare",
            "-d",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .output()
        .unwrap();
    let stdout_d = String::from_utf8(output_d.stdout).unwrap();
    assert!(
        stdout_d.contains("!\t./\n"),
        "with -d, root dir should appear as changed"
    );
}

#[test]
fn compare_summary_on_stderr_not_stdout() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");
    fingerprint_to(&tree, &before);

    fs::write(tree.join("file_a.txt"), "modified content").unwrap();

    let after = dir.path().join("after.txt");
    fingerprint_to(&tree, &after);

    let output = sumpig()
        .args([
            "compare",
            &before.to_string_lossy(),
            &after.to_string_lossy(),
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(
        !stdout.contains("Summary"),
        "Summary should not be on stdout"
    );
    assert!(stderr.contains("Summary:"), "Summary should be on stderr");
}

// --- Mode integration tests ---

#[test]
fn default_mode_is_fast() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("manifest.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.contains("# mode: fast\n"));
    assert!(manifest.contains("# version: 2\n"));
    assert!(manifest.contains("# total_files: 3\n"));
}

#[test]
fn default_mode_deterministic() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let out1 = dir.path().join("run1.txt");
    let out2 = dir.path().join("run2.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &out1.to_string_lossy(),
        ])
        .assert()
        .success();
    sumpig()
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
    assert_eq!(content_lines(&m1), content_lines(&m2));
}

#[test]
fn default_mode_detects_file_modification() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let before = dir.path().join("before.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &before.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    // Modify a file (changes both size and mtime).
    fs::write(tree.join("file_a.txt"), "modified content that is longer").unwrap();

    let after = dir.path().join("after.txt");
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &after.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    let m1 = fs::read_to_string(&before).unwrap();
    let m2 = fs::read_to_string(&after).unwrap();
    assert_ne!(extract_root_hash(&m1), extract_root_hash(&m2));
}

#[test]
fn verify_contents_produces_content_mode() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("content.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
            "--verify-contents",
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.contains("# mode: content\n"));
}

#[test]
fn verify_contents_short_flag() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let output_file = dir.path().join("content.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &output_file.to_string_lossy(),
            "-C",
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&output_file).unwrap();
    assert!(manifest.contains("# mode: content\n"));
}

#[test]
fn default_and_verify_contents_produce_different_hashes() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let fast_out = dir.path().join("fast.txt");
    let content_out = dir.path().join("content.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &fast_out.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &content_out.to_string_lossy(),
            "--verify-contents",
            "--quiet",
        ])
        .assert()
        .success();

    let fast_manifest = fs::read_to_string(&fast_out).unwrap();
    let content_manifest = fs::read_to_string(&content_out).unwrap();

    assert!(fast_manifest.contains("# mode: fast\n"));
    assert!(content_manifest.contains("# mode: content\n"));
    assert_ne!(
        extract_root_hash(&fast_manifest),
        extract_root_hash(&content_manifest)
    );
}

#[test]
fn compare_mode_mismatch_warns() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let fast_out = dir.path().join("fast.txt");
    let content_out = dir.path().join("content.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &fast_out.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &content_out.to_string_lossy(),
            "--verify-contents",
            "--quiet",
        ])
        .assert()
        .success();

    sumpig()
        .args([
            "compare",
            &fast_out.to_string_lossy(),
            &content_out.to_string_lossy(),
        ])
        .assert()
        .stderr(predicate::str::contains("mode mismatch"));
}

// --- Tag integration tests ---

#[test]
fn tag_with_no_value_produces_timestamped_file() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);

    sumpig()
        .args(["fingerprint", &tree.to_string_lossy(), "--tag"])
        .assert()
        .success();

    let sync_dir = tree.join(".sumpig-fingerprints");
    let files: Vec<_> = fs::read_dir(&sync_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1);

    // Filename should contain a timestamp pattern like "2026-03-29T".
    let filename = files[0].file_name();
    let name = filename.to_string_lossy();
    assert!(
        name.contains("T") && name.contains("-") && name.ends_with("Z.txt"),
        "expected timestamped filename, got: {name}"
    );
}

#[test]
fn tag_with_custom_name() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--tag",
            "before-upgrade",
        ])
        .assert()
        .success();

    let sync_dir = tree.join(".sumpig-fingerprints");
    let files: Vec<_> = fs::read_dir(&sync_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1);

    let filename = files[0].file_name();
    let name = filename.to_string_lossy();
    assert!(
        name.ends_with("-before-upgrade.txt"),
        "expected custom tag in filename, got: {name}"
    );
}

// --- Match-settings integration tests ---

#[test]
fn match_settings_applies_depth_and_mode() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");
    let matched = dir.path().join("matched.txt");

    // Create a reference manifest with depth 3 and content mode.
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--depth",
            "3",
            "--verify-contents",
            "--quiet",
        ])
        .assert()
        .success();

    // Fingerprint again using --match-settings.
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &matched.to_string_lossy(),
            "--match-settings",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&matched).unwrap();
    assert!(manifest.contains("# depth: 3\n"));
    assert!(manifest.contains("# mode: content\n"));
}

#[test]
fn match_settings_fast_mode() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");
    let matched = dir.path().join("matched.txt");

    // Create a reference manifest in fast mode (default).
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    // Match it.
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &matched.to_string_lossy(),
            "--match-settings",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&matched).unwrap();
    assert!(manifest.contains("# mode: fast\n"));
    assert!(manifest.contains("# depth: 6\n"));
}

#[test]
fn match_settings_conflict_with_depth() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--match-settings",
            &reference.to_string_lossy(),
            "--depth",
            "3",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn match_settings_conflict_with_verify_contents() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--match-settings",
            &reference.to_string_lossy(),
            "-C",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn match_settings_nonexistent_file() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--match-settings",
            "/nonexistent/manifest.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--match-settings"));
}

#[test]
fn match_settings_short_flag() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");
    let matched = dir.path().join("matched.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--depth",
            "2",
            "--quiet",
        ])
        .assert()
        .success();

    // Use -m short flag.
    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "-o",
            &matched.to_string_lossy(),
            "-m",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    let manifest = fs::read_to_string(&matched).unwrap();
    assert!(manifest.contains("# depth: 2\n"));
}

#[test]
fn match_settings_prints_compare_suggestion() {
    let dir = TempDir::new().unwrap();
    let tree = create_test_tree(&dir);
    let reference = dir.path().join("reference.txt");
    let matched = dir.path().join("matched.txt");

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &reference.to_string_lossy(),
            "--quiet",
        ])
        .assert()
        .success();

    sumpig()
        .args([
            "fingerprint",
            &tree.to_string_lossy(),
            "--output",
            &matched.to_string_lossy(),
            "--match-settings",
            &reference.to_string_lossy(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("To compare: sumpig compare"));
}
