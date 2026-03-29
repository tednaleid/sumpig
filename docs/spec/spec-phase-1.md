# Implementation Spec: sumpig - Phase 1 (Core Library + Fingerprint Command)

**Contract**: ./contract.md
**Estimated Effort**: L

## Technical Approach

Build bottom-up: hash individual files, walk directory trees, construct Merkle hashes, serialize to manifest format, wire up the CLI. Each module is independently testable with unit tests before integration.

The crate has both a library target (src/lib.rs) and a binary target (src/main.rs). The library exposes public modules so criterion benchmarks (Phase 3) can access internals. The binary is a thin CLI wrapper using clap.

Parallelism strategy follows ripgrep's model: jwalk handles parallel directory traversal, rayon's par_iter handles parallel file hashing, per-thread buffers avoid allocation contention. BLAKE3's built-in SIMD (NEON on Apple Silicon) handles single-file throughput.

TDD throughout: write tests alongside each module. Unit tests use in-module `#[cfg(test)] mod tests`. Integration tests use tempfile for fixtures and assert_cmd for CLI invocation.

## Feedback Strategy

**Inner-loop command**: `cargo test`

**Playground**: Test suite (unit tests in each module, integration tests in tests/integration.rs)

**Why this approach**: Every module is a library with pure-ish functions. Fast unit tests give immediate feedback on correctness.

## File Changes

### New Files

| File Path | Purpose |
|---|---|
| `Cargo.toml` | Project manifest with dependencies |
| `CLAUDE.md` | Project principles and development guide |
| `src/lib.rs` | Library root, re-exports public modules |
| `src/main.rs` | CLI entry point, clap subcommand routing |
| `src/hash.rs` | BLAKE3 file hashing, dataless detection, error recording |
| `src/walk.rs` | Parallel directory walking with configurable ignore list |
| `src/merkle.rs` | Merkle tree construction, depth-limited serialization |
| `src/manifest.rs` | Manifest file writing and parsing |
| `justfile` | Project command runner with recipes for check, test, lint, build, bench, etc. |
| `tests/integration.rs` | End-to-end CLI tests for fingerprint command |

## Implementation Details

### 1. Project Setup

**Overview**: Initialize the Rust project with all dependencies and the CLAUDE.md.

**Cargo.toml dependencies**:

```toml
[package]
name = "sumpig"
version = "0.1.0"
edition = "2024"

[dependencies]
blake3 = { version = "1", features = ["rayon"] }
jwalk = "0.8"
rayon = "1"
clap = { version = "4", features = ["derive"] }

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

Note: criterion and `[[bench]]` sections are added in Phase 3.

**justfile** (all project commands go through `just`):

```just
# Run all checks (test + lint)
check: test lint

# Run all tests
test:
    cargo test

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Build release binary
build:
    cargo build --release

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt -- --check

# Run benchmarks (available after Phase 3)
bench:
    cargo bench

# Run a specific test by name
test-one NAME:
    cargo test {{NAME}}

# Build and run fingerprint on a path
run-fingerprint PATH:
    cargo run -- fingerprint {{PATH}}

# Build and run compare on two files
run-compare FILE1 FILE2:
    cargo run -- compare {{FILE1}} {{FILE2}}
```

**CLAUDE.md content** (project principles):

- Correctness over speed: never silently skip files. Every file is hashed, recorded as dataless, or recorded as an error.
- Deterministic output: same input always produces same manifest byte-for-byte.
- Target ripgrep-class performance: parallel walking (jwalk), parallel hashing (rayon), BLAKE3 SIMD.
- Test everything: unit tests in every module, integration tests for CLI, all tests run in seconds.
- Module boundaries: hash.rs, walk.rs, merkle.rs, manifest.rs, compare.rs each own their types and logic.
- All commands through `just`: test, lint, check, build, bench, fmt. Never invoke cargo directly in documentation or workflow.
- Reference spec: `docs/spec/sumpig-spec.md` has the full design rationale.

**Implementation steps**:

1. Create Cargo.toml with dependencies listed above
2. Create src/lib.rs with `pub mod` declarations
3. Create stub files for each module (hash.rs, walk.rs, merkle.rs, manifest.rs)
4. Create src/main.rs with basic clap structure (fingerprint subcommand only for now)
5. Write CLAUDE.md
6. Verify `cargo check` passes

### 2. hash.rs - File Hashing

**Overview**: Hash individual files with BLAKE3. Detect macOS dataless files (iCloud-evicted). Record I/O errors instead of panicking.

```rust
use std::path::Path;

/// Result of hashing a single file
pub enum FileHash {
    /// Successful BLAKE3 hash of file content
    Blake3([u8; 32]),
    /// macOS dataless file (iCloud-evicted), stores file size
    Dataless(u64),
    /// File could not be read, stores error description
    Error(String),
    /// Symbolic link, stores target path
    Symlink(String),
}

/// Hash a file at the given path.
/// Checks for dataless flag and symlinks before attempting to read.
pub fn hash_file(path: &Path) -> FileHash { ... }

/// Format a 32-byte hash as truncated hex (32 hex chars = 128 bits)
pub fn hash_to_hex(hash: &[u8; 32]) -> String { ... }
```

**Key decisions**:

- Check `lstat()` first: if symlink, return `Symlink(readlink target)` without following. If dataless (SF_DATALESS flag 0x40000000 in stat flags), return `Dataless(size)`. Otherwise, read and hash.
- Use 64KB buffered reads (matching ripgrep's default). Per-thread buffer reuse via rayon's `thread_local!` pattern.
- Truncate hash to 32 hex chars (128 bits) only at output time, not in the internal representation. Internal hashes are always full 256-bit.
- For large files (>1MB), use blake3's `update_rayon()` for multi-threaded hashing of a single file. For small files, single-threaded is faster (avoids rayon overhead).

**Implementation steps**:

1. Define the `FileHash` enum
2. Implement `hash_to_hex()`
3. Implement symlink detection (lstat + is_symlink check)
4. Implement dataless detection using `std::os::macos::fs::MetadataExt` for st_flags
5. Implement buffered file reading and BLAKE3 hashing
6. Implement error handling (catch I/O errors, return `FileHash::Error`)
7. Write unit tests

**Feedback loop**:

- **Playground**: Unit tests in hash.rs
- **Experiment**: Hash known content (empty file, known string, symlink, simulated error via nonexistent path)
- **Check command**: `cargo test hash`

**Unit tests for hash.rs**:

- Hash a file with known content, verify against reference BLAKE3 hash (compute expected with `b3sum` or the blake3 crate directly)
- Hash an empty file, verify it produces blake3's empty-input hash
- Hash a symlink, verify it returns `Symlink` with correct target path
- Attempt to hash a nonexistent path, verify it returns `Error`
- `hash_to_hex` produces correct 32-char lowercase hex string
- Verify internal hash is full 32 bytes (256 bits)

Note: dataless detection is hard to unit test without macOS-specific setup. Test it in integration tests or with a mock. At minimum, verify the flag constant is correct.

### 3. walk.rs - Directory Walking

**Overview**: Parallel directory traversal using jwalk. Configurable ignore list with `--no-ignore` override. Returns sorted entries with metadata.

```rust
use std::path::{Path, PathBuf};

pub struct WalkOptions {
    pub use_default_ignores: bool,  // true = apply default ignore list, false = --no-ignore
    pub num_threads: usize,   // 0 = use rayon default (num CPUs)
}

pub struct WalkEntry {
    pub path: PathBuf,        // relative to walk root
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// Default directories to ignore (not hashed, not listed)
pub const IGNORE_DIRS: &[&str] = &[
    "node_modules", ".venv", "venv", "target", "__pycache__",
    "build", "dist", ".Trash", ".sumpig-fingerprints",
];

/// Default files to ignore
pub const IGNORE_FILES: &[&str] = &[".DS_Store", ".localized"];

/// File extensions to ignore
pub const IGNORE_EXTENSIONS: &[&str] = &["nosync"];

/// Walk a directory tree, returning sorted entries.
/// Applies ignore list unless options.use_default_ignores is false.
pub fn walk_directory(root: &Path, options: &WalkOptions) -> Vec<WalkEntry> { ... }
```

**Key decisions**:

- jwalk's `process_read_dir` callback handles ignore filtering during traversal (avoids collecting then filtering)
- .git directories are NOT ignored (detecting iCloud git corruption is a primary use case)
- Results are sorted by path for deterministic output
- Symlinks are included as entries but not followed (no recursion into symlink targets)
- Directory entries are included in the output (needed for Merkle tree construction)

**Implementation steps**:

1. Define `WalkOptions`, `WalkEntry`, and ignore list constants
2. Configure jwalk::WalkDir with parallel options and sort
3. Implement ignore filtering in the walk callback
4. Collect entries with relative paths
5. Sort by path
6. Write unit tests

**Feedback loop**:

- **Playground**: Unit tests with tempfile::TempDir fixtures
- **Experiment**: Create trees with ignored dirs, symlinks, .git dirs
- **Check command**: `cargo test walk`

**Unit tests for walk.rs**:

- Walk a simple tree (3 files, 2 dirs), verify all entries returned in sorted order
- Skip list: create a tree with node_modules/, .DS_Store, .venv/ -- verify they are excluded
- .git directories ARE included (create .git/objects/foo, verify it appears)
- `--no-ignore` (use_default_ignores=false): same tree, verify node_modules/ IS included
- Symlinks are returned as entries but not followed
- *.nosync files/dirs are excluded
- .sumpig-fingerprints/ directory is excluded
- Empty directories are included

### 4. merkle.rs - Streaming Merkle Tree Computation

**Overview**: Compute Merkle directory hashes and emit manifest entries in a single streaming pass over sorted (path, hash) pairs. No explicit tree structure is built in memory. Uses a stack of BLAKE3 hashers, one per open directory.

This is the key memory optimization: instead of building a full `TreeNode` tree in memory (which would be O(total_files) in size), we compute directory hashes on the fly from the sorted input. Peak extra memory is O(max_tree_depth) for the hasher stack, not O(total_files).

**Memory budget**: The main memory cost is the input `Vec<(PathBuf, FileHash)>` from the walk+hash phase (~132 bytes/entry: ~100 for path + 32 for hash). For 1M files, that's ~132MB. The Merkle computation itself adds only a few KB (stack of hashers). The output `Vec<FlatEntry>` is bounded by the depth parameter, not total file count.

```rust
use crate::hash::FileHash;
use std::path::PathBuf;

/// A flattened entry for manifest output
pub struct FlatEntry {
    pub entry_type: EntryType,
    pub value: String,         // hex hash, size, error reason, or symlink target
    pub path: String,          // relative path with ./ prefix
}

pub enum EntryType {
    Blake3,
    Dataless,
    Error,
    Symlink,
}

/// Compute Merkle directory hashes and produce manifest entries from sorted file entries.
///
/// Algorithm: maintain a stack of (directory_path, blake3::Hasher) pairs.
/// As we iterate sorted entries, detect directory boundary transitions by comparing
/// path prefixes. When leaving a directory, finalize its hash, feed it into the
/// parent's hasher, and emit a directory FlatEntry (if within max_depth).
///
/// Input entries MUST be sorted by path. Only file entries are provided as input;
/// directory entries are synthesized from the path structure.
pub fn compute_manifest(
    sorted_entries: &[(PathBuf, FileHash)],
    max_depth: usize,
) -> (Vec<FlatEntry>, [u8; 32]) { ... }  // returns (entries, root_hash)

/// Compute a synthetic hash for non-Blake3 entries (dataless, error, symlink).
/// Used to include these entries in directory hash computation.
fn synthetic_hash(entry_type: &EntryType, value: &str) -> [u8; 32] { ... }
```

**Key decisions**:

- **No TreeNode type.** The tree is never materialized. Directory hashes are computed incrementally from sorted input using a stack of running hashers.
- Directory hash computation: for each child, feed `name_bytes + b'\0' + child_hash_bytes` into the directory's BLAKE3 hasher. Children are already sorted (input is sorted by path). Finalize when all children have been processed.
- For non-Blake3 children (dataless, error, symlink), compute a synthetic hash: `blake3(entry_type_prefix + ":" + value_bytes)`.
- File entries within max_depth are emitted as FlatEntry records immediately. Directory entries are emitted when their hash is finalized.
- The root directory entry "./" is always emitted regardless of depth.

**Streaming algorithm (pseudocode)**:

```
stack = [(root_path, new_hasher)]
output = []

for (path, hash) in sorted_entries:
    dir = parent_dir(path)
    name = filename(path)

    // Pop directories we've left
    while stack.top().path is not a prefix of dir:
        (dir_path, hasher) = stack.pop()
        dir_hash = hasher.finalize()
        dir_name = basename(dir_path)
        // Feed this directory's hash into its parent
        stack.top().hasher.update(dir_name + '\0' + dir_hash)
        // Emit directory entry if within depth
        if depth(dir_path) <= max_depth:
            output.push(dir_entry(dir_path, dir_hash))

    // Push new directories we've entered
    for each new directory component between stack.top() and dir:
        stack.push((new_dir_path, new_hasher))

    // Process this file entry
    child_hash = effective_hash(hash)  // real or synthetic
    stack.top().hasher.update(name + '\0' + child_hash)
    if depth(path) <= max_depth:
        output.push(file_entry(path, hash))

// Finalize remaining directories on the stack
while stack is not empty:
    (dir_path, hasher) = stack.pop()
    dir_hash = hasher.finalize()
    if stack is not empty:
        stack.top().hasher.update(basename(dir_path) + '\0' + dir_hash)
    output.push(dir_entry(dir_path, dir_hash))

root_hash = last finalized hash
sort output by path
return (output, root_hash)
```

**Implementation steps**:

1. Define `FlatEntry`, `EntryType`
2. Implement `synthetic_hash` for non-Blake3 entries
3. Implement the streaming stack-based `compute_manifest` algorithm
4. Write unit tests

**Feedback loop**:

- **Playground**: Unit tests with hand-constructed sorted entry lists
- **Experiment**: Compute manifests from known inputs, verify hashes are deterministic and change correctly when inputs change
- **Check command**: `cargo test merkle`

**Unit tests for merkle.rs**:

- Compute from 3 files in 2 directories, verify correct FlatEntries and root hash
- Same files in different insertion order produce same root hash (entries are sorted before calling)
- Changing one file's hash changes the root hash
- Changing one file changes parent dir hash but NOT sibling dir hashes
- Empty directory (no file entries) has a consistent hash
- max_depth=0: only root entry emitted
- max_depth=1: root + immediate children emitted
- max_depth > tree depth: all entries emitted (same as unlimited)
- Same root hash regardless of max_depth (depth only affects output, not hashing)
- Dataless and error entries contribute to directory hashes via synthetic_hash
- Symlink entries contribute to directory hashes via synthetic_hash
- Verify memory behavior: processing 10K entries doesn't require proportional tree allocation (this is implicit in the design but worth asserting indirectly by checking that it completes without issues)

### 5. manifest.rs - Manifest Output

**Overview**: Write and parse the manifest file format. Streaming output (write entries as they're produced). Round-trip parsing for the compare command.

```rust
use std::io::{Write, BufRead};

pub struct ManifestHeader {
    pub host: String,
    pub path: String,
    pub depth: usize,
    pub date: String,
    pub total_files: usize,
    pub total_dirs: usize,
    pub root_hash: String,
}

pub struct ManifestEntry {
    pub entry_type: EntryType,
    pub value: String,
    pub path: String,
}

/// Write a manifest header and entries to a writer.
pub fn write_manifest<W: Write>(
    writer: &mut W,
    header: &ManifestHeader,
    entries: &[FlatEntry],
) -> io::Result<()> { ... }

/// Parse a manifest file into header + entries.
pub fn parse_manifest<R: BufRead>(reader: R) -> Result<(ManifestHeader, Vec<ManifestEntry>), ParseError> { ... }
```

**Key decisions**:

- Output format: `# comment` header lines, then `type:value  path` data lines (two-space separator, matching sha256sum/b3sum convention)
- Header includes hostname (via `gethostname` syscall or `hostname` crate -- just use std/libc), path, depth, date, file/dir counts, root hash
- Entries are sorted by path (already sorted from Merkle tree flattening)
- Parse is lenient on unknown header fields (forward compatibility) but strict on data lines

**Implementation steps**:

1. Define `ManifestHeader`, `ManifestEntry`, `ParseError`
2. Implement `write_manifest` (header comments + sorted data lines)
3. Implement `parse_manifest` (parse header comments, parse data lines)
4. Write unit tests

**Feedback loop**:

- **Playground**: Unit tests with in-memory buffers
- **Experiment**: Write a manifest, parse it back, compare. Verify round-trip fidelity.
- **Check command**: `cargo test manifest`

**Unit tests for manifest.rs**:

- Round-trip: write manifest with known entries, parse it back, verify header and entries match
- Parse header fields correctly (host, path, depth, date, counts, root hash)
- Parse blake3 entries correctly
- Parse dataless entries correctly
- Parse error entries correctly
- Parse symlink entries correctly
- Reject malformed data lines (missing separator, invalid type prefix)
- Unknown header fields are ignored (not an error)
- Empty manifest (header only, no entries) parses correctly

### 6. main.rs - CLI + Fingerprint Command

**Overview**: Clap-derived CLI with fingerprint subcommand. Orchestrates walk -> hash -> merkle -> manifest pipeline. Writes output to file or stdout. Reports summary to stderr.

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sumpig", about = "Merkle tree directory fingerprinting and comparison")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a fingerprint manifest for a directory tree
    Fingerprint {
        /// Directory to fingerprint
        path: PathBuf,
        /// Output depth (controls manifest granularity, not hashing depth)
        #[arg(short, long, default_value = "6")]
        depth: usize,
        /// Output file (default: <path>/.sumpig-fingerprints/<hostname>.txt)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Worker thread count (default: number of CPU cores)
        #[arg(short, long)]
        jobs: Option<usize>,
        /// Disable default ignore list (hash everything)
        #[arg(long)]
        no_ignore: bool,
    },
    // Compare subcommand added in Phase 2
}
```

**Key decisions**:

- Default output path: `<scanned_path>/.sumpig-fingerprints/<hostname>.txt`. Create the `.sumpig-fingerprints/` directory if it does not exist.
- All progress/status output goes to stderr. Manifest content can go to stdout if `--output -` is specified (future consideration).
- Summary line on stderr after completion: file count, dir count, elapsed time, root hash.
- Exit codes: 0 = success, 2 = usage error.

**Implementation steps**:

1. Define Cli and Commands with clap derive macros
2. Implement fingerprint pipeline: resolve path -> walk -> hash (parallel) -> sort entries -> streaming Merkle computation -> write manifest
3. Handle default output path (hostname detection, directory creation)
4. Print summary to stderr
5. Wire up error handling (exit 2 on bad args, propagate I/O errors)

**Feedback loop**:

- **Playground**: Integration tests with tempfile directories
- **Experiment**: Run on small temp directories, verify output format and content
- **Check command**: `cargo test --test integration`

### 7. Integration Tests

**Overview**: End-to-end CLI tests using assert_cmd and tempfile.

**Test cases for tests/integration.rs**:

- `fingerprint` on a small tree (5 files, 3 dirs) produces valid manifest format
- Manifest has correct header fields (host, path, depth, date, counts)
- Running fingerprint twice on the same tree produces byte-identical output (determinism)
- Modify one file, re-fingerprint: root hash changes, modified file's entry changes
- `--depth 1` produces fewer entries than `--depth 6` but same root hash
- `--output FILE` writes to specified path instead of default
- Default output goes to `<path>/.sumpig-fingerprints/<hostname>.txt`
- `--no-ignore` includes directories that would normally be ignored (create a node_modules/ in fixture)
- `--jobs 1` produces same output as default (determinism regardless of thread count)
- Progress/summary output goes to stderr, not stdout
- Nonexistent path produces error message and exit code 2
- Fingerprint of empty directory succeeds with valid manifest

## Error Handling

| Error Scenario | Handling Strategy |
|---|---|
| Path does not exist | Print error to stderr, exit 2 |
| Path is not a directory | Print error to stderr, exit 2 |
| File unreadable (permission denied) | Record as `error:permission denied` in manifest, continue |
| File unreadable (I/O error) | Record as `error:<description>` in manifest, continue |
| Cannot create output directory | Print error to stderr, exit 1 |
| Cannot write output file | Print error to stderr, exit 1 |

## Failure Modes

| Component | Failure Mode | Trigger | Impact | Mitigation |
|---|---|---|---|---|
| hash.rs | Partial read | File modified during hashing | Inconsistent hash | Accept -- hash reflects what was read; re-run for consistency |
| hash.rs | Dataless misdetection | Flag not set on evicted file | File read fails or returns empty content | Falls through to I/O error path, recorded as error entry |
| walk.rs | Permission denied on directory | Restricted directory in tree | Subtree not walked | Record parent dir with error, log to stderr |
| walk.rs | Symlink loop | Circular symlinks | Infinite recursion | Not followed -- symlinks recorded but not traversed |
| merkle.rs | Large entry list | Millions of files | ~132MB RAM per 1M files for sorted (path, hash) list | Streaming Merkle computation adds only O(depth) extra. The entry list is the floor. |
| manifest.rs | Non-UTF8 filename | Files with invalid Unicode names | Path cannot be written as text | Use lossy UTF-8 conversion, note in error entry |

## Validation Commands

```bash
# Run all checks (test + lint)
just check

# Run all tests
just test

# Run lints
just lint

# Run only tests for a specific module
just test-one hash
just test-one walk
just test-one merkle
just test-one manifest

# Build release binary
just build

# Quick smoke test
just run-fingerprint /tmp/some-test-dir
```

---

_This spec is ready for implementation. Follow the patterns and validate at each step._
