# sumpig

<p align="center">
  <img src="docs/img/sumpig.png" alt="sumpig logo" width="400">
</p>

Merkle tree directory fingerprinting and comparison. Generates a content fingerprint
of a directory tree using BLAKE3 hashes, then compares fingerprints to find exactly
what differs between two copies of the same tree.

The primary use case is verifying iCloud Drive sync between two Macs, but sumpig works
for any scenario where two directory trees should be identical: backups, rsync copies,
deploy artifacts, etc.

## Install

### Homebrew

```
brew install tednaleid/sumpig/sumpig
```

### From source

Requires Rust 1.85+ (edition 2024).

```
git clone https://github.com/tednaleid/sumpig.git
cd sumpig
cargo install --path .
```

### From GitHub releases

Download the binary for your platform from the
[releases page](https://github.com/tednaleid/sumpig/releases),
extract it, and put it somewhere on your PATH.

## Usage

### Fingerprint a directory

```
sumpig fingerprint ~/Documents
```

This fingerprints every file's metadata (size and modification time), computes Merkle
directory hashes, and writes a manifest to `~/Documents/.sumpig-fingerprints/<hostname>.txt`.

Options:

- `--verify-contents`, `-C` -- hash file contents instead of metadata (slower, detects silent corruption).
- `--depth N` -- control output granularity (default: 6). Does not affect hashing depth.
- `--output FILE` -- write manifest to a specific path instead of the default.
- `--jobs N` -- number of worker threads (default: all cores).
- `--no-ignore` -- disable the default ignore list (node_modules, target, .venv, etc.).
- `--quiet` -- suppress progress bars and summary output.
- `--tag [NAME]` -- tag the output file with a name, or a timestamp if no name is given.

### Tracking changes over time

By default, sumpig overwrites the fingerprint file on each run. Use `--tag` to keep
a history of fingerprints and compare them to see what changed:

```
sumpig fingerprint --tag ~/Documents              # creates <hostname>-2026-03-29T15-30-00Z.txt
sumpig fingerprint --tag before-upgrade ~/Documents  # creates <hostname>-before-upgrade.txt
```

Then compare any two tagged fingerprints:

```
sumpig compare .sumpig-fingerprints/cardinal-before-upgrade.txt \
               .sumpig-fingerprints/cardinal-after-upgrade.txt
```

Without `--tag`, the default `<hostname>.txt` filename is used, which overwrites on each
run. This is useful when comparing the same directory across two machines (each machine
writes its own hostname file into the same iCloud-synced directory).

### Metadata vs content verification

By default, sumpig fingerprints file metadata (size and modification time) without reading
file contents. This is fast (~5 seconds on a 40K-file tree) and sufficient for routine
iCloud sync checks, since iCloud preserves modification times.

For deeper verification, use `--verify-contents` (or `-C`) to read and hash every file
with BLAKE3. This is ~5x slower (~28 seconds on the same tree) but detects silent
corruption that preserves file size and timestamps.

```
sumpig fingerprint ~/Documents                    # metadata (default, fast)
sumpig fingerprint --verify-contents ~/Documents  # content hashing (thorough)
sumpig fingerprint -C ~/Documents                 # same, short form
```

Use content verification after recovering from a known sync failure, or when checking
integrity of git repositories where silent corruption is a concern.

Manifests record their mode in the header (`# mode: fast` or `# mode: content`). The
compare command warns if you compare manifests from different modes, since their hashes
are not comparable.

### Compare two fingerprints

```
sumpig compare machine-a.txt machine-b.txt
```

Output uses tab-separated prefix and path on stdout, one entry per line:

```
!	./path/to/changed-file.txt
<	./path/only-in-first.txt
>	./path/only-in-second.txt
```

Directories appear when they are at the manifest depth boundary (where individual
files are not listed). This is designed for piping -- summary and warnings go to
stderr, so `sumpig compare a.txt b.txt | cut -f2` gives you just the paths, and
`sumpig compare a.txt b.txt | grep "^<"` gives you just the missing files.

Uses the Merkle tree property to skip matching subtrees, so comparison is fast
even for large trees.

Exit codes: 0 = identical, 1 = differences found, 2 = usage error.

## How it works

sumpig builds a Merkle hash tree where each directory's hash covers all files beneath it.
If two root hashes match, the entire tree is identical. If they differ, sumpig walks
down the tree comparing only the branches that diverge, converging on the exact changed
files in O(changes * depth) comparisons instead of O(total files).

## License

MIT
