# Implementation Spec: sumpig - Phase 3 (Benchmarks)

**Contract**: ./contract.md
**Estimated Effort**: S

## Technical Approach

Add criterion benchmarks for the three performance-critical paths: file hashing throughput, directory walking speed, and Merkle tree construction. Establish baselines for regression detection. Each benchmark file tests a focused subsystem with multiple input sizes to characterize performance across the expected range.

Criterion produces HTML reports in `target/criterion/` with statistical analysis, confidence intervals, and automatic regression detection between runs.

## Feedback Strategy

**Inner-loop command**: `cargo bench`

**Playground**: Criterion benchmark harness with tempfile fixtures

**Why this approach**: Benchmarks are the artifact. Running them validates they work and produces the performance data.

## File Changes

### New Files

| File Path | Purpose |
|---|---|
| `benches/hash_bench.rs` | BLAKE3 hashing throughput benchmarks for various file sizes |
| `benches/walk_bench.rs` | Directory walking speed benchmarks on synthetic trees |
| `benches/merkle_bench.rs` | Merkle tree construction benchmarks for various entry counts |

### Modified Files

| File Path | Changes |
|---|---|
| `Cargo.toml` | Add criterion dev-dependency, add `[[bench]]` sections |

## Implementation Details

### 1. Cargo.toml Changes

Add to dev-dependencies and bench configuration:

```toml
[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
# ... existing dev-dependencies ...

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

### 2. hash_bench.rs - Hashing Throughput

**Overview**: Benchmark BLAKE3 hashing throughput across file sizes that represent the real-world distribution: many small files (source code, configs) and fewer large files (binaries, media).

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use tempfile::NamedTempFile;
use std::io::Write;
use sumpig::hash;

fn bench_hash_file(c: &mut Criterion) {
    let sizes: &[(& str, usize)] = &[
        ("1KB", 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("hash_file");
    for (name, size) in sizes {
        // Create temp file with random-ish content
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("blake3", name), size, |b, &size| {
            let file = create_temp_file(size);
            b.iter(|| hash::hash_file(file.path()));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hash_file);
criterion_main!(benches);
```

**Key decisions**:

- Use `Throughput::Bytes` so criterion reports GB/s, the natural unit for hashing benchmarks
- Create temp files once per benchmark group, not per iteration (amortize setup)
- Include a 10MB file to exercise the large-file rayon path (if the threshold is 1MB)
- Skip 100MB -- too slow for routine benchmark runs. Can be added as a separate group with `--bench` filtering.

**Benchmarks**:

- `hash_file/blake3/1KB` -- dominated by open/read/close overhead
- `hash_file/blake3/100KB` -- typical source file range
- `hash_file/blake3/1MB` -- threshold boundary for rayon hashing
- `hash_file/blake3/10MB` -- exercises multi-threaded hashing path

### 3. walk_bench.rs - Directory Walking Speed

**Overview**: Benchmark directory traversal speed on synthetic trees of various sizes. Measures the overhead of walking + skip filtering independent of hashing.

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use tempfile::TempDir;
use sumpig::walk::{self, WalkOptions};

fn bench_walk(c: &mut Criterion) {
    let sizes: &[(&str, usize, usize)] = &[
        // (name, num_dirs, files_per_dir)
        ("100_files", 10, 10),
        ("1K_files", 50, 20),
        ("10K_files", 100, 100),
    ];

    let mut group = c.benchmark_group("walk_directory");
    for (name, num_dirs, files_per_dir) in sizes {
        let dir = create_synthetic_tree(*num_dirs, *files_per_dir);
        group.bench_with_input(BenchmarkId::new("parallel", name), &dir, |b, dir| {
            let options = WalkOptions { skip_defaults: true, num_threads: 0 };
            b.iter(|| walk::walk_directory(dir.path(), &options));
        });
    }
    group.finish();
}

fn bench_walk_skip_filtering(c: &mut Criterion) {
    // Create tree with skippable directories mixed in
    let dir = create_tree_with_skippable_dirs();
    let mut group = c.benchmark_group("walk_skip");
    group.bench_function("with_skip", |b| {
        let options = WalkOptions { skip_defaults: true, num_threads: 0 };
        b.iter(|| walk::walk_directory(dir.path(), &options));
    });
    group.bench_function("no_skip", |b| {
        let options = WalkOptions { skip_defaults: false, num_threads: 0 };
        b.iter(|| walk::walk_directory(dir.path(), &options));
    });
    group.finish();
}

criterion_group!(benches, bench_walk, bench_walk_skip_filtering);
criterion_main!(benches);
```

**Key decisions**:

- Create fixture trees once (outside the benchmark loop) using tempfile::TempDir
- Files contain minimal content (1 byte) -- we're measuring walk speed, not I/O
- Include a skip filtering benchmark to measure the overhead/savings of the skip list
- Cap at 10K files for routine runs. Larger trees can be benchmarked manually.

**Benchmarks**:

- `walk_directory/parallel/100_files` -- small project scale
- `walk_directory/parallel/1K_files` -- medium project scale
- `walk_directory/parallel/10K_files` -- large project scale
- `walk_skip/with_skip` -- skip filtering overhead
- `walk_skip/no_skip` -- baseline without filtering

### 4. merkle_bench.rs - Streaming Merkle Computation

**Overview**: Benchmark the streaming Merkle hash computation from pre-computed sorted entries. Isolates the directory hash computation overhead from file I/O. Tests both the throughput at various entry counts and the effect of depth limiting.

```rust
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};
use sumpig::hash::FileHash;
use sumpig::merkle;
use std::path::PathBuf;

fn bench_compute_manifest(c: &mut Criterion) {
    let sizes: &[(&str, usize)] = &[
        ("1K_entries", 1_000),
        ("10K_entries", 10_000),
        ("100K_entries", 100_000),
    ];

    let mut group = c.benchmark_group("compute_manifest");
    for (name, count) in sizes {
        let entries = generate_sorted_entries(*count);
        group.bench_with_input(BenchmarkId::new("entries", name), &entries, |b, entries| {
            b.iter(|| merkle::compute_manifest(entries, 6));
        });
    }
    group.finish();
}

fn bench_depth_variation(c: &mut Criterion) {
    let entries = generate_sorted_entries(10_000);

    let mut group = c.benchmark_group("compute_manifest_depth");
    for depth in [1, 3, 6, 20] {
        group.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, &depth| {
            b.iter(|| merkle::compute_manifest(&entries, depth));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_compute_manifest, bench_depth_variation);
criterion_main!(benches);
```

**Key decisions**:

- Pre-generate sorted path+hash entries outside the benchmark loop
- Generate realistic path structures (nested directories, varying depth) to exercise the streaming algorithm accurately
- Benchmark depth variation separately to measure its impact on output size and computation time
- Include 100K entries to match real-world scale (40K+ files)

**Benchmarks**:

- `compute_manifest/entries/1K_entries` -- small project
- `compute_manifest/entries/10K_entries` -- medium project
- `compute_manifest/entries/100K_entries` -- large project (primary target)
- `compute_manifest_depth/depth/1` -- minimal output, mostly directory hash computation
- `compute_manifest_depth/depth/6` -- default depth
- `compute_manifest_depth/depth/20` -- effectively unlimited, maximum output

## Validation Commands

```bash
# Run all benchmarks
just bench

# Run a specific benchmark group
just bench -- hash_file
just bench -- walk_directory
just bench -- build_tree

# Save a baseline for regression comparison
just bench -- --save-baseline initial

# Compare against baseline
just bench -- --baseline initial

# View HTML reports (after running benchmarks)
open target/criterion/report/index.html

# Run full checks to make sure nothing broke
just check
```

## Notes

- Criterion auto-detects regressions by comparing against the previous run. No manual baseline management is required for day-to-day development.
- `--save-baseline` is useful for marking a known-good state before optimization work.
- HTML reports in `target/criterion/` include violin plots, confidence intervals, and iteration time distributions.
- Add `target/` to .gitignore (standard Rust convention, already in place).

---

_This spec is ready for implementation. Follow the patterns and validate at each step._
