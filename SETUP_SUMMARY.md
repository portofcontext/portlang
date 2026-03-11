# cargo-dist Setup Summary for Portlang

## What Was Configured

Successfully set up **cargo-dist** for automated releases of the portlang CLI to Homebrew (Mac only).

## Files Created/Modified

### Created by cargo-dist CLI:
1. **`dist-workspace.toml`** - Main configuration file
   - Mac-only targets (Intel + Apple Silicon)
   - Homebrew installer only
   - Workflow dispatch releases (manual trigger, not git tags)
   - Publishing to `portofcontext/homebrew-tap`

2. **`.github/workflows/release.yml`** - Auto-generated GitHub Actions workflow
   - Builds Mac binaries
   - Creates GitHub Releases
   - Publishes to Homebrew tap
   - **Note:** This file is auto-generated - don't edit manually!

3. **`Cargo.toml`** - Added `[profile.dist]` section for optimized builds

### Created manually:
4. **`release.sh`** - Simple script to trigger releases
5. **`RELEASE.md`** - Detailed release documentation
6. **`RELEASE_QUICKSTART.md`** - Quick reference guide
7. **`SETUP_SUMMARY.md`** - This file

### Modified:
8. **`crates/portlang-cli/Cargo.toml`** - Added description and homepage for Homebrew
9. **`Cargo.toml`** - Updated repository URL

## Configuration Details

### Targets (Apple Silicon Only)
```toml
targets = ["aarch64-apple-darwin"]
```

### Installers
```toml
installers = ["homebrew"]
```

### Homebrew Tap
```toml
tap = "portofcontext/homebrew-tap"
publish-jobs = ["homebrew"]
```

### Release Trigger
```toml
dispatch-releases = true  # Manual workflow_dispatch, not git tag pushes
```

## How to Release

### Quick version:
```bash
./release.sh v0.1.0
```

### Manual version:
```bash
# 1. Update version in Cargo.toml
# 2. Commit and push
git add Cargo.toml
git commit -m "Bump version to 0.1.0"
git push

# 3. Trigger release
gh workflow run release.yml --ref main -f tag="v0.1.0"

# 4. Monitor
gh run watch
```

## One-Time Setup Required

Before your first release, you need to:

### 1. Homebrew tap repository
✅ **Already exists!** You're reusing `portofcontext/homebrew-tap` from pctx.

The tap will contain both formulas:
- `Formula/pctx.rb` (existing)
- `Formula/portlang.rb` (new)

### 2. Add token to portlang repository
You already have the `HOMEBREW_TAP_TOKEN` from pctx. Add it to portlang:

```bash
gh secret set HOMEBREW_TAP_TOKEN --repo portofcontext/portlang
# Use the same token value as pctx
```

**If you don't have the token saved:**
- Create a new token at: https://github.com/settings/tokens/new
- Scopes: `repo` + `workflow`
- Add it to portlang repo with command above

## Testing

To test without publishing:
```bash
gh workflow run release.yml --ref main -f tag="dry-run"
```

## What Gets Published

When you release `v0.1.0`:

1. **GitHub Release** at `https://github.com/portofcontext/portlang/releases/tag/v0.1.0`
   - `portlang-aarch64-apple-darwin.tar.gz` (Apple Silicon)
   - SHA256 checksums

2. **Homebrew Formula** pushed to `portofcontext/homebrew-tap`
   - `Formula/portlang.rb`

3. **Users can install with:**
   ```bash
   brew tap portofcontext/tap
   brew install portlang
   ```

## Updating Configuration

To change release settings:

1. Edit `dist-workspace.toml`
2. Run `dist generate` to regenerate the workflow
3. Commit both files

**Never edit `.github/workflows/release.yml` directly!**

## Maintenance

### Update cargo-dist version:
```bash
dist update
dist generate
git add dist-workspace.toml .github/workflows/release.yml
git commit -m "Update cargo-dist"
```

### Add more platforms:
Edit `dist-workspace.toml` targets array, then run `dist generate`.

### Change installers:
Edit `dist-workspace.toml` installers array, then run `dist generate`.

## Architecture

```
┌─────────────────┐
│  ./release.sh   │
└────────┬────────┘
         │
         v
┌─────────────────────────┐
│ GitHub Actions Workflow │
│   (release.yml)         │
└────────┬────────────────┘
         │
         ├─> Build Mac binaries (Intel + ARM)
         ├─> Create GitHub Release + upload artifacts
         ├─> Generate Homebrew formula
         └─> Push formula to homebrew-tap repo
                 │
                 v
         ┌──────────────────┐
         │  Users install:  │
         │  brew install    │
         │  portlang        │
         └──────────────────┘
```

## Comparison with pctx Setup

Based on `pctx` repository setup, portlang uses the same pattern:
- ✅ cargo-dist 0.30.2
- ✅ GitHub Actions CI
- ✅ workflow_dispatch releases
- ✅ Homebrew tap publishing
- ✅ GitHub Attestations
- ✅ SHA256 checksums

**Differences:**
- ❌ No npm publishing (pctx has this)
- ❌ No Linux/Windows builds (Apple Silicon only)
- ❌ No Intel Mac support (Apple Silicon only, required for Docker containers)
- ❌ No shell installer (Homebrew only)

## Support

- cargo-dist docs: https://axodotdev.github.io/cargo-dist/
- cargo-dist issues: https://github.com/axodotdev/cargo-dist/issues
- portlang issues: https://github.com/portofcontext/portlang/issues

## Next Steps

1. Complete one-time setup (create tap repo, add GitHub token)
2. Test with a dry run: `gh workflow run release.yml --ref main -f tag="dry-run"`
3. Release your first version: `./release.sh v0.1.0`
4. Share installation instructions with users:
   ```bash
   brew tap portofcontext/tap
   brew install portlang
   ```
