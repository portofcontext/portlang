---
name: release-prep
description: >
  Prepares a portlang release by syncing downstream artifacts. Use before ./release.sh:
  syncs CLI.md to the skills repo, bumps version in editors/vscode/package.json and
  ../skills/portlang/SKILL.md, reviews SKILL.md content for staleness, and prints a
  pre-release checklist. Trigger when: cutting a release, CLI commands changed,
  field.rs or LSP behavior changed, or version numbers are out of sync across artifacts.
license: MIT
metadata:
  author: portofcontext
  version: "1.0.0"
---

# Release Prep

Syncs all downstream portlang artifacts before a release. Run this before `./release.sh <version>`.

## Phase 1: Read Current State

Read these sources of truth in parallel:

1. `Cargo.toml` — get `[workspace.package].version`
2. `editors/vscode/package.json` — get `"version"` field
3. `../skills/portlang/SKILL.md` — get `metadata.version` from frontmatter

Print a version comparison table:

| Artifact | Current version |
|---|---|
| Cargo workspace | X.Y.Z |
| VSCode extension | X.Y.Z |
| portlang skill | X.Y.Z |

Flag any that don't match the Cargo workspace version (VSCode extension must match; skill version is independent).

## Phase 2: Regenerate CLI.md

```bash
cargo run docs
```

Then show what changed:

```bash
git diff CLI.md
```

Note any new commands, changed flags, or removed options — this informs the skill content review in Phase 5.

## Phase 3: Bump Versions

**3a. VSCode extension**

Edit `editors/vscode/package.json`: set `"version"` to match `[workspace.package].version` from `Cargo.toml`.

Then run `npm install` in the extension directory to sync the lockfile:

```bash
cd editors/vscode && npm install
```

**3b. portlang skill**

The skill version in `../skills/portlang/SKILL.md` (`metadata.version`) follows its own semver. Choose the bump based on what changed since the last portlang release:

- New CLI commands, new field types, new features → **minor** bump (e.g. `1.2.9` → `1.3.0`)
- Bug fixes, flag renames, doc corrections → **patch** bump (e.g. `1.2.9` → `1.2.10`)
- Breaking changes to `.field` syntax or verifier semantics → **major** bump (e.g. `1.2.9` → `2.0.0`)

State the recommended bump with rationale, then apply it by editing the `version:` line in `../skills/portlang/SKILL.md` frontmatter.

## Phase 4: Sync CLI.md to Skills Repo

```bash
cp CLI.md ../skills/portlang/reference/CLI.md
diff CLI.md ../skills/portlang/reference/CLI.md && echo "In sync" || echo "MISMATCH - check above"
```

## Phase 5: Review Skill Content

Get context on what changed:

```bash
git log --oneline -10
git diff HEAD~1 -- crates/portlang-core/src/types/field.rs crates/portlang-cli/src/
```

Read `../skills/portlang/SKILL.md` and check for stale sections based on the diff:

- New CLI flags or commands → update "Essential Commands" section
- New or changed field struct fields → update "Field File Structure" section
- New verifier types → update verifier examples
- New runner options → update runner documentation
- Changed defaults → update any mentions of those defaults

Propose specific edits for each stale section. Apply mechanical changes (e.g. a renamed flag) directly. Ask for confirmation on content additions or rewrites.

## Phase 6: Show Skills Repo Diff

The `../skills` directory is a separate git repo. Show what changed — the user handles staging and committing themselves:

```bash
cd ../skills && git diff
```

Do not run `git add` or `git commit`.

## Phase 7: Pre-Release Checklist

Print with status for each item:

```
Pre-release checklist for portlang v<VERSION>:

Code
[ ] cargo fmt
[ ] cargo test --workspace --all-features passes
[ ] cargo clippy clean

Documentation
[ ] CLI.md regenerated and committed (cargo run docs)
[ ] field.structure updated (if field.rs changed)

Version bumps
[x] Cargo.toml [workspace.package].version = <VERSION>
[x] editors/vscode/package.json version = <VERSION>
[x] editors/vscode/package-lock.json synced (npm install)
[x] ../skills/portlang/SKILL.md metadata.version bumped

Skills repo
[x] ../skills/portlang/reference/CLI.md synced
[ ] ../skills/portlang/SKILL.md content reviewed and updated
[ ] ../skills committed and pushed (user does this)

Release
[ ] ./release.sh <VERSION>
[ ] vscode-build CI completes and .vsix attached to GitHub Release (automatic)
    To watch the build from your terminal (blocks until done):
    gh run watch $(gh run list --workflow=vscode-build.yml --limit 1 --json databaseId -q '.[0].databaseId')
[ ] .vsix uploaded to marketplace.visualstudio.com/manage (manual)
[ ] ../skills pushed
```

Mark [x] for items completed during this session.
