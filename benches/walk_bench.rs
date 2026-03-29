use std::fs;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

use sumpig::walk::{self, WalkOptions};

fn create_synthetic_tree(num_dirs: usize, files_per_dir: usize) -> TempDir {
    let dir = TempDir::new().unwrap();
    for d in 0..num_dirs {
        let subdir = dir.path().join(format!("dir_{d:04}"));
        fs::create_dir(&subdir).unwrap();
        for f in 0..files_per_dir {
            fs::write(subdir.join(format!("file_{f:04}.txt")), "x").unwrap();
        }
    }
    dir
}

fn create_tree_with_ignorable_dirs() -> TempDir {
    let dir = TempDir::new().unwrap();
    // Regular directories.
    for d in 0..10 {
        let subdir = dir.path().join(format!("src_{d:02}"));
        fs::create_dir(&subdir).unwrap();
        for f in 0..20 {
            fs::write(subdir.join(format!("file_{f:02}.txt")), "x").unwrap();
        }
    }
    // Directories that should be ignored by default.
    for ignored in &["node_modules", "target", ".venv", "__pycache__"] {
        let subdir = dir.path().join(ignored);
        fs::create_dir(&subdir).unwrap();
        for f in 0..50 {
            fs::write(subdir.join(format!("file_{f:02}.txt")), "x").unwrap();
        }
    }
    dir
}

fn bench_walk(c: &mut Criterion) {
    let sizes: &[(&str, usize, usize)] = &[
        ("100_files", 10, 10),
        ("1K_files", 50, 20),
        ("10K_files", 100, 100),
    ];

    let mut group = c.benchmark_group("walk_directory");
    for (name, num_dirs, files_per_dir) in sizes {
        let dir = create_synthetic_tree(*num_dirs, *files_per_dir);
        group.bench_with_input(BenchmarkId::new("parallel", name), &dir, |b, dir| {
            let options = WalkOptions {
                use_default_ignores: true,
                num_threads: 0,
            };
            b.iter(|| walk::walk_directory(dir.path(), &options));
        });
    }
    group.finish();
}

fn bench_walk_ignore_filtering(c: &mut Criterion) {
    let dir = create_tree_with_ignorable_dirs();
    let mut group = c.benchmark_group("walk_ignore");
    group.bench_function("with_ignore", |b| {
        let options = WalkOptions {
            use_default_ignores: true,
            num_threads: 0,
        };
        b.iter(|| walk::walk_directory(dir.path(), &options));
    });
    group.bench_function("no_ignore", |b| {
        let options = WalkOptions {
            use_default_ignores: false,
            num_threads: 0,
        };
        b.iter(|| walk::walk_directory(dir.path(), &options));
    });
    group.finish();
}

criterion_group!(benches, bench_walk, bench_walk_ignore_filtering);
criterion_main!(benches);
