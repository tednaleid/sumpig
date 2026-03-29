use std::io::Write;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tempfile::NamedTempFile;

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

criterion_group!(benches, bench_hash_file);
criterion_main!(benches);
