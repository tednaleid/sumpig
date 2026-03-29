# Merkle Trees: How sumpig verifies directory trees

## What is a Merkle tree?

A Merkle tree is a data structure where every node stores a hash that covers all of its
descendants. Changing any leaf causes a cascade of hash changes all the way up to the root.

The simplest example: imagine a directory with two files. The directory's hash is computed
from its children's hashes. If you change one file, its hash changes, and the directory
hash changes too. If the directory hashes match on two machines, both files must be
identical -- you verified two files with one comparison.

Git uses this exact structure. A Git tree object is a Merkle hash of its contents. When
you compare two commits, Git can skip entire subtrees whose hashes match and zoom in on
exactly what changed.

## How sumpig builds a Merkle tree

sumpig builds its tree bottom-up:

1. **Hash every file.** Each file's content is hashed with BLAKE3 to produce a 32-byte
   digest.

2. **Compute directory hashes.** For each directory, sort its children by name, then
   concatenate `child_name + null_byte + child_hash` for each child. Hash that
   concatenation with BLAKE3. This produces the directory's Merkle hash.

3. **Repeat up to the root.** Each directory hash feeds into its parent's computation.
   The root directory's hash is a single 32-byte fingerprint of the entire tree.

The sort-by-name step matters. Without it, the same set of files could produce different
hashes depending on filesystem enumeration order. The null byte separator prevents
ambiguity between a file named "ab" in directory "c" and a file named "b" in directory
"ca".

## Why this makes comparison fast

Comparing two directory trees naively requires checking every file in both trees. For a
tree with a million files, that means a million comparisons.

Merkle trees let you skip work:

1. Compare the root hashes. If they match, the entire trees are identical. Done.
2. If the roots differ, compare the root's children. Most children will match.
3. Recurse only into children whose hashes differ.
4. Eventually you reach the individual files that changed.

If 3 files differ in a million-file tree, you might compare 50 hashes instead of a
million. The work is proportional to the number of differences times the depth of the
tree, not the total number of files.

## Depth and output granularity

sumpig always hashes every file and computes every directory hash, regardless of the
`--depth` flag. Depth only controls how much detail appears in the output manifest.

With `--depth 1`, you see the root and its immediate children. Each child directory's
hash still covers everything beneath it. If two manifests match at depth 1, the trees are
identical.

With `--depth 6`, you see entries down to six levels deep. This gives you more
granularity when trees differ -- you can see which subdirectory contains the mismatch
without re-running the tool.

The root hash is the same regardless of depth. Depth is a display choice, not a
correctness tradeoff.

## Special entries

Not every file can be hashed normally. sumpig handles three special cases:

- **Dataless files** (macOS iCloud-evicted): The file exists but its data is in the cloud.
  sumpig records it as `dataless:<file_size>` and includes a synthetic hash in the Merkle
  tree so the root hash still accounts for it.

- **Error files** (permission denied, I/O failure): sumpig records them as
  `error:<reason>` with a synthetic hash. No file is silently skipped.

- **Symlinks**: Recorded as `symlink:<target_path>`. The link target is hashed, not the
  file it points to.

These synthetic hashes ensure that the Merkle tree is always complete. If a file changes
from dataless to present (or vice versa), the root hash changes.
