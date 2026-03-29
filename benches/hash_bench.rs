use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rayon::prelude::*;
use tempfile::NamedTempFile;

/// Root directory for persistent benchmark fixtures.
/// Lives under target/ so it's gitignored and cleaned with `cargo clean`.
const FIXTURE_DIR: &str = "target/bench-fixtures";

fn create_temp_file(size: usize) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    // Fill with repeating bytes (not zeros, to avoid compression artifacts).
    let chunk = vec![0xABu8; 8192];
    let mut remaining = size;
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        file.write_all(&chunk[..n]).unwrap();
        remaining -= n;
    }
    file.flush().unwrap();
    file
}

/// Write a file with the given size if it doesn't already exist.
fn ensure_file(path: &Path, size: usize) {
    if path.exists() {
        // Verify size matches; recreate if not.
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() == size as u64 {
                return;
            }
        }
    }
    let chunk = vec![0xABu8; size.min(8192)];
    let mut f = fs::File::create(path).unwrap();
    let mut remaining = size;
    while remaining > 0 {
        let n = remaining.min(chunk.len());
        f.write_all(&chunk[..n]).unwrap();
        remaining -= n;
    }
}

/// Get or create a set of persistent fixture files for parallel benchmarks.
/// Files live in target/bench-fixtures/<name>/ and are reused across runs.
fn fixture_file_set(name: &str, count: usize, size: usize) -> (Vec<PathBuf>, u64) {
    let dir = PathBuf::from(FIXTURE_DIR).join(name);
    fs::create_dir_all(&dir).unwrap();
    let mut paths = Vec::with_capacity(count);
    for i in 0..count {
        let path = dir.join(format!("file_{i:06}.bin"));
        ensure_file(&path, size);
        paths.push(path);
    }
    let total = count as u64 * size as u64;
    (paths, total)
}

fn bench_hash_file(c: &mut Criterion) {
    let sizes: &[(&str, usize)] = &[
        ("1KB", 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("hash_file");
    for (name, size) in sizes {
        let file = create_temp_file(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("blake3", name), size, |b, _| {
            b.iter(|| sumpig::hash::hash_file(file.path()));
        });
    }
    group.finish();
}

/// Benchmark parallel hashing of many files, matching the real fingerprint pipeline.
/// This catches issues like thread contention that per-file benchmarks miss.
/// Fixture files are created once in target/bench-fixtures/ and reused.
fn bench_hash_parallel(c: &mut Criterion) {
    let configs: &[(&str, usize, usize)] = &[
        ("1K_x_10KB", 1000, 10 * 1024),
        ("100_x_1MB", 100, 1024 * 1024),
        ("10_x_10MB", 10, 10 * 1024 * 1024),
        // Mixed workload closer to real directory trees: many small files + some large ones.
        ("10K_x_100KB", 10_000, 100 * 1024),
    ];

    let mut group = c.benchmark_group("hash_parallel");
    for (name, count, size) in configs {
        let (paths, total_bytes) = fixture_file_set(name, *count, *size);
        group.throughput(Throughput::Bytes(total_bytes));
        group.bench_with_input(BenchmarkId::new("par_iter", name), &paths, |b, paths| {
            b.iter(|| {
                let _results: Vec<_> = paths
                    .par_iter()
                    .map(|p| sumpig::hash::hash_file(p))
                    .collect();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_hash_file, bench_hash_parallel);
criterion_main!(benches);
