# sumpig

Merkle tree directory fingerprinting and comparison tool. Verifies that two directory trees
are identical by computing and comparing BLAKE3 hash trees.

## Principles

- Red/green TDD: write failing tests first, then implement to make them pass. This is non-negotiable.
- Correctness over speed: never silently skip files. Every file is hashed, recorded as dataless,
  or recorded as an error.
- Deterministic output: same input directory always produces the same manifest, byte for byte.
- Target ripgrep-class performance: parallel walking (jwalk), parallel hashing (rayon), BLAKE3 SIMD.
- Test everything: unit tests in every module, integration tests for CLI. All tests run in seconds.
- Lean dependencies: don't add a crate for something achievable in 30 lines of code.

## Architecture

Single crate with library + binary targets. Modules:

- `hash.rs` -- BLAKE3 file hashing, metadata hashing (--fast), dataless detection, error recording
- `walk.rs` -- parallel directory walking with configurable skip list
- `merkle.rs` -- streaming Merkle tree computation (no in-memory tree)
- `manifest.rs` -- manifest file writing and parsing
- `compare.rs` -- two-manifest comparison with Merkle skip optimization (Phase 2)
- `main.rs` -- CLI entry point, clap subcommand routing

## Development

All build, test, lint, format, and bench commands MUST go through the justfile. Never run
cargo directly. If a recipe doesn't exist for what you need, add one to the justfile.

```
just setup    # install required toolchain components (clippy, rustfmt)
just check    # run all tests, linting, and format check (used by CI)
just test     # run all tests
just test-one NAME  # run a specific test by name
just lint     # run clippy
just build    # build release binary
just bench    # run benchmarks (accepts args, e.g. just bench --bench hash_bench)
just fmt      # format code
just clean    # remove build artifacts (use this, never bare rm -rf)
just install  # install release binary to ~/.cargo/bin
just bump     # bump version, generate release notes, tag, and push
just retag    # re-trigger release workflow for an existing version
just install-hooks  # install pre-commit hook that runs just check
```

## Testing conventions

- Unit tests live in `#[cfg(test)] mod tests` within each module.
- Integration tests use `tempfile::TempDir` for fixtures and `assert_cmd` for CLI invocation.
- Tests must not depend on filesystem state outside their temp directory.

## Reference

- Contract and phase specs: `docs/spec/`
