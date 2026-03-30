# Release Pipeline and Homebrew Tap Setup

Reference guide for setting up automated releases with Claude-generated release notes
and Homebrew tap auto-updates for a Rust CLI project. Based on the patterns used in
sumpig and montty.

## Overview

The release pipeline has three parts:

1. **`just bump`** -- version bump, Claude release notes, annotated tag, push
2. **GitHub Actions release workflow** -- multi-platform build, GitHub Release, Homebrew update
3. **Homebrew tap repo** -- separate repo that holds the Formula, auto-updated by CI

```
Developer runs: just bump 0.3.0
  -> Updates Cargo.toml, commits
  -> Extracts commits since last tag
  -> Calls claude -p to generate release notes (falls back to git log)
  -> Creates annotated tag: git tag -a "v0.3.0" -F <notes_file>
  -> Pushes commits and tags

GitHub Actions triggers on tag push
  -> Builds release binaries for each platform
  -> Uploads to GitHub Release with release notes from tag
  -> Computes SHA-256 for each artifact
  -> Clones homebrew-<project> tap repo
  -> Generates Formula with version + SHA-256 values
  -> Commits and pushes to tap repo

Users install with: brew install <owner>/<project>/<project>
```

---

## justfile recipes

### bump

Takes a bare version number. Auto-increments patch if no version given.

```just
bump version="":
    #!/usr/bin/env bash
    set -euo pipefail
    current=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
    if [ -z "{{version}}" ]; then
        IFS='.' read -r major minor patch <<< "$current"
        new="$major.$minor.$((patch + 1))"
    else
        new="{{version}}"
    fi
    echo "Bumping $current -> $new"
    if [ "$current" != "$new" ]; then
        sed -i '' "s/^version = \"$current\"/version = \"$new\"/" Cargo.toml
        cargo generate-lockfile --quiet
        git add Cargo.toml Cargo.lock
        git commit -m "Bump version to $new"
    fi
    # Generate release notes from commits since last tag
    prev_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "")
    if [ -n "$prev_tag" ]; then
        log=$(git log "$prev_tag"..HEAD --oneline --no-merges)
    else
        log=$(git log --oneline --no-merges)
    fi
    notes_file=$(mktemp)
    if command -v claude >/dev/null 2>&1; then
        claude -p "Generate concise release notes for version $new. Commits:\n$log\n\nGuidelines: group related commits, focus on user-facing changes, skip version bumps and CI changes, one line per bullet, past tense, output only a bullet list." > "$notes_file" 2>/dev/null || echo "$log" | sed 's/^[0-9a-f]* /- /' > "$notes_file"
    else
        echo "$log" | sed 's/^[0-9a-f]* /- /' > "$notes_file"
    fi
    echo "Release notes:"
    cat "$notes_file"
    git tag -a "v$new" -F "$notes_file"
    rm -f "$notes_file"
    git push && git push --tags
    echo "v$new released!"
```

Key points:
- Uses `claude -p` with a structured prompt for release notes
- Falls back to formatted commit log if Claude is unavailable or fails
- Creates an **annotated tag** with `-a` and `-F` (not a lightweight tag)
- The tag message becomes the release notes on GitHub

### retag

Re-triggers a failed release without losing the release notes.

```just
retag version:
    #!/usr/bin/env bash
    set -euo pipefail
    # Save existing tag annotation before deleting
    notes=$(git tag -l --format='%(contents)' "v{{version}}" 2>/dev/null || echo "v{{version}}")
    notes_file=$(mktemp)
    trap 'rm -f "$notes_file"' EXIT
    echo "$notes" > "$notes_file"
    gh release delete "v{{version}}" --yes || true
    git push origin ":refs/tags/v{{version}}" || true
    git tag -d "v{{version}}" || true
    git tag -a "v{{version}}" -F "$notes_file"
    git push && git push --tags
```

**Gotcha: annotated vs lightweight tags.** A naive `git tag -f` creates a lightweight
tag, which has no message body. This loses the release notes. You must:
1. Extract the annotation with `git tag -l --format='%(contents)'`
2. Delete the old tag
3. Recreate with `git tag -a ... -F <file>` to preserve the annotation

---

## GitHub Actions release workflow

### Tag fetching gotchas

Annotated tag objects are NOT reliably available in CI even when the workflow is
triggered by a tag push. You need all three of these:

```yaml
- uses: actions/checkout@v5
  with:
    fetch-depth: 0      # full history
    fetch-tags: true     # fetch tag objects
```

AND an explicit fetch before using the tag:

```yaml
- name: Ensure annotated tag is available
  run: git fetch origin "refs/tags/${{ github.ref_name }}:refs/tags/${{ github.ref_name }}" --force
```

Without this, `--notes-from-tag` in `gh release create` may produce empty notes.
This was discovered the hard way across multiple commits -- the combination of
`fetch-depth: 0` + `fetch-tags: true` is necessary but not always sufficient,
especially when submodules are involved.

### Release notes extraction

Two approaches that work:

**Option A: `gh release create --notes-from-tag`** (simpler)
```yaml
- name: Create GitHub Release
  env:
    GH_TOKEN: ${{ github.token }}
  run: |
    gh release create ${{ github.ref_name }} \
      --title "${{ github.ref_name }}" \
      --notes-from-tag \
      artifacts-*.tar.gz
```

**Option B: Manual extraction** (more control)
```yaml
- name: Extract release notes from tag
  run: |
    git cat-file tag "refs/tags/$GITHUB_REF_NAME" | sed '1,/^$/d' > /tmp/release-notes.txt

- name: Upload to GitHub Release
  uses: softprops/action-gh-release@v2
  with:
    body_path: /tmp/release-notes.txt
    files: artifacts-*.tar.gz
```

The `sed '1,/^$/d'` strips the tag metadata header (tagger, date, etc.), leaving only
the annotation body.

### Multi-platform Rust builds

Build matrix for common targets:

```yaml
jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-apple-darwin
            os: macos-26
          - target: aarch64-apple-darwin
            os: macos-26
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
      - run: rustup target add ${{ matrix.target }}
      - run: cargo build --release --target ${{ matrix.target }}
      - name: Package binary
        run: |
          cd target/${{ matrix.target }}/release
          tar czf ../../../<project>-${{ matrix.target }}.tar.gz <binary>
      - uses: actions/upload-artifact@v5
        with:
          name: <project>-${{ matrix.target }}
          path: <project>-${{ matrix.target }}.tar.gz
```

---

## Homebrew tap setup

### Formula vs Cask

- **Formula** -- for CLI tools distributed as binaries or built from source
- **Cask** -- for macOS GUI apps distributed as `.dmg` or `.pkg`

For a Rust CLI with pre-built binaries, use a Formula with platform-specific blocks.

### Multi-platform Formula template

```ruby
class MyTool < Formula
  desc "Tool description"
  homepage "https://github.com/owner/project"
  version "VERSION"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/owner/project/releases/download/vVERSION/project-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_MACOS_ARM"
    end
    on_intel do
      url "https://github.com/owner/project/releases/download/vVERSION/project-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_MACOS_INTEL"
    end
  end
  on_linux do
    on_intel do
      url "https://github.com/owner/project/releases/download/vVERSION/project-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_LINUX"
    end
  end

  def install
    bin.install "binary_name"
  end

  test do
    system "#{bin}/binary_name", "--version"
  end
end
```

Note: `#{version}` in the url is a Ruby interpolation that Homebrew expands at install
time. When generating the formula in CI, use literal version strings in the `version`
and `sha256` fields, but keep `#{version}` in the urls so Homebrew resolves them.

### One-time setup

1. Create a `scripts/setup-homebrew-tap.sh` that:
   - Creates `owner/homebrew-project` repo via `gh repo create`
   - Fetches latest release, downloads each platform artifact
   - Computes SHA-256 for each
   - Generates the initial `Formula/project.rb`
   - Creates a README with install instructions
   - Commits and pushes

2. Create a fine-grained Personal Access Token:
   - Go to: https://github.com/settings/personal-access-tokens/new
   - Scope: Only the `owner/homebrew-project` repo
   - Permissions: Contents -> Read and write
   - Store as `HOMEBREW_TAP_TOKEN` secret on the main project repo:
     `gh secret set HOMEBREW_TAP_TOKEN --repo owner/project`

### Auto-update from CI

In the release workflow's publish job, after uploading artifacts:

```yaml
- name: Compute artifact SHA-256 values
  if: env.HOMEBREW_TAP_TOKEN != ''
  id: sha256
  run: |
    echo "macos_arm=$(shasum -a 256 project-aarch64-apple-darwin.tar.gz | awk '{print $1}')" >> "$GITHUB_OUTPUT"
    echo "macos_intel=$(shasum -a 256 project-x86_64-apple-darwin.tar.gz | awk '{print $1}')" >> "$GITHUB_OUTPUT"
    echo "linux=$(shasum -a 256 project-x86_64-unknown-linux-gnu.tar.gz | awk '{print $1}')" >> "$GITHUB_OUTPUT"

- name: Update Homebrew formula
  if: env.HOMEBREW_TAP_TOKEN != ''
  run: |
    VERSION="${GITHUB_REF_NAME#v}"
    # ... clone tap repo, generate formula with SHA-256 values, commit, push
```

The `if: env.HOMEBREW_TAP_TOKEN != ''` guard means the workflow works without Homebrew
configured -- it just skips the formula update step.

---

## Common pitfalls

1. **Lightweight vs annotated tags.** `git tag -f` and `git tag <name>` create lightweight
   tags (no message). Always use `git tag -a <name> -F <file>` or `git tag -a <name> -m "msg"`.

2. **Tag objects not available in CI.** Even with `fetch-depth: 0`, annotated tag objects
   may not be fetched. Use `fetch-tags: true` AND explicit `git fetch origin refs/tags/...`.

3. **`retag` losing release notes.** Must extract annotation before deleting the tag.
   Use `git tag -l --format='%(contents)'` to get the message body.

4. **Formula indentation.** When generating a formula with heredoc in a YAML workflow,
   the heredoc content gets the shell indentation. This doesn't affect Ruby parsing but
   looks messy. Use `sed` or generate without leading whitespace.

5. **Version prefix mismatch.** If tags use `v` prefix (e.g., `v0.2.1`), the Formula
   version field should be bare (`0.2.1`). Strip with `${GITHUB_REF_NAME#v}`. The url
   should include the `v` since that's what the GitHub Release download URL uses.

6. **SHA-256 computation platform.** `shasum -a 256` works on macOS and Linux. On some
   Linux runners you may need `sha256sum` instead. The `shasum` command is more portable.
