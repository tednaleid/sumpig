#!/usr/bin/env bash
# ABOUTME: Creates the tednaleid/homebrew-sumpig tap repo on GitHub and seeds it
# ABOUTME: with an initial formula from the latest sumpig release.
set -euo pipefail

OWNER="tednaleid"
TAP_REPO="homebrew-sumpig"
MAIN_REPO="sumpig"

# -- Preflight checks --

if ! command -v gh &>/dev/null; then
    echo "Error: gh CLI is required. Install with: brew install gh"
    exit 1
fi

if ! gh auth status &>/dev/null; then
    echo "Error: not authenticated with gh. Run: gh auth login"
    exit 1
fi

# -- Get latest release version and compute SHA-256 for each platform --

echo "Fetching latest release info..."
VERSION=$(gh release view --repo "${OWNER}/${MAIN_REPO}" --json tagName -q .tagName)
BARE_VERSION="${VERSION#v}"

TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin" "x86_64-unknown-linux-gnu")
declare -A SHA256S

for target in "${TARGETS[@]}"; do
    url="https://github.com/${OWNER}/${MAIN_REPO}/releases/download/${VERSION}/sumpig-${target}.tar.gz"
    echo "Downloading sumpig-${target}.tar.gz to compute SHA-256..."
    sha=$(curl -sL "$url" | shasum -a 256 | awk '{print $1}')
    SHA256S[$target]="$sha"
    echo "  ${target}: ${sha}"
done

# -- Create the tap repo --

if gh repo view "${OWNER}/${TAP_REPO}" &>/dev/null; then
    echo "Repo ${OWNER}/${TAP_REPO} already exists, skipping creation."
else
    echo "Creating ${OWNER}/${TAP_REPO}..."
    gh repo create "${OWNER}/${TAP_REPO}" --public \
        --description "Homebrew tap for sumpig, a Merkle tree directory fingerprinting tool"
fi

# -- Clone, populate, and push --

WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

gh repo clone "${OWNER}/${TAP_REPO}" "$WORKDIR"
cd "$WORKDIR"

mkdir -p Formula

cat > Formula/sumpig.rb << FORMULA
class Sumpig < Formula
  desc "Merkle tree directory fingerprinting and comparison"
  homepage "https://github.com/${OWNER}/${MAIN_REPO}"
  version "${BARE_VERSION}"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/${OWNER}/${MAIN_REPO}/releases/download/v#{version}/sumpig-aarch64-apple-darwin.tar.gz"
      sha256 "${SHA256S[aarch64-apple-darwin]}"
    end
    on_intel do
      url "https://github.com/${OWNER}/${MAIN_REPO}/releases/download/v#{version}/sumpig-x86_64-apple-darwin.tar.gz"
      sha256 "${SHA256S[x86_64-apple-darwin]}"
    end
  end
  on_linux do
    on_intel do
      url "https://github.com/${OWNER}/${MAIN_REPO}/releases/download/v#{version}/sumpig-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "${SHA256S[x86_64-unknown-linux-gnu]}"
    end
  end

  def install
    bin.install "sumpig"
  end

  test do
    system "#{bin}/sumpig", "--version"
  end
end
FORMULA

cat > README.md << 'README'
# homebrew-sumpig

Homebrew tap for [sumpig](https://github.com/tednaleid/sumpig), a Merkle tree directory fingerprinting and comparison tool.

## Install

```bash
brew install tednaleid/sumpig/sumpig
```

Or:

```bash
brew tap tednaleid/sumpig
brew install sumpig
```

## Update

```bash
brew upgrade sumpig
```
README

git add Formula/sumpig.rb README.md
git commit -m "Initial formula for sumpig ${VERSION}"
git push

echo ""
echo "Tap repo created and populated at: https://github.com/${OWNER}/${TAP_REPO}"
echo ""
echo "-- Next step: create a fine-grained Personal Access Token --"
echo ""
echo "1. Go to: https://github.com/settings/personal-access-tokens/new"
echo "2. Token name: sumpig-homebrew-tap"
echo "3. Repository access: Only select repositories -> ${OWNER}/${TAP_REPO}"
echo "4. Permissions: Contents -> Read and write"
echo "5. Generate the token and copy it"
echo ""
echo "Then set it as a secret on the sumpig repo:"
echo ""
echo "  gh secret set HOMEBREW_TAP_TOKEN --repo ${OWNER}/${MAIN_REPO}"
echo ""
echo "(Paste the token when prompted.)"
