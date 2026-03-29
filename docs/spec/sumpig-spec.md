# sumpig -- Merkle tree directory fingerprinting and comparison

## What this tool does

`sumpig` generates a fingerprint file for a directory tree that captures the content of every
file as a Merkle tree of BLAKE3 hashes. Run it on two machines (or two copies of the same
directory), then compare the fingerprint files to verify they are identical or find exactly
what differs.

The primary use case is verifying iCloud Drive sync between two Macs, but the tool is
general-purpose -- useful for verifying backups, rsync copies, deploy artifacts, or any
scenario where two directory trees should be identical.

## Usage

```
sumpig fingerprint <path> [--depth N] [--output FILE] [--jobs N]
sumpig compare <file1> <file2>
```

### fingerprint

Recursively walks `<path>`, hashes every file with BLAKE3, computes Merkle directory hashes,
and writes a manifest file.

- `--depth N` (default: 6) -- controls output granularity, NOT hashing depth (see below)
- `--output FILE` -- write manifest here; default: `<path>/.sumpig-fingerprints/<hostname>.txt`
- `--jobs N` -- worker thread count; default: number of CPU cores
- Creates the `.sumpig-fingerprints/` directory if it does not exist

### compare

Loads two fingerprint files and reports differences. Uses the Merkle tree property to skip
matching subtrees and focus on what actually differs.

Exit codes: 0 = identical, 1 = differences found, 2 = usage error.

## Critical design concept: depth vs hashing depth

**Hashing is always fully recursive.** A directory hash at any level incorporates ALL files
beneath it, no matter how deep the actual tree goes.

**Depth controls output granularity.** `--depth 6` means the manifest shows individual file
and directory entries down to depth 6 from the root. A directory entry at depth 6 has a hash
that covers everything below it -- even if the actual tree continues another 10 levels deep.

**This means:**
- The root hash is always a complete fingerprint of the entire tree
- If the root hashes match, the trees are identical -- one comparison, done
- If they differ, the manifest shows where the divergence is down to `--depth` level
- To drill deeper into a mismatch, re-run on just that subtree with higher depth

### Example

Given this tree:
```
docs/
  readme.md
  src/
    main.rs
    utils/
      helpers.rs
```

With `--depth 1`, the manifest contains:
```
blake3:abc123  ./
blake3:def456  ./docs/            # hash covers readme.md + src/ + utils/ + helpers.rs
blake3:789abc  ./other_dir/
```

The `docs/` hash (def456) is the Merkle hash of everything inside it. If that hash matches
on both machines, everything inside docs/ is identical -- no need to check further.

With `--depth 3`, the same tree shows more detail:
```
blake3:abc123  ./
blake3:def456  ./docs/
blake3:aaa111  ./docs/readme.md
blake3:bbb222  ./docs/src/
blake3:ccc333  ./docs/src/main.rs
blake3:ddd444  ./docs/src/utils/  # hash covers helpers.rs
```

## Merkle tree construction

### What is a Merkle tree?

A Merkle tree is a hash tree where each node's hash covers all of its descendants. Changing
any file anywhere in the tree causes hash changes to propagate all the way up to the root.
Git uses this exact structure -- tree objects are Merkle hashes of their contents.

### How sumpig builds the tree

Bottom-up construction:

1. **Leaf nodes (files):** `blake3(file_contents)` -- hash the raw bytes of the file
2. **Interior nodes (directories):** Sort children by name, concatenate
   `child_name + '\0' + child_hash` for each child, then `blake3(concatenated_bytes)`
3. **Repeat up to root**

The sort-by-name step is critical -- it ensures the hash is deterministic regardless of
filesystem enumeration order. The null byte separator prevents ambiguity between names.

### Why this is efficient for comparison

Comparing two Merkle trees:
1. Compare root hashes. If equal, trees are identical. Done.
2. If unequal, compare child hashes of the root directory
3. Recurse only into children with mismatched hashes
4. Converge on the exact changed files in O(changes * depth) comparisons

For a tree with 1 million files where 3 files differ, you might compare ~50 hashes instead
of 1 million.

## Output format

Flat text, one line per entry, sorted by path. Diffable with standard `diff`.

```
# sumpig fingerprint
# version: 1
# host: cardinal
# path: /Users/tednaleid/Documents
# depth: 6
# date: 2026-03-28T15:30:00
# total_files: 41816
# total_dirs: 3200
# root: a1b2c3d4e5f67890a1b2c3d4e5f67890
blake3:a1b2c3d4e5f67890a1b2c3d4e5f67890  ./
blake3:f6e5d4c3b2a1f6e5f6e5d4c3b2a1f6e5  ./archives/
blake3:1a2b3c4d5e6f1a2b1a2b3c4d5e6f1a2b  ./archives/workspace/
blake3:6f5e4d3c2b1a6f5e6f5e4d3c2b1a6f5e  ./archives/workspace/file.txt
blake3:deadbeefdeadbeefdeadbeefdeadbeef  ./photos/
blake3:cafebabecafebabecafebabecafebabe  ./photos/vacation.jpg
```

Hashes are truncated to 32 hex characters (128 bits) in the manifest for readability. This is
still astronomically collision-resistant for file comparison purposes.

Directories are identifiable by their trailing `/`.

### Why not YAML/JSON?

- `diff file1.txt file2.txt` works immediately with no tooling
- Streaming output -- no need to buffer the entire tree in memory
- Simple to parse (split on two spaces)
- Same convention as sha256sum, b3sum, md5sum

## What to ignore

### Directories to ignore (not hashed, not listed)

- `node_modules` -- reproducible from package.json
- `.venv`, `venv` -- reproducible from requirements
- `target` -- Rust build artifacts
- `__pycache__` -- Python bytecode cache
- `build`, `dist` -- build output
- `.Trash` -- macOS trash
- `*.nosync` -- explicitly excluded from iCloud sync
- `.sumpig-fingerprints` -- sumpig's own output directory

### Files to ignore

- `.DS_Store` -- macOS Finder metadata, differs per machine
- `.localized` -- macOS localization markers

### What NOT to ignore

- `.git` directories -- MUST be included. iCloud sync can corrupt git objects, and detecting
  this is one of the primary motivations for this tool.

### Dataless files (macOS FPFS)

On macOS Tahoe, iCloud can evict files to save disk space. These "dataless" files have the
`SF_DATALESS` flag (0x40000000) set in stat flags. Their data fork is empty/not present
locally.

When sumpig encounters a dataless file:
- Record it as `dataless:<file_size>` instead of `blake3:<hash>`
- The compare command should flag dataless entries as warnings since the content cannot be
  verified
- Print a summary of dataless file count after fingerprinting

## Language and crate choices: Rust

### Core dependencies

| Crate | Purpose | Why this one |
|-------|---------|-------------|
| `blake3` | BLAKE3 hashing | Official impl, SIMD auto-detection (NEON on Apple Silicon), ~8.4 GB/s, Rayon integration for multi-threaded large file hashing |
| `jwalk` | Parallel directory walking | ~4x faster than walkdir, rayon-based, sorted output with metadata |
| `rayon` | Parallelism | Work-stealing thread pool, `par_iter()` for easy parallelism. Used by both blake3 and jwalk |
| `clap` | CLI argument parsing | Derive macros for subcommands, auto-generated help |
| `criterion` | Benchmarks | Statistical benchmarks with regression detection, HTML reports |

### Dev/test dependencies

| Crate | Purpose |
|-------|---------|
| `tempfile` | Temporary directories for integration tests |
| `assert_cmd` | CLI integration testing (invoke binary, assert output) |
| `predicates` | Assertion helpers for assert_cmd |

## Architecture

Inspired by ripgrep's modular crate structure, but as a single crate with well-separated
modules (a workspace is overkill for this scope).

### Module structure

```
sumpig/
  Cargo.toml
  src/
    main.rs          # CLI entry point, clap subcommand routing
    walk.rs          # directory walking with ignore logic, jwalk configuration
    hash.rs          # file hashing (blake3), dataless detection, buffer management
    merkle.rs        # Merkle tree construction: DirNode, bottom-up hash computation
    manifest.rs      # output format: writing and parsing manifest files
    compare.rs       # two-manifest comparison using Merkle property
  benches/
    hash_bench.rs    # criterion benchmarks for file hashing throughput
    walk_bench.rs    # criterion benchmarks for directory walking
    merkle_bench.rs  # criterion benchmarks for tree construction
  tests/
    integration.rs   # end-to-end CLI tests with tempdir fixtures
```

### Key types

```rust
/// A node in the Merkle tree (either a file or directory)
enum TreeNode {
    File {
        name: String,
        hash: [u8; 32],       // blake3 hash of file content
    },
    Dataless {
        name: String,
        size: u64,             // file size from stat
    },
    Dir {
        name: String,
        hash: [u8; 32],       // merkle hash of sorted children
        children: Vec<TreeNode>,
    },
}

/// A parsed manifest entry (from a fingerprint file)
struct ManifestEntry {
    hash_type: HashType,       // Blake3 or Dataless
    hash: String,              // hex string (or size for dataless)
    path: String,              // relative path
}
```

### Data flow

```
fingerprint command:
  1. jwalk parallel walk (respecting ignore list)
     --> stream of (path, metadata) entries
  2. rayon par_iter: hash each file with blake3
     --> Vec<(path, hash)>
  3. build Merkle tree bottom-up from sorted entries
     --> TreeNode root with recursive hashes
  4. serialize tree to manifest format (depth-limited)
     --> write to output file

compare command:
  1. parse both manifest files into Vec<ManifestEntry>
  2. build path->hash maps for each
  3. walk both trees, comparing hashes
     - matching directory hash? skip children
     - different directory hash? recurse
     - file only in one? report
  4. print diff report
```

## Performance strategy

### Parallelism model (inspired by ripgrep)

- **Directory walking**: jwalk handles parallel traversal with rayon work-stealing. Each
  directory is processed by whichever thread is free (work-stealing prevents idle threads).
- **File hashing**: `rayon::par_iter()` over collected file paths. Each thread reads and
  hashes files independently. Per-thread read buffers avoid allocation contention.
- **Large file optimization**: For files above a threshold (e.g. 1MB), use blake3's
  `update_rayon()` which hashes a single file across multiple threads using BLAKE3's
  internal tree structure.
- **I/O strategy**: Buffered reads by default (64KB buffer). Memory-mapped I/O is tempting
  but ripgrep found it hurts on macOS due to kernel overhead. Start with buffered reads,
  benchmark, add mmap as an option only if it helps.

### Avoiding allocations in hot paths

- Pre-allocate read buffer per thread (reuse across files)
- Use `Vec::with_capacity()` when collection sizes are predictable
- Avoid string formatting in the hashing loop
- Sort entries in-place rather than creating new collections

## Testing strategy

### Unit tests (per module, run with `cargo test`)

Each module should have `#[cfg(test)] mod tests` with focused tests:

**hash.rs tests:**
- Hash a known string, verify against reference blake3 hash
- Hash an empty file
- Hash a large file (verify streaming works correctly)
- Dataless file detection (mock the stat flags)

**merkle.rs tests:**
- Build tree from 3 files, verify root hash
- Same files in different insertion order produce same root hash (determinism)
- Adding a file changes root hash
- Changing one file changes root hash and parent dir hashes but not sibling hashes
- Empty directory has a consistent hash
- Depth-limited serialization includes correct entries

**manifest.rs tests:**
- Round-trip: write manifest, parse it back, compare
- Parse header fields correctly
- Handle dataless entries
- Reject malformed input

**compare.rs tests:**
- Identical manifests: report match, exit 0
- One file differs: report exactly that file and its parent dirs
- File only in one manifest: report correctly
- Merkle skip: matching directory hash skips children (verify by counting comparisons)
- Dataless entries produce warnings

**walk.rs tests:**
- Ignore list: node_modules, .DS_Store, etc. are excluded
- .git directories ARE included
- Symlinks are not followed
- .sumpig-fingerprints directory is excluded

### Integration tests (tests/integration.rs)

Use `tempfile::TempDir` to create directory fixtures. Use `assert_cmd` to invoke the
compiled binary and assert on stdout/stderr/exit code.

- `fingerprint` on a small tree produces correct format
- `fingerprint` run twice on same tree produces byte-identical output
- Modify one file, re-fingerprint, root hash changes
- `compare` two identical manifests: exit 0, reports "identical"
- `compare` two different manifests: exit 1, reports exact differences
- `--depth 1` vs `--depth 6`: same root hash, different number of entries
- `--output` flag writes to specified path
- Default output goes to `.sumpig-fingerprints/<hostname>.txt`
- Progress output goes to stderr (not mixed with manifest on stdout)

### Criterion benchmarks (benches/)

**hash_bench.rs:**
- Benchmark hashing throughput for various file sizes (1KB, 100KB, 1MB, 100MB)
- Compare single-threaded vs rayon-parallel hashing for large files
- Measure overhead of file open/read/close cycle for small files

**walk_bench.rs:**
- Benchmark directory walking speed on a synthetic tree (create with tempdir)
- Measure with/without ignore filtering
- Compare sequential vs parallel walking

**merkle_bench.rs:**
- Benchmark tree construction from N pre-computed hashes (1K, 10K, 100K entries)
- Benchmark depth-limited serialization

Run benchmarks with `cargo bench`. Criterion produces HTML reports in `target/criterion/`
with statistical analysis and regression detection between runs.

### Performance regression testing

- Keep a baseline benchmark run committed (criterion supports `--save-baseline`)
- CI can compare against baseline to catch regressions
- Key metrics to track: files/second throughput, bytes/second hashing throughput

## Compare output format

When `sumpig compare file1.txt file2.txt` finds differences:

```
Root hashes differ.

Changed directories:
  ./archives/workspace/rust/ckrs/.git/    cardinal:abc123  macstudio:def456

Changed files:
  ./archives/workspace/rust/ckrs/.git/index    cardinal:111aaa  macstudio:222bbb
  ./archives/workspace/rust/ckrs/.git/HEAD     cardinal:333ccc  macstudio:444ddd

Only in cardinal:
  ./archives/workspace/newproject/

Only in macstudio:
  (none)

Dataless warnings:
  (none)

Summary: 2 files differ, 1 directory only in cardinal, 0 only in macstudio
```

## Project setup

```bash
cargo init sumpig
cd sumpig
cargo add blake3 --features rayon
cargo add jwalk
cargo add rayon
cargo add clap --features derive
cargo add --dev criterion tempfile assert_cmd predicates
```

Add to Cargo.toml:
```toml
[[bench]]
name = "hash_bench"
harness = false

[[bench]]
name = "walk_bench"
harness = false

[[bench]]
name = "merkle_bench"
harness = false
```

## Deliverables

When implementing, the plan file also asks for:
- `docs/merkle-tree-primer.md` in the sumpig project -- explains Merkle trees, how
  sumpig uses them, and why they make comparison efficient. Written for someone who
  hasn't encountered the concept before.

## Verification

1. `sumpig fingerprint .` on the sumpig project itself -- verify output format
2. Run twice on same directory -- output should be byte-identical
3. Modify one file, re-run -- root hash changes, file entry changes
4. `sumpig compare` on two identical manifests -- reports identical, exit 0
5. `sumpig compare` on two different manifests -- reports exact differences, exit 1
6. Test with `--depth 1` vs `--depth 6` -- same root hash, different granularity
7. `cargo bench` -- all benchmarks run, establish baseline
8. Large-scale: run on a real directory with thousands of files, verify it completes fast
