use anyhow::{Context, Result};
use portlang_core::{Action, Skill, SkillSourceKind, TrajectoryStep};
use std::path::Path;

/// Write each resolved skill's content to `.portlang_skills/<slug>/SKILL.md`
/// in the workspace so the agent can read them on-demand via bash / read tool.
///
/// For local directory-based skills, also copies `scripts/`, `references/`, and
/// `assets/` subdirectories so bundled executables and reference files are reachable.
pub async fn write_skills_to_workspace(skills: &[Skill], workspace: &Path) -> Result<()> {
    for skill in skills {
        let Some(ref content) = skill.content else {
            continue;
        };
        let skill_dir = workspace.join(".portlang_skills").join(&skill.slug);
        tokio::fs::create_dir_all(&skill_dir)
            .await
            .with_context(|| format!("Failed to create skill directory for {}", skill.slug))?;
        tokio::fs::write(skill_dir.join("SKILL.md"), content)
            .await
            .with_context(|| format!("Failed to write SKILL.md for {}", skill.slug))?;

        // Copy bundled resources from local directory-based skills.
        if let SkillSourceKind::Local { ref path } = skill.kind {
            if path.is_dir() {
                copy_skill_resources(path, &skill_dir).await;
            }
        }
    }
    Ok(())
}

/// Copy `scripts/`, `references/`, and `assets/` files from a skill source
/// directory into the corresponding destination directory in the workspace.
async fn copy_skill_resources(src: &Path, dst: &Path) {
    for subdir in &["scripts", "references", "assets"] {
        let src_sub = src.join(subdir);
        if !src_sub.is_dir() {
            continue;
        }
        let dst_sub = dst.join(subdir);
        if let Err(e) = tokio::fs::create_dir_all(&dst_sub).await {
            tracing::warn!("Failed to create resource dir {}: {}", dst_sub.display(), e);
            continue;
        }
        let mut entries = match tokio::fs::read_dir(&src_sub).await {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to read {}: {}", src_sub.display(), e);
                continue;
            }
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let Ok(ft) = entry.file_type().await else {
                continue;
            };
            if ft.is_file() {
                let dst_file = dst_sub.join(entry.file_name());
                if let Err(e) = tokio::fs::copy(entry.path(), &dst_file).await {
                    tracing::warn!(
                        "Failed to copy skill resource {}: {}",
                        entry.path().display(),
                        e
                    );
                }
            }
        }
    }
}

/// Build the system prompt metadata block for skills (Tier 1 progressive disclosure).
///
/// Only name + description (~100 tokens/skill) appear here; full SKILL.md content
/// lives on the workspace filesystem and is read on-demand.
///
/// Spec compliance:
/// - Skills with a missing or empty `description` are omitted entirely.
/// - Bundled resource paths (`scripts/`, `references/`, `assets/`) are enumerated
///   so the agent knows they exist before activating the skill.
/// - A brief instruction tells the agent to resolve relative paths against the
///   skill directory.
pub fn build_skill_metadata_block(skills: &[Skill]) -> String {
    let workspace_path = "/workspace/.portlang_skills";

    // Spec: skip skills with missing/empty description — a description is
    // essential for the agent to know when to activate the skill.
    let listable: Vec<&Skill> = skills
        .iter()
        .filter(|s| {
            s.content.is_some()
                && extract_skill_description(s.content.as_deref().unwrap_or("")).is_some()
        })
        .collect();

    if listable.is_empty() {
        return String::new();
    }

    let mut block = format!(
        "## Skills\n\n\
         The following skills are available at `{wp}/`. \
         Read a skill's SKILL.md when it is relevant to the task. \
         Paths referenced in skill instructions are relative to the skill \
         directory — resolve them to absolute paths before use \
         (e.g. `{wp}/<slug>/scripts/foo.py`).\n\n",
        wp = workspace_path
    );

    for skill in &listable {
        let content = skill.content.as_deref().unwrap_or("");
        let name = extract_skill_name(content).unwrap_or_else(|| skill.slug.clone());
        let description =
            extract_skill_description(content).expect("already filtered for Some(description)");

        block.push_str(&format!(
            "- **{}** (`{}/{}/SKILL.md`): {}",
            name, workspace_path, skill.slug, description
        ));

        // Enumerate bundled resources so the agent knows they exist.
        if !skill.resources.is_empty() {
            let listed = skill
                .resources
                .iter()
                .map(|r| format!("`{}/{}/{}`", workspace_path, skill.slug, r))
                .collect::<Vec<_>>()
                .join(", ");
            block.push_str(&format!(" — bundled: {}", listed));
        }

        block.push('\n');
    }

    block
}

/// Extract the `allowed-tools` field from a SKILL.md YAML frontmatter block.
///
/// Spec: space-delimited list of pre-approved tools (experimental).
/// Returns `None` if the field is absent; returns an empty vec if the field
/// is present but blank.
pub fn extract_allowed_tools(content: &str) -> Option<Vec<String>> {
    // The key contains a hyphen so we use the shared extractor directly.
    frontmatter_field(content, "allowed-tools")
        .map(|val| val.split_whitespace().map(|s| s.to_string()).collect())
}

/// Extract the `name` field from a SKILL.md YAML frontmatter block.
///
/// Spec: the `name` field is required (1-64 chars, lowercase+hyphens only).
/// Returns `None` if there is no frontmatter or the field is absent.
pub fn extract_skill_name(content: &str) -> Option<String> {
    frontmatter_field(content, "name")
}

/// Extract the `description` field from a SKILL.md YAML frontmatter block.
pub fn extract_skill_description(content: &str) -> Option<String> {
    frontmatter_field(content, "description")
}

/// Validate a SKILL.md `name` field against the Agent Skills spec constraints:
/// - 1–64 characters
/// - Lowercase ASCII alphanumeric (`a-z`, `0-9`) and hyphens only
/// - Must not start or end with `-`
/// - Must not contain `--`
///
/// Returns `Ok(())` if valid, `Err(reason)` if not.
/// Callers should warn but still load the skill on validation failure (lenient mode).
pub fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("name must not be empty".into());
    }
    if name.len() > 64 {
        return Err(format!(
            "name exceeds 64 characters ({} chars): {:?}",
            name.len(),
            name
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(format!(
            "name contains invalid characters (only a-z, 0-9, hyphens allowed): {:?}",
            name
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(format!(
            "name must not start or end with a hyphen: {:?}",
            name
        ));
    }
    if name.contains("--") {
        return Err(format!(
            "name must not contain consecutive hyphens: {:?}",
            name
        ));
    }
    Ok(())
}

/// Shared frontmatter field extractor.
///
/// Handles simple `key: value`, quoted `key: "value"`, and the lenient case of
/// values containing unquoted colons (`key: foo: bar` → `foo: bar`).
fn frontmatter_field(content: &str, field: &str) -> Option<String> {
    let content = content.trim();
    if !content.starts_with("---") {
        return None;
    }
    let rest = &content[3..];
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let prefix = format!("{}:", field);
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix(&prefix) {
            let val = val.trim().trim_matches('"');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Skill, SkillSourceKind};
    use std::path::PathBuf;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn skill_with_content(slug: &str, content: &str) -> Skill {
        Skill {
            source: slug.to_string(),
            kind: SkillSourceKind::Local {
                path: PathBuf::from(format!("/fake/{slug}.md")),
            },
            slug: slug.to_string(),
            content: Some(content.to_string()),
            resources: Vec::new(),
        }
    }

    fn skill_with_resources(slug: &str, content: &str, resources: &[&str]) -> Skill {
        Skill {
            source: slug.to_string(),
            kind: SkillSourceKind::Local {
                path: PathBuf::from(format!("/fake/{slug}")),
            },
            slug: slug.to_string(),
            content: Some(content.to_string()),
            resources: resources.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn skill_no_content(slug: &str) -> Skill {
        Skill {
            source: slug.to_string(),
            kind: SkillSourceKind::Local {
                path: PathBuf::from(format!("/fake/{slug}.md")),
            },
            slug: slug.to_string(),
            content: None,
            resources: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Spec §SKILL.md format — frontmatter extraction
    // -----------------------------------------------------------------------

    /// Spec: `name` and `description` are required fields in YAML frontmatter.
    #[test]
    fn spec_extract_name_from_minimal_skill() {
        let content = "---\nname: pdf-processing\ndescription: Handle PDFs.\n---\n\nBody.";
        assert_eq!(
            extract_skill_name(content).as_deref(),
            Some("pdf-processing")
        );
    }

    /// Spec: `description` extraction from minimal well-formed frontmatter.
    #[test]
    fn spec_extract_description_from_minimal_skill() {
        let content = "---\nname: pdf-processing\ndescription: Handle PDFs.\n---\n\nBody.";
        assert_eq!(
            extract_skill_description(content).as_deref(),
            Some("Handle PDFs.")
        );
    }

    /// Spec: missing `name` field → None (should warn + load anyway per lenient policy).
    #[test]
    fn spec_extract_name_missing_returns_none() {
        let content = "---\ndescription: No name field here.\n---\n\nBody.";
        assert!(extract_skill_name(content).is_none());
    }

    /// Spec: no frontmatter block → both fields are None.
    #[test]
    fn spec_extract_no_frontmatter_returns_none() {
        let content = "# Just markdown, no frontmatter";
        assert!(extract_skill_name(content).is_none());
        assert!(extract_skill_description(content).is_none());
    }

    /// Spec: quoted string values are accepted.
    #[test]
    fn spec_extract_description_quoted_value() {
        let content = "---\nname: my-skill\ndescription: \"Quoted description.\"\n---\n";
        assert_eq!(
            extract_skill_description(content).as_deref(),
            Some("Quoted description.")
        );
    }

    /// Spec (lenient): description containing a colon should still parse.
    /// Common issue: `description: Use when: the user asks about X`
    #[test]
    fn spec_extract_description_lenient_unquoted_colon_in_value() {
        let content = "---\nname: my-skill\ndescription: Use when: the user asks about PDFs\n---\n";
        let desc = extract_skill_description(content).unwrap();
        // The full value after "description:" should be returned, colon and all
        assert!(
            desc.contains("Use when"),
            "description should be extracted despite colon in value"
        );
    }

    /// Spec: optional fields (license, compatibility, metadata, allowed-tools)
    /// don't interfere with required field extraction.
    #[test]
    fn spec_extract_with_optional_fields_present() {
        let content = "---\nname: pdf-processing\ndescription: Extract PDFs.\nlicense: Apache-2.0\ncompatibility: Requires pdftools\n---\n";
        assert_eq!(
            extract_skill_name(content).as_deref(),
            Some("pdf-processing")
        );
        assert_eq!(
            extract_skill_description(content).as_deref(),
            Some("Extract PDFs.")
        );
    }

    // -----------------------------------------------------------------------
    // Spec §name field — validation constraints
    // -----------------------------------------------------------------------

    /// Spec: valid names — lowercase alphanumeric + hyphens.
    #[test]
    fn spec_validate_name_valid_pdf_processing() {
        assert!(validate_skill_name("pdf-processing").is_ok());
    }

    #[test]
    fn spec_validate_name_valid_code_review() {
        assert!(validate_skill_name("code-review").is_ok());
    }

    #[test]
    fn spec_validate_name_valid_data_analysis() {
        assert!(validate_skill_name("data-analysis").is_ok());
    }

    #[test]
    fn spec_validate_name_valid_single_char() {
        assert!(validate_skill_name("a").is_ok());
    }

    #[test]
    fn spec_validate_name_valid_alphanumeric() {
        assert!(validate_skill_name("skill2").is_ok());
    }

    /// Spec: max 64 characters — exactly 64 is valid.
    #[test]
    fn spec_validate_name_valid_exactly_64_chars() {
        let name = "a".repeat(64);
        assert!(validate_skill_name(&name).is_ok());
    }

    /// Spec: max 64 characters — 65 is invalid.
    #[test]
    fn spec_validate_name_invalid_65_chars_too_long() {
        let name = "a".repeat(65);
        let err = validate_skill_name(&name).unwrap_err();
        assert!(
            err.contains("64"),
            "error should mention the 64-char limit: {err}"
        );
    }

    /// Spec: uppercase not allowed.
    #[test]
    fn spec_validate_name_invalid_uppercase() {
        let err = validate_skill_name("PDF-Processing").unwrap_err();
        assert!(
            err.contains("invalid characters"),
            "error should mention invalid chars: {err}"
        );
    }

    /// Spec: must not start with a hyphen.
    #[test]
    fn spec_validate_name_invalid_leading_hyphen() {
        let err = validate_skill_name("-pdf").unwrap_err();
        assert!(
            err.contains("hyphen"),
            "error should mention hyphen constraint: {err}"
        );
    }

    /// Spec: must not end with a hyphen.
    #[test]
    fn spec_validate_name_invalid_trailing_hyphen() {
        let err = validate_skill_name("pdf-").unwrap_err();
        assert!(
            err.contains("hyphen"),
            "error should mention hyphen constraint: {err}"
        );
    }

    /// Spec: must not contain consecutive hyphens.
    #[test]
    fn spec_validate_name_invalid_consecutive_hyphens() {
        let err = validate_skill_name("pdf--processing").unwrap_err();
        assert!(
            err.contains("consecutive"),
            "error should mention consecutive hyphens: {err}"
        );
    }

    /// Spec: must not be empty.
    #[test]
    fn spec_validate_name_invalid_empty() {
        let err = validate_skill_name("").unwrap_err();
        assert!(!err.is_empty(), "should return a non-empty error message");
    }

    /// Spec: special characters (spaces, underscores) not allowed.
    #[test]
    fn spec_validate_name_invalid_underscore() {
        assert!(validate_skill_name("my_skill").is_err());
    }

    #[test]
    fn spec_validate_name_invalid_space() {
        assert!(validate_skill_name("my skill").is_err());
    }

    // -----------------------------------------------------------------------
    // Spec §Progressive disclosure — Tier 1: catalog (~50–100 tokens/skill)
    // -----------------------------------------------------------------------

    /// Spec: "The name and description fields are loaded at startup for all skills."
    /// Metadata block must contain both name and description, not full body.
    #[test]
    fn spec_tier1_metadata_block_contains_name_and_description() {
        let skills = vec![skill_with_content(
            "code-review",
            "---\nname: code-review\ndescription: Opinionated code review guide.\n---\n\nFull instructions here.",
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(block.contains("code-review"), "should contain skill name");
        assert!(
            block.contains("Opinionated code review guide."),
            "should contain description"
        );
        // Full body should NOT appear in the metadata block (progressive disclosure)
        assert!(
            !block.contains("Full instructions here."),
            "full body must not appear in tier-1 metadata block"
        );
    }

    /// Spec: when frontmatter `name` differs from the derived slug,
    /// the frontmatter name is used for display.
    #[test]
    fn spec_tier1_metadata_uses_frontmatter_name_over_slug() {
        let skills = vec![skill_with_content(
            "derived-slug",
            "---\nname: canonical-name\ndescription: A skill.\n---\n",
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(
            block.contains("canonical-name"),
            "should use frontmatter name"
        );
    }

    /// Spec: the path to SKILL.md is included so the agent can activate it.
    #[test]
    fn spec_tier1_metadata_includes_skill_path_for_activation() {
        let skills = vec![skill_with_content(
            "my-skill",
            "---\nname: my-skill\ndescription: A skill.\n---\n",
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(
            block.contains("SKILL.md"),
            "path to SKILL.md must be present for agent activation"
        );
        assert!(
            block.contains("my-skill"),
            "slug must appear in path so agent can locate the file"
        );
    }

    /// Spec: "If no skills are discovered, omit the catalog … entirely."
    #[test]
    fn spec_tier1_metadata_empty_when_no_skills() {
        assert_eq!(build_skill_metadata_block(&[]), "");
    }

    /// Spec: unresolved skills must not appear in the catalog
    /// (prevents the model from trying to load skills it can't use).
    #[test]
    fn spec_tier1_metadata_excludes_unresolved_skills() {
        let skills = vec![
            skill_with_content("resolved", "---\nname: resolved\ndescription: OK.\n---\n"),
            skill_no_content("unresolved"),
        ];
        let block = build_skill_metadata_block(&skills);
        assert!(block.contains("resolved"), "resolved skill should appear");
        assert!(
            !block.contains("unresolved"),
            "unresolved skill must not appear in catalog"
        );
    }

    #[test]
    fn spec_tier1_metadata_empty_when_all_unresolved() {
        let skills = vec![skill_no_content("a"), skill_no_content("b")];
        assert_eq!(build_skill_metadata_block(&skills), "");
    }

    // -----------------------------------------------------------------------
    // Spec §Progressive disclosure — Tier 2: full content in workspace
    // -----------------------------------------------------------------------

    /// Spec: full SKILL.md is written to workspace filesystem under `<slug>/SKILL.md`.
    /// Directory structure: `<workspace>/.portlang_skills/<slug>/SKILL.md`.
    #[tokio::test]
    async fn spec_tier2_write_creates_slug_directory_with_skill_md() {
        let dir = TempDir::new().unwrap();
        let skills = vec![skill_with_content(
            "pdf-processing",
            "---\nname: pdf-processing\ndescription: Handle PDFs.\n---\n\nFull body here.",
        )];

        write_skills_to_workspace(&skills, dir.path())
            .await
            .unwrap();

        let skill_md = dir
            .path()
            .join(".portlang_skills")
            .join("pdf-processing")
            .join("SKILL.md");
        assert!(
            skill_md.exists(),
            "SKILL.md must be at <workspace>/.portlang_skills/<slug>/SKILL.md"
        );
        let written = std::fs::read_to_string(&skill_md).unwrap();
        assert!(
            written.contains("Full body here."),
            "full SKILL.md content must be written to workspace"
        );
    }

    /// Spec: unresolved skills produce no workspace file (nothing to write).
    #[tokio::test]
    async fn spec_tier2_write_skips_unresolved_skills() {
        let dir = TempDir::new().unwrap();
        let skills = vec![skill_no_content("ghost")];
        write_skills_to_workspace(&skills, dir.path())
            .await
            .unwrap();
        let skill_dir = dir.path().join(".portlang_skills").join("ghost");
        assert!(
            !skill_dir.exists(),
            "no directory should be created for an unresolved skill"
        );
    }

    /// Spec: multiple skills each get their own subdirectory.
    #[tokio::test]
    async fn spec_tier2_write_creates_separate_dir_per_skill() {
        let dir = TempDir::new().unwrap();
        let skills = vec![
            skill_with_content("skill-a", "---\nname: skill-a\ndescription: A.\n---\n"),
            skill_with_content("skill-b", "---\nname: skill-b\ndescription: B.\n---\n"),
        ];
        write_skills_to_workspace(&skills, dir.path())
            .await
            .unwrap();
        assert!(dir
            .path()
            .join(".portlang_skills/skill-a/SKILL.md")
            .exists());
        assert!(dir
            .path()
            .join(".portlang_skills/skill-b/SKILL.md")
            .exists());
    }

    // -----------------------------------------------------------------------
    // Gap 1+2: Directory-based local skills + scripts/ copied to workspace
    // -----------------------------------------------------------------------

    /// A local directory skill writes scripts/ into the workspace alongside SKILL.md.
    #[tokio::test]
    async fn spec_dir_skill_copies_scripts_to_workspace() {
        let src = TempDir::new().unwrap();
        // Create skill directory structure
        std::fs::write(
            src.path().join("SKILL.md"),
            "---\nname: my-skill\ndescription: Does stuff.\n---\n\nRun `scripts/run.sh`.",
        )
        .unwrap();
        let scripts_dir = src.path().join("scripts");
        std::fs::create_dir(&scripts_dir).unwrap();
        std::fs::write(scripts_dir.join("run.sh"), "#!/bin/bash\necho hello").unwrap();

        let workspace = TempDir::new().unwrap();
        let skill = Skill {
            source: src.path().to_string_lossy().to_string(),
            kind: SkillSourceKind::Local {
                path: src.path().to_path_buf(),
            },
            slug: "my-skill".to_string(),
            content: Some(
                "---\nname: my-skill\ndescription: Does stuff.\n---\n\nRun `scripts/run.sh`."
                    .to_string(),
            ),
            resources: vec!["scripts/run.sh".to_string()],
        };

        write_skills_to_workspace(&[skill], workspace.path())
            .await
            .unwrap();

        assert!(workspace
            .path()
            .join(".portlang_skills/my-skill/SKILL.md")
            .exists());
        assert!(
            workspace
                .path()
                .join(".portlang_skills/my-skill/scripts/run.sh")
                .exists(),
            "scripts/run.sh must be copied into workspace skill directory"
        );
    }

    /// A local directory skill with references/ and assets/ copies all three subdirs.
    #[tokio::test]
    async fn spec_dir_skill_copies_all_resource_subdirs() {
        let src = TempDir::new().unwrap();
        std::fs::write(
            src.path().join("SKILL.md"),
            "---\nname: s\ndescription: D.\n---\n",
        )
        .unwrap();
        for subdir in &["scripts", "references", "assets"] {
            let d = src.path().join(subdir);
            std::fs::create_dir(&d).unwrap();
            std::fs::write(d.join("file.txt"), "content").unwrap();
        }

        let workspace = TempDir::new().unwrap();
        let skill = Skill {
            source: src.path().to_string_lossy().to_string(),
            kind: SkillSourceKind::Local {
                path: src.path().to_path_buf(),
            },
            slug: "s".to_string(),
            content: Some("---\nname: s\ndescription: D.\n---\n".to_string()),
            resources: vec![
                "assets/file.txt".to_string(),
                "references/file.txt".to_string(),
                "scripts/file.txt".to_string(),
            ],
        };

        write_skills_to_workspace(&[skill], workspace.path())
            .await
            .unwrap();

        for subdir in &["scripts", "references", "assets"] {
            assert!(
                workspace
                    .path()
                    .join(format!(".portlang_skills/s/{subdir}/file.txt"))
                    .exists(),
                "{subdir}/file.txt must be copied"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Gap 3+4: Resource enumeration in catalog + relative path instruction
    // -----------------------------------------------------------------------

    /// Spec: catalog entry must enumerate bundled resources so agent knows scripts exist.
    #[test]
    fn spec_tier1_metadata_enumerates_bundled_resources() {
        let skills = vec![skill_with_resources(
            "pdf-skill",
            "---\nname: pdf-skill\ndescription: Handle PDFs.\n---\n",
            &["scripts/extract.py", "references/guide.md"],
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(
            block.contains("scripts/extract.py"),
            "catalog must list bundled scripts"
        );
        assert!(
            block.contains("references/guide.md"),
            "catalog must list bundled references"
        );
    }

    /// Spec: catalog must instruct agent to resolve relative paths to absolute.
    #[test]
    fn spec_tier1_metadata_includes_relative_path_instruction() {
        let skills = vec![skill_with_content(
            "my-skill",
            "---\nname: my-skill\ndescription: A skill.\n---\n",
        )];
        let block = build_skill_metadata_block(&skills);
        assert!(
            block.to_lowercase().contains("relative") || block.contains("absolute"),
            "metadata block must tell agent how to resolve relative paths: {block}"
        );
    }

    // -----------------------------------------------------------------------
    // Gap 6: Skip skills with missing/empty description
    // -----------------------------------------------------------------------

    /// Spec: skill with no description field must be omitted from the catalog.
    #[test]
    fn spec_skill_without_description_omitted_from_catalog() {
        let skills = vec![
            skill_with_content(
                "good",
                "---\nname: good\ndescription: I have a description.\n---\n",
            ),
            skill_with_content(
                "no-desc",
                "---\nname: no-desc\n---\n\nBody but no description.",
            ),
        ];
        let block = build_skill_metadata_block(&skills);
        assert!(block.contains("good"), "skill with description must appear");
        assert!(
            !block.contains("no-desc"),
            "skill without description must be omitted"
        );
    }

    // -----------------------------------------------------------------------
    // Gap 8: allowed-tools field parsing
    // -----------------------------------------------------------------------

    #[test]
    fn spec_extract_allowed_tools_space_delimited() {
        let content = "---\nname: s\ndescription: D.\nallowed-tools: Bash(git:*) Read Write\n---\n";
        let tools = extract_allowed_tools(content).unwrap();
        assert_eq!(tools, vec!["Bash(git:*)", "Read", "Write"]);
    }

    #[test]
    fn spec_extract_allowed_tools_absent_returns_none() {
        let content = "---\nname: s\ndescription: D.\n---\n";
        assert!(extract_allowed_tools(content).is_none());
    }
}

/// Heuristically detect which skills were invoked during the run.
/// Scans goal text and all assistant text / tool-call steps for `$slug` patterns.
pub fn detect_skill_invocations(
    steps: &[TrajectoryStep],
    goal: &str,
    skills: &[Skill],
) -> Vec<String> {
    let mut invoked = Vec::new();
    for skill in skills {
        let pattern = format!("${}", skill.slug);
        let cat_pattern = format!(".portlang_skills/{}/SKILL.md", skill.slug);
        let mentioned = goal.contains(&pattern)
            || goal.contains(&cat_pattern)
            || steps.iter().any(|step| {
                if let Action::TextOutput { text } = &step.action {
                    text.contains(&pattern) || text.contains(&cat_pattern)
                } else if let Action::ToolCall { input, .. } = &step.action {
                    let s = input.to_string();
                    s.contains(&cat_pattern)
                } else {
                    false
                }
            });
        if mentioned {
            invoked.push(skill.slug.clone());
        }
    }
    invoked
}
