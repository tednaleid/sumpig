# Implementation Spec: sumpig - CI Pipeline + Releases

**Contract**: ./contract.md
**Estimated Effort**: S

## Technical Approach

Three GitHub Actions workflows covering the development lifecycle:

1. **CI** (ci.yml): test, lint, and format check on every push to main and every PR.
   Runs on both ubuntu and macOS to catch platform-specific issues.

2. **Release** (release.yml): triggered by pushing a version tag (v*). Builds release
   binaries for macOS (arm64, x86_64) and Linux (x86_64), creates a GitHub release
   with the binaries attached.

3. **Benchmarks** (bench.yml): triggered after a release is published. Runs criterion
   benchmarks on a consistent environment and uploads the HTML report as an artifact.

Tag management is handled by justfile recipes (`just bump` and `just retag`) adapted
from the limn project's patterns.

## File Changes

### New Files

| File Path | Purpose |
|---|---|
| `LICENSE` | MIT license |
| `.github/workflows/ci.yml` | Test + lint + fmt-check on push/PR |
| `.github/workflows/release.yml` | Build release binaries on tag, create GitHub release |
| `.github/workflows/bench.yml` | Run benchmarks post-release, upload criterion report |

### Modified Files

| File Path | Changes |
|---|---|
| `Cargo.toml` | Add license, description, repository fields |
| `justfile` | Add bump and retag recipes |

## Workflow Details

### CI (ci.yml)

- **Trigger**: push to main, pull requests
- **Matrix**: ubuntu-latest, macos-latest
- **Steps**: checkout, install Rust (dtolnay/rust-toolchain), cargo test, cargo clippy, cargo fmt --check

### Release (release.yml)

- **Trigger**: tag push matching `v*`
- **Matrix**: x86_64-apple-darwin (macos-13), aarch64-apple-darwin (macos-14), x86_64-unknown-linux-gnu (ubuntu-latest)
- **Steps**: checkout, install Rust with target, cargo build --release --target, tar/gzip binary, upload artifact
- **Publish job**: downloads all artifacts, creates GitHub release with `gh release create`, attaches binaries
- **Binary naming**: `sumpig-<target>.tar.gz`

### Benchmarks (bench.yml)

- **Trigger**: release published
- **Runs on**: ubuntu-latest (consistent environment for comparable results)
- **Steps**: checkout, install Rust, cargo bench, upload `target/criterion/` as artifact
- **Retention**: 90 days

### just bump

Takes an optional version argument. Defaults to patch increment of current Cargo.toml version.
Updates Cargo.toml, commits, generates release notes (uses `claude -p` if available, falls back
to commit log), creates annotated tag with release notes, pushes commit and tag.

### just retag

Deletes the GitHub release and remote tag, force-creates the tag on current commit, pushes.
Used to re-trigger release workflows after fixing an issue.

## Validation

- `just check` passes locally
- Push triggers CI workflow on GitHub
- `just bump 0.1.0` creates tag, triggers release + bench workflows
- GitHub releases page shows binary downloads for all 3 targets
- Actions artifacts tab shows criterion benchmark report
