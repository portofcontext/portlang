#!/bin/bash
set -euo pipefail

# Release script for portlang CLI
# This script triggers a GitHub Actions workflow to build and publish a new release

VERSION="${1:-}"

if [ -z "$VERSION" ]; then
    echo "Usage: ./release.sh <version>"
    echo "Example: ./release.sh v0.1.0"
    echo ""
    echo "The version should match the version in Cargo.toml (with or without 'v' prefix)"
    exit 1
fi

# Normalize version (add 'v' prefix if not present)
if [[ ! "$VERSION" =~ ^v ]]; then
    VERSION="v${VERSION}"
fi

echo "🚀 Triggering release workflow for version: $VERSION"
echo ""
echo "This will:"
echo "  1. Build binary for Apple Silicon Mac"
echo "  2. Create a GitHub Release with artifacts"
echo "  3. Publish to Homebrew tap at portofcontext/homebrew-tap"
echo ""

read -p "Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Cancelled."
    exit 1
fi

# Trigger the GitHub Actions workflow using workflow_dispatch
gh workflow run release.yml --ref main -f tag="$VERSION"

echo ""
echo "✅ Release workflow triggered!"
echo ""
echo "Monitor the progress at:"
echo "  https://github.com/portofcontext/portlang/actions/workflows/release.yml"
echo ""
echo "Once complete, the release will be available at:"
echo "  https://github.com/portofcontext/portlang/releases/tag/$VERSION"
echo ""
echo "The vscode-build workflow will run automatically after the Release workflow finishes."
echo "  https://github.com/portofcontext/portlang/actions/workflows/vscode-build.yml"
