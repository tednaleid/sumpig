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

- `hash.rs` -- BLAKE3 file hashing, dataless detection, error recording
- `walk.rs` -- parallel directory walking with configurable skip list
- `merkle.rs` -- streaming Merkle tree computation (no in-memory tree)
- `manifest.rs` -- manifest file writing and parsing
- `compare.rs` -- two-manifest comparison with Merkle skip optimization (Phase 2)
- `main.rs` -- CLI entry point, clap subcommand routing

## Development

All commands go through `just`:

```
just setup    # install required toolchain components (clippy, rustfmt)
just check    # run tests + lint
just test     # run all tests
just lint     # run clippy
just build    # build release binary
just bench    # run benchmarks
just fmt      # format code
```

## Testing conventions

- Unit tests live in `#[cfg(test)] mod tests` within each module.
- Integration tests use `tempfile::TempDir` for fixtures and `assert_cmd` for CLI invocation.
- Tests must not depend on filesystem state outside their temp directory.

## Reference

- Contract and phase specs: `docs/spec/`
