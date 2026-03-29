use std::fs;
use std::io::Write;
use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rayon::prelude::*;
use tempfile::{NamedTempFile, TempDir};

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

/// Create a directory of files for parallel hashing benchmarks.
/// Returns (temp_dir, file_paths, total_bytes).
fn create_file_set(count: usize, size: usize) -> (TempDir, Vec<PathBuf>, u64) {
    let dir = TempDir::new().unwrap();
    let chunk = vec![0xABu8; size.min(8192)];
    let mut paths = Vec::with_capacity(count);
    for i in 0..count {
        let path = dir.path().join(format!("file_{i:06}.bin"));
        let mut f = fs::File::create(&path).unwrap();
        let mut remaining = size;
        while remaining > 0 {
            let n = remaining.min(chunk.len());
            f.write_all(&chunk[..n]).unwrap();
            remaining -= n;
        }
        paths.push(path);
    }
    let total = count as u64 * size as u64;
    (dir, paths, total)
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
fn bench_hash_parallel(c: &mut Criterion) {
    let configs: &[(&str, usize, usize)] = &[
        ("1K_x_10KB", 1000, 10 * 1024),
        ("100_x_1MB", 100, 1024 * 1024),
        ("10_x_10MB", 10, 10 * 1024 * 1024),
    ];

    let mut group = c.benchmark_group("hash_parallel");
    for (name, count, size) in configs {
        let (_dir, paths, total_bytes) = create_file_set(*count, *size);
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
