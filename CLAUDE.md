### Skills

- Use `$release-prep` before any release (see Releasing section below)
- Use `$rust-best-practices` when writing or reviewing Rust code

### CLI Changes
If you change anything in the CLI, run `cargo run docs` to regenerate CLI.md


### Field config file changes
If you change anything about the field functionality or structure (crates/portlang-core/src/types/field.rs) be sure to update field.structure AND the LSP functionalities in editors/

### Releasing

Run `$release-prep` (the agent skill) before `./release.sh <version>`. It handles:
- Syncing `CLI.md` to `../skills/portlang/reference/CLI.md`
- Bumping `editors/vscode/package.json` version to match Cargo
- Reviewing and bumping `../skills/portlang/SKILL.md` version
- Pre-release checklist

After `./release.sh <version>`, the `vscode-build` GitHub Actions workflow automatically:
- Builds `portlang-lsp` binary
- Packages the VSCode extension (`.vsix`)
- Attaches the `.vsix` to the GitHub Release

Final manual step: upload the `.vsix` to marketplace.visualstudio.com/manage.