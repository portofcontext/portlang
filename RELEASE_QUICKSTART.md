# Portlang Release Quick Start

## One-Time Setup

### 1. Homebrew Tap (Already Exists!)
✅ You already have `portofcontext/homebrew-tap` from the pctx project.

The same tap can host multiple formulas:
- `Formula/pctx.rb` (existing)
- `Formula/portlang.rb` (will be created on first release)

**No action needed!**

### 2. Add GitHub Secret to Portlang Repo
You need to add the same `HOMEBREW_TAP_TOKEN` to the portlang repository:

```bash
gh secret set HOMEBREW_TAP_TOKEN --repo portofcontext/portlang
# Paste the same token you used for pctx
```

**If you don't have the token:**
1. Go to https://github.com/settings/tokens
2. Click "Generate new token (classic)"
3. Name: `HOMEBREW_TAP_TOKEN_PORTLANG` (or any name)
4. Scopes: `repo` + `workflow`
5. Copy and paste when prompted above

## Release a New Version

### 1. Update version in Cargo.toml
```bash
# Edit Cargo.toml [workspace.package] section
# Change: version = "0.1.0" to your new version
```

### 2. Commit and push
```bash
git add Cargo.toml
git commit -m "Bump version to X.Y.Z"
git push
```

### 3. Run release script
```bash
./release.sh v0.1.0
```

### 4. Monitor progress
```bash
gh run watch
# Or visit: https://github.com/portofcontext/portlang/actions
```

### 5. Test installation
```bash
brew tap portofcontext/tap
brew install portlang
portlang --version
```

## What Gets Built

- ✅ Apple Silicon Mac binary (`aarch64-apple-darwin`)
- ✅ GitHub Release with downloadable archives
- ✅ Homebrew formula published to `portofcontext/homebrew-tap`

## Targets

**Apple Silicon Only** - Intel Macs are not supported (required for Docker container compatibility).

## Files You Created

- `dist-workspace.toml` - cargo-dist configuration
- `.github/workflows/release.yml` - Auto-generated release workflow (don't edit manually!)
- `release.sh` - Simple release script
- `RELEASE.md` - Detailed documentation
- `RELEASE_QUICKSTART.md` - This file

## Common Issues

**Error: HOMEBREW_TAP_TOKEN not found**
→ Complete setup step 2-3 above

**Error: Repository not found: portofcontext/homebrew-tap**
→ Complete setup step 1 above

**Version mismatch**
→ Ensure Cargo.toml version matches the tag (without 'v' prefix)

## More Info

See `RELEASE.md` for detailed documentation and troubleshooting.
