# Performance Notes

## Default mode (metadata fingerprinting)

By default, sumpig hashes file metadata (size + modification time) instead of reading file
contents. This skips all file I/O beyond stat calls, which is fast enough for routine use:

| Mode | Wall clock | User CPU | System CPU |
|------|-----------|----------|-----------|
| default (metadata) | 5.3s | 0.9s | 18.9s |
| `--verify-contents` | 28.1s | 65.1s | 41.0s |

~5x faster on a ~40K file directory tree. The Merkle tree structure is identical in both
modes -- only the leaf hashes differ (metadata hash vs content hash). The manifest header
records the mode (`# mode: fast` vs `# mode: content`) and compare warns if modes differ.

Metadata mode detects file additions, deletions, and size/timestamp changes. It cannot
detect corruption that preserves file size and modification time.

## Content verification (--verify-contents / -C)

sumpig uses two levels of parallelism:

1. **Directory walking**: jwalk traverses the filesystem using rayon work-stealing threads.
2. **File hashing**: `rayon::par_iter()` hashes multiple files concurrently. Each file is
   hashed single-threaded using BLAKE3 with 64KB buffered reads.

This combination keeps all CPU cores busy while maintaining bounded memory usage. Each
hashing thread uses one 64KB buffer regardless of file size.

### Observed throughput (Apple Silicon, SSD)

Per-file BLAKE3 hashing throughput (single-threaded, criterion benchmarks):

| File size | Throughput |
|-----------|-----------|
| 1KB       | ~84 MiB/s |
| 100KB     | ~1.8 GiB/s |
| 1MB       | ~2.2 GiB/s |
| 10MB      | ~2.2 GiB/s |

Small files are dominated by file-open overhead. Larger files saturate at around 2.2 GiB/s
per thread, which is the single-threaded BLAKE3 rate on this hardware. With file-level
parallelism across all cores, aggregate throughput is much higher.

Aggregate parallel throughput (par_iter over many files, criterion benchmarks):

| Workload | Aggregate throughput |
|----------|---------------------|
| 1000 x 10KB files  | ~600 MiB/s |
| 100 x 1MB files    | ~12 GiB/s  |
| 10 x 10MB files    | ~13 GiB/s  |
| 10000 x 100KB files | ~8.8 GiB/s |

Real-world content verification on a ~40K file directory tree: ~28 seconds wall-clock time.

## Benchmarking methodology

The `hash_parallel` benchmark group (in `benches/hash_bench.rs`) tests hashing many files
concurrently via `par_iter`, matching the real fingerprint pipeline. Fixture files are
created once in `target/bench-fixtures/` and reused across runs so file creation overhead
doesn't pollute measurements.

This is critical for evaluating hashing strategies because per-file isolation benchmarks
can be misleading -- they miss thread contention and resource sharing effects that dominate
real-world performance (see mmap+rayon experiment below).

**Limitation**: Even the parallel benchmarks may not fully reproduce real-world behavior.
The mmap+rayon experiment showed only minor differences in criterion benchmarks (100-10K
files) but a 2.5x slowdown on a real 40K-file directory tree. Contributing factors that
benchmarks don't capture: mixed file sizes, cold cache, filesystem metadata pressure, and
deeper rayon task queue contention at higher file counts. The real-world `sumpig
fingerprint` test on an actual directory tree remains the definitive measurement for
hashing strategy decisions.

## Experiments tried

### blake3 mmap+rayon per-file parallel hashing (rejected)

**Date**: 2026-03-28

**Hypothesis**: For files above 1MB, using `blake3::Hasher::update_mmap_rayon()` would
hash individual large files faster by using memory-mapped IO and splitting the file across
rayon threads internally. The OS would handle paging, avoiding OOM risk.

**Per-file benchmark results (criterion, no contention)**:

| File size | Streaming | mmap+rayon | Speedup |
|-----------|----------|-----------|---------|
| 1KB       | 84 MiB/s | 82 MiB/s  | ~same   |
| 100KB     | 1.8 GiB/s| 1.7 GiB/s | ~same   |
| 1MB       | 2.2 GiB/s| 6.7 GiB/s | 3.0x    |
| 10MB      | 2.2 GiB/s| 11.9 GiB/s| 5.4x    |

Per-file benchmarks looked very promising for large files.

**Real-world results (entire directory tree)**:

| Version | Wall clock | User CPU | System CPU | CPU utilization |
|---------|-----------|----------|-----------|----------------|
| Streaming | 29.5s | 65s | 40s | 356% |
| mmap+rayon | 74.2s | 72s | 88s | 215% |

**Result**: 2.5x slower in practice. Rejected.

**Root cause**: Thread contention between two layers of rayon parallelism. The outer
`par_iter()` distributes files across rayon threads. Each file's `update_mmap_rayon()` also
tries to use rayon threads internally. This causes:

- Thread oversubscription: more work items than cores
- Doubled system CPU time from mmap page fault handling across competing threads
- Lower CPU utilization (215% vs 356%) because threads spend more time waiting

The per-file benchmarks were misleading because they ran one file at a time with no
contention for the rayon thread pool.

**Lesson**: Per-file parallelism and cross-file parallelism compete for the same thread
pool. When the workload is many files (the common case for directory fingerprinting), outer
parallelism wins. Per-file parallelism would only help if there were a single very large
file, which is rare in this use case.

A prior iteration also tried `blake3::Hasher::update_rayon()` (without mmap), which
required reading the entire file into memory first. This caused OOM on large files and was
removed in commit 0747b28.

### MetroHash128 as BLAKE3 replacement (rejected)

**Date**: 2026-03-28

**Hypothesis**: BLAKE3 is a cryptographic hash function running at ~2.2 GiB/s
single-threaded on Apple Silicon. MetroHash128 is a non-cryptographic 128-bit hash that
runs 3-4x faster on raw throughput benchmarks. For sumpig's use case (iCloud sync
verification, no adversarial threat model), cryptographic guarantees aren't needed.
Switching to metro could meaningfully reduce fingerprinting time.

**Method**: Swapped blake3 for metrohash128 throughout (file hashing, Merkle tree
construction, synthetic hashes). All tests passed. Ran real-world fingerprinting on a
~40K file directory tree.

**Real-world results**:

| Hash | Wall clock | User CPU | System CPU | CPU utilization |
|------|-----------|----------|-----------|----------------|
| BLAKE3 | 28.9s | 64.9s | 39.2s | 360% |
| MetroHash128 | 27.7s | 6.1s | 33.2s | 141% |

**Result**: Wall-clock time is identical within noise (~4% difference). Rejected as a
user-visible performance improvement.

**Analysis**: Metro uses 10x less CPU time (6s vs 65s user), confirming it is dramatically
faster at raw hashing. But the wall-clock time didn't improve because the workload is
I/O-bound -- system CPU (filesystem reads) dominates at ~33-39s regardless of hash
function. CPU utilization dropped from 360% to 141% because metro finishes hashing so
quickly that threads spend most of their time waiting on I/O.

**Side benefit**: Metro would reduce power consumption and thermal pressure on battery, but
this is not worth the tradeoff of losing cryptographic collision resistance. BLAKE3
provides stronger guarantees for data integrity verification with no user-visible
performance cost.

**Lesson**: For directory fingerprinting workloads on SSD, the hash function is not the
bottleneck. I/O (file open, read, close) dominates. Optimizing hash throughput yields
CPU savings but not wall-clock savings. This finding informed the decision to make
metadata fingerprinting the default mode -- it eliminates file reads entirely, achieving
a 5x speedup. Future performance work should focus on hash caching to bring content
verification closer to metadata-mode speed on repeated runs.
