# Implementation Spec: sumpig - Phase 2 (Compare Command)

**Contract**: ./contract.md
**Estimated Effort**: M

## Technical Approach

Parse two manifest files into structured data, then walk both entry sets comparing hashes. Use the Merkle property to skip matching subtrees: if a directory hash matches in both manifests, everything below it is identical and can be skipped. Report differences grouped by type (changed, only-in-one, warnings).

The compare module works entirely on parsed manifest data -- it does not touch the filesystem. This makes it fast and fully testable with in-memory fixtures.

## Feedback Strategy

**Inner-loop command**: `cargo test compare`

**Playground**: Unit tests with hand-crafted manifest entries

**Why this approach**: Compare operates on parsed data structures, not the filesystem. Unit tests with constructed inputs are the fastest feedback loop.

## File Changes

### New Files

| File Path | Purpose |
|---|---|
| `src/compare.rs` | Two-manifest comparison with Merkle skip optimization |

### Modified Files

| File Path | Changes |
|---|---|
| `src/lib.rs` | Add `pub mod compare;` |
| `src/main.rs` | Add `Compare` subcommand to clap enum, implement compare pipeline |
| `tests/integration.rs` | Add integration tests for compare command |

## Implementation Details

### 1. compare.rs - Comparison Logic

**Overview**: Compare two parsed manifests and produce a structured diff report. Uses the Merkle tree property: when a directory hash matches in both manifests, skip all children.

```rust
use crate::manifest::ManifestEntry;

pub struct CompareResult {
    pub identical: bool,
    pub host1: String,
    pub host2: String,
    pub changed_dirs: Vec<ChangedEntry>,
    pub changed_files: Vec<ChangedEntry>,
    pub only_in_first: Vec<String>,      // paths
    pub only_in_second: Vec<String>,     // paths
    pub dataless_warnings: Vec<String>,  // paths with dataless on either side
    pub error_warnings: Vec<String>,     // paths with error on either side
}

pub struct ChangedEntry {
    pub path: String,
    pub value1: String,   // hash/size/error from manifest 1
    pub value2: String,   // hash/size/error from manifest 2
}

/// Compare two sets of manifest entries.
/// Uses Merkle skip: if a directory hash matches, skip its children.
pub fn compare_manifests(
    entries1: &[ManifestEntry],
    entries2: &[ManifestEntry],
    host1: &str,
    host2: &str,
) -> CompareResult { ... }

/// Format a CompareResult for terminal output.
pub fn format_report(result: &CompareResult) -> String { ... }
```

**Key decisions**:

- Build path-to-entry maps from both manifests for O(1) lookup
- Walk entries in sorted order. When encountering a directory with matching hashes, mark all children as "skip" (they're covered by the directory match)
- Entries only in one manifest: check if they're children of a matched directory (if so, they're implicitly matched). If not, report them.
- Dataless/error entries: always flag as warnings regardless of whether they "match" -- the content cannot be verified
- Report uses hostnames from manifest headers for clarity (e.g., `cardinal:abc123  macstudio:def456`)

**Implementation steps**:

1. Define `CompareResult`, `ChangedEntry`
2. Implement manifest entry indexing (path -> entry map)
3. Implement Merkle skip logic: identify matching directories, track skipped paths
4. Implement difference detection: walk both entry sets, compare values, collect differences
5. Implement only-in-one detection: entries in one map but not the other (accounting for Merkle skip)
6. Implement warning collection: dataless and error entries on either side
7. Implement `format_report` matching the output format from the spec
8. Write unit tests

**Feedback loop**:

- **Playground**: Unit tests with constructed ManifestEntry vectors
- **Experiment**: Test with identical entries, one file changed, file only in one, dataless entries, error entries, Merkle skip scenarios
- **Check command**: `cargo test compare`

**Unit tests for compare.rs**:

- Identical manifests: `identical` is true, all diff vectors empty
- One file differs: appears in `changed_files`, parent dir appears in `changed_dirs`
- File only in manifest 1: appears in `only_in_first`
- File only in manifest 2: appears in `only_in_second`
- Directory only in one manifest: directory and all its children in `only_in_*`
- Merkle skip: two manifests with matching directory hash but different entries listed below -- the directory match means children are NOT compared (verify by checking changed_files is empty)
- Dataless entry on one side, blake3 on other: appears in `dataless_warnings`
- Dataless entry on both sides with same size: appears in `dataless_warnings` (can't verify content even if sizes match)
- Error entry: appears in `error_warnings`
- Format report matches expected output structure (header, sections, summary line)

### 2. Compare Subcommand

**Overview**: Add the compare subcommand to the CLI. Parse both files, run comparison, print report, set exit code.

```rust
// Add to Commands enum in main.rs
Compare {
    /// First fingerprint file
    file1: PathBuf,
    /// Second fingerprint file
    file2: PathBuf,
},
```

**Key decisions**:

- Exit codes: 0 = identical, 1 = differences found, 2 = usage error
- If manifests have different depths, warn on stderr but still compare (entries present in both are comparable)
- If manifests have different root paths, warn on stderr (paths are relative, so comparison still works, but the user should know)

**Implementation steps**:

1. Add `Compare` variant to `Commands` enum
2. In match arm: open and parse both files using `manifest::parse_manifest`
3. Call `compare::compare_manifests`
4. Print report to stdout
5. Print warnings (dataless, error) to stderr
6. Exit with appropriate code

### 3. Integration Tests for Compare

**Test cases to add to tests/integration.rs**:

- Fingerprint a temp dir, copy the manifest, compare: exit 0, reports "identical"
- Fingerprint, modify one file, re-fingerprint, compare old vs new: exit 1, reports the changed file
- Fingerprint, add a new file, re-fingerprint, compare: exit 1, reports file only in second
- Fingerprint, delete a file, re-fingerprint, compare: exit 1, reports file only in first
- Compare two identical manifests from different hosts: exit 0 (hostnames differ but content matches)
- Compare nonexistent file: exit 2, error message
- Compare manifest against itself: exit 0
- Depth mismatch: compare depth-1 vs depth-6 manifests. Root hashes match, exit 0. (Both have root entry with same hash.)

## Error Handling

| Error Scenario | Handling Strategy |
|---|---|
| Manifest file does not exist | Print error to stderr, exit 2 |
| Manifest file is malformed | Print parse error to stderr, exit 2 |
| Manifests have different depth | Warn on stderr, continue comparison |
| Manifests have different root paths | Warn on stderr, continue comparison |

## Failure Modes

| Component | Failure Mode | Trigger | Impact | Mitigation |
|---|---|---|---|---|
| compare.rs | Depth mismatch hides differences | Manifest 1 at depth 3, manifest 2 at depth 6 | Entries below depth 3 in manifest 2 have no counterpart in manifest 1 | Compare only entries present in both; warn about depth mismatch |
| compare.rs | Large manifest pair | Two manifests with 100K+ entries each | Slow comparison, high memory | HashMap lookup is O(1) per entry; 100K entries ~= 20MB RAM. Acceptable. |
| format_report | Very long diff | Thousands of differences | Output is overwhelming | Accept for v1 -- user can pipe to pager or head |

## Validation Commands

```bash
# Run compare unit tests
just test-one compare

# Run all checks (test + lint)
just check

# Run all tests
just test

# Quick smoke test: compare a manifest against itself
just run-fingerprint /tmp/test-dir
cp /tmp/test-dir/.sync-fingerprints/*.txt /tmp/copy.txt
just run-compare /tmp/test-dir/.sync-fingerprints/*.txt /tmp/copy.txt
echo $?  # should be 0
```

---

_This spec is ready for implementation. Follow the patterns and validate at each step._
