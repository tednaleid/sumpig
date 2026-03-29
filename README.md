# sumpig

Merkle tree directory fingerprinting and comparison. Generates a content fingerprint
of a directory tree using BLAKE3 hashes, then compares fingerprints to find exactly
what differs between two copies of the same tree.

The primary use case is verifying iCloud Drive sync between two Macs, but sumpig works
for any scenario where two directory trees should be identical: backups, rsync copies,
deploy artifacts, etc.

## Install

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

This recursively hashes every file with BLAKE3, computes Merkle directory hashes,
and writes a manifest to `~/Documents/.sumpig-fingerprints/<hostname>.txt`.

Options:

- `--depth N` -- control output granularity (default: 6). Does not affect hashing depth.
- `--output FILE` -- write manifest to a specific path instead of the default.
- `--jobs N` -- number of worker threads (default: all cores).
- `--no-ignore` -- disable the default ignore list (node_modules, target, .venv, etc.).
- `--quiet` -- suppress progress bars and summary output.
- `--fast` -- use file metadata (size + modification time) instead of reading file contents.

### Fast mode

```
sumpig fingerprint --fast ~/Documents
```

Fast mode hashes file metadata instead of reading file contents. On a 40K-file directory
tree this runs in about 5 seconds vs 28 seconds for content mode -- roughly 5x faster.

The Merkle tree structure is identical in both modes: if two fast-mode root hashes match,
every file has the same size and modification time on both machines. This is a strong
signal that iCloud sync completed correctly, since iCloud preserves modification times.

Use fast mode for routine checks where you want a quick answer. Use content mode (the
default) when you need to verify actual file contents, such as after recovering from a
known sync failure or when checking integrity of git repositories where silent corruption
is a concern.

Manifests record their mode in the header (`# mode: fast` or `# mode: content`). The
compare command warns if you compare manifests from different modes, since their hashes
are not comparable.

### Compare two fingerprints

```
sumpig compare machine-a.txt machine-b.txt
```

Reports changed files, changed directories, entries only on one side, and warnings
for dataless or unreadable files. Uses the Merkle tree property to skip matching
subtrees, so comparison is fast even for large trees.

Exit codes: 0 = identical, 1 = differences found, 2 = usage error.

## How it works

sumpig builds a Merkle hash tree where each directory's hash covers all files beneath it.
If two root hashes match, the entire tree is identical. If they differ, sumpig walks
down the tree comparing only the branches that diverge, converging on the exact changed
files in O(changes * depth) comparisons instead of O(total files).

## License

MIT
