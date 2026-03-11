# Portlang Release Guide

This document describes how to release new versions of the portlang CLI using cargo-dist.

## Overview

The portlang CLI uses [cargo-dist](https://github.com/axodotdev/cargo-dist) to automate building and releasing binaries. The release process:

1. Builds Mac binaries (Intel and Apple Silicon)
2. Creates a GitHub Release with downloadable artifacts
3. Publishes to Homebrew tap for easy installation

## Prerequisites

Before you can release, ensure you have:

### 1. GitHub CLI installed
```bash
brew install gh
gh auth login
```

### 2. Homebrew tap repository
✅ **Already exists!** You're using the shared `portofcontext/homebrew-tap` repository.

This tap already hosts the pctx formula and will also host portlang:
- `Formula/pctx.rb` (existing)
- `Formula/portlang.rb` (will be created automatically)

Multiple formulas in one tap is standard practice. No action needed.

### 3. GitHub Personal Access Token
Create a Personal Access Token (classic) with `repo` scope:

1. Go to https://github.com/settings/tokens
2. Click "Generate new token (classic)"
3. Name it: `HOMEBREW_TAP_TOKEN`
4. Select scopes:
   - ✅ `repo` (all)
   - ✅ `workflow`
5. Generate and copy the token

### 4. Add GitHub Secret
Add the token to your repository secrets:

```bash
# Using GitHub CLI
gh secret set HOMEBREW_TAP_TOKEN --repo portofcontext/portlang

# Or manually at:
# https://github.com/portofcontext/portlang/settings/secrets/actions
```

## Release Process

### Step 1: Update Version

Update the version in `Cargo.toml`:

```toml
[workspace.package]
version = "0.1.0"  # Change this to your new version
```

Commit the version change:
```bash
git add Cargo.toml
git commit -m "Bump version to 0.1.0"
git push
```

### Step 2: Trigger Release

Use the included release script:

```bash
./release.sh v0.1.0
```

Or manually trigger the workflow:

```bash
gh workflow run release.yml --ref main -f tag="v0.1.0"
```

The version format should match what's in `Cargo.toml`. Both `v0.1.0` and `0.1.0` are accepted.

### Step 3: Monitor Release

Watch the release workflow progress:

```bash
gh run watch
```

Or visit: https://github.com/portofcontext/portlang/actions/workflows/release.yml

The workflow will:
1. **Plan** - Determine what needs to be built
2. **Build Local Artifacts** - Build Mac binaries (Intel + Apple Silicon)
3. **Build Global Artifacts** - Generate checksums and metadata
4. **Host** - Upload artifacts to GitHub
5. **Publish Homebrew Formula** - Push formula to homebrew-tap
6. **Announce** - Create GitHub Release

### Step 4: Verify Release

Once complete, verify the release:

1. Check GitHub Release: `https://github.com/portofcontext/portlang/releases`
2. Check Homebrew formula: `https://github.com/portofcontext/homebrew-tap`
3. Test installation:
   ```bash
   brew tap portofcontext/tap
   brew install portlang
   portlang --version
   ```

## Testing Before Release

You can do a dry run without publishing:

```bash
gh workflow run release.yml --ref main -f tag="dry-run"
```

This will build everything but won't create a GitHub Release or publish to Homebrew.

## Configuration Files

The release setup consists of:

### `dist-workspace.toml`
Main configuration for cargo-dist:
- **targets**: Mac only (`aarch64-apple-darwin`, `x86_64-apple-darwin`)
- **installers**: Homebrew only
- **dispatch-releases**: Use manual workflow dispatch instead of git tags
- **tap**: Points to `portofcontext/homebrew-tap`

### `.github/workflows/release.yml`
Auto-generated GitHub Actions workflow. **Do not edit manually!**

If you need to change the workflow, update `dist-workspace.toml` and run:
```bash
dist generate
```

### `crates/portlang-cli/Cargo.toml`
Contains package metadata for Homebrew:
- **description**: Shown in `brew info`
- **homepage**: Package homepage
- **repository**: Source code location

## Troubleshooting

### Release workflow fails with "401 Unauthorized" on Homebrew publish

The `HOMEBREW_TAP_TOKEN` secret is missing or invalid. Follow Prerequisites step 3-4.

### Version mismatch error

Ensure the version in `Cargo.toml` matches the tag you're releasing. If you want to release `v0.1.0`, the `Cargo.toml` should have `version = "0.1.0"`.

### Homebrew tap repository not found

Create the repository `portofcontext/homebrew-tap` as described in Prerequisites step 2.

### Can't find binaries after release

Check that both Mac targets built successfully in the workflow logs. The artifacts should include:
- `portlang-aarch64-apple-darwin.tar.gz` (Apple Silicon)
- `portlang-x86_64-apple-darwin.tar.gz` (Intel)
- `portlang.rb` (Homebrew formula)

## Advanced Usage

### Releasing a specific package in a workspace

If you have multiple packages to release:
```bash
./release.sh portlang-cli-v0.1.0
```

### Creating prereleases

Use a prerelease suffix:
```bash
./release.sh v0.1.0-beta.1
```

This will mark the GitHub Release as a prerelease.

### Updating cargo-dist

To update to a new version of cargo-dist:
```bash
dist update
dist generate
```

This will update `cargo-dist-version` in `dist-workspace.toml` and regenerate the workflow.

## Files Generated by cargo-dist

These files are auto-generated and should not be edited manually:
- `.github/workflows/release.yml` - GitHub Actions workflow
- Build artifacts in `target/distrib/` (local only, not committed)

## Manual Release Checklist

If you prefer a checklist approach:

- [ ] Update version in `Cargo.toml`
- [ ] Commit and push version bump
- [ ] Run `./release.sh vX.Y.Z`
- [ ] Monitor workflow in GitHub Actions
- [ ] Verify GitHub Release created
- [ ] Check Homebrew tap updated
- [ ] Test installation with `brew install portofcontext/tap/portlang`
- [ ] Announce release (optional)

## Support

For issues with cargo-dist: https://github.com/axodotdev/cargo-dist/issues
For portlang issues: https://github.com/portofcontext/portlang/issues
