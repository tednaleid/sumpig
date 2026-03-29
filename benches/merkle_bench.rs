use std::path::PathBuf;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use sumpig::hash::FileHash;
use sumpig::merkle;

/// Generate sorted (PathBuf, FileHash) entries with a realistic directory structure.
/// Creates entries like: dir_0000/file_0000.txt, dir_0000/file_0001.txt, etc.
fn generate_sorted_entries(count: usize) -> Vec<(PathBuf, FileHash)> {
    let files_per_dir = 20;
    let mut entries = Vec::with_capacity(count);

    for i in 0..count {
        let dir_idx = i / files_per_dir;
        let file_idx = i % files_per_dir;
        let path = PathBuf::from(format!("dir_{dir_idx:04}/file_{file_idx:04}.txt"));

        // Generate a deterministic hash for each entry.
        let content = format!("content_{i}");
        let hash = *blake3::hash(content.as_bytes()).as_bytes();
        entries.push((path, FileHash::Blake3(hash)));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

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
