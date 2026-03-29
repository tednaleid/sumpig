# Implementation Spec: sumpig - Progress Reporting + Parallel Hashing

**Contract**: ./contract.md
**Estimated Effort**: S

## Technical Approach

Two coupled improvements to the fingerprint command:

1. **Parallel file hashing**: Convert the hash phase from sequential `.map()` to `rayon::par_iter()`.
   The spec already calls for parallel hashing, but the Phase 1 implementation used sequential iteration.
   This is a correctness fix relative to the spec and a significant performance improvement.

2. **Progress reporting**: Add indicatif-based progress feedback to stderr so the user knows the tool
   is working during long runs. A spinner during the walk phase and a progress bar with ETA during
   the hash phase.

Both changes are in `src/main.rs` only. The library modules (hash, walk, merkle, manifest) are unchanged.

## Feedback Strategy

**Inner-loop command**: `just check`

**Playground**: Integration tests + manual runs on large directories

**Why this approach**: The changes are in the CLI orchestration layer. Existing tests verify correctness;
manual runs verify the UX.

## File Changes

### Modified Files

| File Path | Changes |
|---|---|
| `Cargo.toml` | Add `indicatif = "0.17"` to dependencies |
| `src/main.rs` | Add `--quiet` flag, convert hash to `par_iter`, add spinner + progress bar |
| `tests/integration.rs` | Add test for `--quiet` flag |

### New Files

None.

## Implementation Details

### 1. Add indicatif dependency

Add to `[dependencies]` in Cargo.toml:

```toml
indicatif = "0.17"
```

indicatif has minimal transitive dependencies (console, number_prefix, portable-atomic). It
automatically suppresses drawing when stderr is not a terminal (non-TTY safe).

### 2. Add --quiet flag

Add to the `Fingerprint` variant in the `Commands` enum:

```rust
/// Suppress progress bars and summary output
#[arg(short, long)]
quiet: bool,
```

Thread through to `run_fingerprint()`. When quiet is true, no progress bars are created and
the summary `eprintln!` is skipped. The `-q` short flag follows Unix convention.

### 3. Convert hash phase to parallel

The current sequential code:

```rust
let hashed_entries: Vec<_> = walk_entries
    .into_iter()
    .filter(|e| !e.is_dir)
    .map(|e| { /* hash */ })
    .collect();
```

Becomes:

```rust
use rayon::prelude::*;

let files_to_hash: Vec<_> = walk_entries
    .into_iter()
    .filter(|e| !e.is_dir)
    .collect();

let file_count = files_to_hash.len();

let hashed_entries: Vec<_> = files_to_hash
    .into_par_iter()
    .map(|e| { /* hash + pb.inc(1) */ })
    .collect();
```

The intermediate collect is needed to get the file count for the progress bar before iteration starts.

`hash_file()` is safe to call from multiple threads: it uses only local state (stack buffer, hasher)
and reads files via `fs::File::open()` which is thread-safe. The `update_rayon()` call for large
files uses rayon's global pool, which handles nested parallelism via work-stealing.

### 4. Walk phase spinner

```rust
let spinner = ProgressBar::new_spinner();
spinner.set_style(ProgressStyle::with_template("  {spinner} Scanning...").unwrap());
spinner.enable_steady_tick(Duration::from_millis(120));

let walk_entries = sumpig::walk::walk_directory(&canonical, &walk_options);

spinner.finish_and_clear();
```

The walk returns a complete `Vec<WalkEntry>`, so we cannot show a live count without changing
the library API. A spinner is sufficient since the walk phase is fast (~2s for 280K files).

### 5. Hash phase progress bar

```rust
let pb = ProgressBar::new(file_count as u64);
pb.set_style(
    ProgressStyle::with_template(
        "  Hashing  [{bar:30}] {pos}/{len}  {percent}%  {eta} remaining"
    )
    .unwrap()
    .progress_chars("##-"),
);

let hashed_entries: Vec<_> = files_to_hash
    .into_par_iter()
    .map(|e| {
        let result = hash_file(&full_path);
        pb.inc(1);
        result
    })
    .collect();

pb.finish_and_clear();
```

`ProgressBar` is `Send + Sync`. The `inc()` method uses an internal `AtomicU64`, so no mutex
contention from parallel threads. indicatif rate-limits redraws to ~20/s regardless of inc() frequency.

Both spinner and progress bar are gated on `!quiet`. Wrapped in `Option<ProgressBar>` so the
hot loop has only a cheap `None` check when quiet.

### 6. Suppress summary with --quiet

Wrap the existing `eprintln!` summary in `if !quiet { ... }`.

## Testing Requirements

### Existing tests (unchanged)

All 55 existing tests continue to pass. Key validators for parallel correctness:
- `fingerprint_deterministic` -- same output from two runs
- `jobs_one_same_output_as_default` -- same output regardless of thread count

### New test

| Test | Coverage |
|---|---|
| `quiet_flag_suppresses_summary` | `--quiet` produces empty stderr, manifest still written correctly |

## Error Handling

| Error Scenario | Handling Strategy |
|---|---|
| indicatif on non-TTY stderr | Automatic: indicatif suppresses drawing, no code needed |
| rayon thread panic in hash_file | Rayon propagates panics; existing error handling in hash_file returns FileHash::Error instead of panicking |

## Validation Commands

```bash
# All tests pass
just check

# Manual: see progress bar on large directory
just install && sumpig fingerprint <large-dir>

# Manual: quiet mode
sumpig fingerprint <large-dir> --quiet

# Manual: determinism with parallel hashing
sumpig fingerprint . --output /tmp/a.txt
sumpig fingerprint . --output /tmp/b.txt
diff <(grep -v '# date' /tmp/a.txt) <(grep -v '# date' /tmp/b.txt)
```

---

_This spec is ready for implementation._
