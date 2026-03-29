# Install required toolchain components
setup:
    rustup component add clippy rustfmt

# Run all checks (test + lint + format)
check: test lint fmt-check

# Run all tests
test:
    cargo test

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Build release binary
build:
    cargo build --release

# Format code
fmt:
    cargo fmt

# Check formatting without modifying
fmt-check:
    cargo fmt -- --check

# Run benchmarks (available after Phase 3)
bench:
    cargo bench

# Run a specific test by name
test-one NAME:
    cargo test {{NAME}}

# Install release binary to ~/.cargo/bin (or CARGO_INSTALL_ROOT)
install:
    cargo install --path .

# Build and run with arbitrary arguments
run *ARGS:
    cargo run -- {{ARGS}}

# Bump version in Cargo.toml, commit, tag with release notes, and push.
# Usage: just bump 0.2.0 (or just bump for patch increment)
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
        git add Cargo.toml
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

# Delete a GitHub release and re-tag the current commit to re-trigger release workflows
retag tag:
    gh release delete {{tag}} --yes || true
    git push origin :refs/tags/{{tag}} || true
    git tag -f {{tag}}
    git push && git push --tags
