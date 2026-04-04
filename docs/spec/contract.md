# sumpig Contract

**Created**: 2026-03-28
**Confidence Score**: 95/100
**Status**: Draft
**Supersedes**: None

## Problem Statement

iCloud Drive sync between Macs is opaque and unreliable. Ted has encountered corrupted git repos on his MacBook Pro after iCloud sync, destroying trust in whether files actually match between his Mac Studio and travel laptop. There is no fast, trustworthy way to verify that two directory trees are identical or to pinpoint exactly what differs.

The cost of undetected sync failures is high: working on corrupted repos, losing changes, or not knowing which machine has the authoritative copy. This needs a tool that can be run periodically on both machines and produce a definitive answer.

## Goals

1. **Definitive verification**: if sumpig says two trees match, they match. No silent skips, no ambiguity, no "probably fine." Every file is either hashed, recorded as dataless, or recorded as an error.
2. **Pinpoint differences**: when trees differ, report exactly which files and directories differ, with enough structure (Merkle tree) to drill down efficiently.
3. **Fast enough to run habitually**: targeting ripgrep-class performance. Fingerprinting a 40K-file directory tree should take seconds, not minutes. Performance benchmarks with regression detection ensure it stays fast.
4. **Fully tested with fast feedback**: every module has red/green unit tests. Integration tests exercise the CLI end-to-end. All tests run in seconds, not minutes.
5. **Ergonomic CLI**: clear subcommands, good defaults, progress feedback, actionable output. Follows conventions of modern Rust CLI tools.

## Success Criteria

- [ ] `sumpig fingerprint <path>` recursively hashes all files and produces a deterministic manifest
- [ ] Running fingerprint twice on an unchanged directory produces byte-identical output
- [ ] Modifying one file changes the root hash and the relevant path of directory hashes
- [ ] `sumpig compare <file1> <file2>` reports identical trees (exit 0) or exact differences (exit 1)
- [ ] Compare uses Merkle property to skip matching subtrees (verifiable by output structure)
- [ ] Dataless files (iCloud-evicted, SF_DATALESS flag) recorded as `dataless:<size>` entries
- [ ] `--hydrate` flag on fingerprint forces download of dataless files before hashing
- [ ] Unreadable files (permission denied, I/O error) recorded as `error:<reason>` entries
- [ ] Compare flags dataless and error entries as warnings
- [ ] `--depth N` controls output granularity without affecting hash correctness (same root hash at any depth)
- [ ] `--no-ignore` overrides the default ignore list for full verification
- [ ] Default output writes to `<path>/.sumpig-fingerprints/<hostname>.txt`
- [ ] All unit tests pass and run in under 5 seconds total
- [ ] All integration tests pass and run in under 10 seconds total
- [ ] Criterion benchmarks establish baselines for hash throughput, walk speed, and tree construction
- [ ] CLAUDE.md captures project principles for consistent future development

## Scope Boundaries

### In Scope

- `fingerprint` subcommand: parallel directory walk, BLAKE3 hashing, Merkle tree construction, manifest output
- `compare` subcommand: two-manifest comparison with Merkle skip, structured diff report
- Configurable output depth (`--depth N`)
- Configurable ignore list (`--no-ignore` to disable defaults)
- Configurable output path (`--output FILE`)
- Configurable parallelism (`--jobs N`)
- Dataless file detection (macOS SF_DATALESS)
- Error recording for unreadable files
- Progress reporting to stderr
- CLAUDE.md with project principles
- justfile with recipes for all common project commands (check, test, lint, build, bench, fmt)
- Unit tests for every module
- Integration tests for both subcommands
- Criterion benchmarks for hash, walk, and merkle modules

### Out of Scope

- Daemon mode or watch mode -- periodic runs via cron/launchd are simpler and more trustworthy
- Network transfer of fingerprint files -- iCloud syncs the `.sumpig-fingerprints/` directory naturally
- GUI or TUI -- this is a CLI tool
- Cross-platform support beyond macOS -- primary use case is Mac-to-Mac; Linux/Windows can be added later if needed
- Custom ignore list configuration (beyond --no-ignore) -- the defaults cover the common cases; full customization adds complexity for little value in v1

### Future Considerations

- GitHub Actions CI pipeline (test, lint, bench on PR)
- `just bump` and `just retag` recipes for release tagging
- `sumpig watch` mode for continuous monitoring
- Custom ignore list via config file or `--skip` flag
- `sumpig compare --dir` to auto-detect fingerprint files in a directory
- Homebrew formula for distribution
- Shell completions (bash, zsh, fish)

## Execution Plan

### Dependency Graph

```
Phase 1: Core Library + Fingerprint Command
  ├── Phase 2: Compare Command (blocked by Phase 1)
  └── Phase 3: Benchmarks (blocked by Phase 1)
```

### Execution Steps

**Strategy**: Hybrid (Phase 1 sequential, then Phases 2 and 3 parallel)

1. **Phase 1 -- Core Library + Fingerprint Command** _(blocking)_
   ```
   /ideation:execute-spec docs/spec/spec-phase-1.md
   ```

2. **Phases 2 and 3 -- parallel after Phase 1**

   Run sequentially if preferred:
   ```
   /ideation:execute-spec docs/spec/spec-phase-2.md
   /ideation:execute-spec docs/spec/spec-phase-3.md
   ```

   Or in parallel via agent team (see prompt below).

### Agent Team Prompt

```
Phase 1 (Core Library + Fingerprint Command) is complete. Create an agent team to
implement 2 remaining phases in parallel. Each phase is independent.

Spawn 2 teammates with plan approval required. Each teammate should:
1. Read their assigned spec file
2. Read CLAUDE.md for project conventions
3. Plan their implementation approach and wait for approval
4. Implement following spec, running `just check` after each component
5. Run validation commands from their spec after implementation

Teammates:

1. "Compare Command" -- docs/spec/spec-phase-2.md
   Two-manifest comparison with Merkle skip optimization. Adds compare subcommand
   to CLI and integration tests.

2. "Benchmarks" -- docs/spec/spec-phase-3.md
   Criterion benchmarks for hash throughput, walk speed, and Merkle computation.
   Adds bench configuration to Cargo.toml.

Coordinate on shared files (src/main.rs, src/lib.rs, Cargo.toml) to avoid merge
conflicts -- only one teammate should modify a shared file at a time.
```
