### CLI Changes
If you change anything in the CLI, run `cargo run docs` to regenerate CLI.md


### Field config file changes
If you change anything about the field functionality or structure (crates/portlang-core/src/types/field.rs) be sure to update field.structure AND the LSP functionalities in editors/

### Releasing the LSP is currently a manual process
cargo build --release -p portlang-lsp
cp target/release/portlang-lsp editors/vscode/bin/portlang-lsp
cd editors/vscode
npm run package
This produces a portlang-0.1.0.vsix file in editors/vscode/
drag-and-drop it at marketplace.visualstudio.com/manage.