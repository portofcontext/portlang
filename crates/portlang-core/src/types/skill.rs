use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The parsed form of a [[skill]] source string
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SkillSourceKind {
    /// GitHub repo — from shorthand `owner/repo`, `github:owner/repo`, or full GitHub URL
    GitHub {
        owner: String,
        repo: String,
        /// Optional git ref (branch/tag/commit), e.g. from `/tree/main/...`
        #[serde(default)]
        ref_: Option<String>,
        /// Optional path within repo to the skill directory or SKILL.md
        #[serde(default)]
        subpath: Option<String>,
        /// Filter: only load the skill matching this name within a multi-skill repo
        #[serde(default)]
        skill_filter: Option<String>,
    },
    /// GitLab instance (gitlab.com or self-hosted)
    GitLab {
        /// Normalized `.git` URL
        url: String,
        #[serde(default)]
        ref_: Option<String>,
        #[serde(default)]
        subpath: Option<String>,
    },
    /// Arbitrary git URL (SSH git@... or non-GitHub/GitLab HTTP)
    Git { url: String },
    /// Any HTTP(S) domain serving `/.well-known/skills/index.json`
    WellKnown { url: String },
    /// ClawHub registry — portlang extension
    ClawHub {
        #[serde(default)]
        org: Option<String>,
        name: String,
    },
    /// Local file or directory (resolved absolute path)
    Local { path: PathBuf },
}

/// A skill declared in a [[skill]] field section.
/// `content` is `None` until resolved by `SkillResolver`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    /// Raw source string from the .field file (e.g. `"owner/repo"`)
    pub source: String,
    /// Parsed source kind
    pub kind: SkillSourceKind,
    /// Short slug used for `$slug` invocation (e.g. `"my-skill"`)
    pub slug: String,
    /// Resolved SKILL.md content — None until fetched by SkillResolver
    #[serde(default)]
    pub content: Option<String>,
    /// Relative paths of bundled resource files discovered during resolution.
    /// Examples: `"scripts/extract.py"`, `"references/guide.md"`, `"assets/template.md"`.
    /// Populated for local directory-based skills; empty for flat-file and remote skills.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
}

/// Parse a skill source string into a `(SkillSourceKind, slug)` pair.
///
/// Mirrors the source-parsing logic from the skills.sh CLI
/// (`https://github.com/vercel-labs/skills`), so portlang `[[skill]]` sources
/// are portable with `npx skills add <source>`.
///
/// Supported formats:
/// - `owner/repo`  — GitHub shorthand
/// - `owner/repo/path/to/skill`  — with subpath
/// - `owner/repo@skill-name`  — skill filter
/// - `github:owner/repo`  — explicit github: prefix (stripped)
/// - `gitlab:owner/repo`  — routed to gitlab.com
/// - full GitHub/GitLab URLs (with `/tree/branch/path` support)
/// - `https://example.com`  — well-known (fetches `/.well-known/skills/index.json`)
/// - `git@github.com:owner/repo.git`  — SSH git URL
/// - `./relative` or `/absolute`  — local path
/// - `clawhub:name` or `clawhub:org/name`  — ClawHub registry
pub fn parse_skill_source(
    input: &str,
    config_dir: &std::path::Path,
) -> Result<(SkillSourceKind, String), String> {
    let input = input.trim();

    // --- clawhub: prefix ---
    if let Some(rest) = input.strip_prefix("clawhub:") {
        let (org, name) = if let Some((org, name)) = rest.split_once('/') {
            (Some(org.to_string()), name.to_string())
        } else {
            (None, rest.to_string())
        };
        let slug = slug_from_name(&name);
        return Ok((SkillSourceKind::ClawHub { org, name }, slug));
    }

    // --- github: prefix — strip and re-parse ---
    if let Some(rest) = input.strip_prefix("github:") {
        return parse_skill_source(rest, config_dir);
    }

    // --- gitlab: prefix — route to gitlab.com ---
    if let Some(rest) = input.strip_prefix("gitlab:") {
        return parse_skill_source(&format!("https://gitlab.com/{}", rest), config_dir);
    }

    // --- local paths: absolute, ./, ../, C:\, . or .. ---
    if is_local_path(input) {
        let resolved = if std::path::Path::new(input).is_absolute() {
            PathBuf::from(input)
        } else {
            config_dir.join(input)
        };
        let slug = resolved
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("local-skill")
            .to_string();
        let slug = slug_from_name(&slug);
        return Ok((SkillSourceKind::Local { path: resolved }, slug));
    }

    // --- GitHub URL: https://github.com/... ---
    if let Some(captures) = try_parse_github_url(input) {
        return Ok(captures);
    }

    // --- GitLab URL: any host with /-/tree/ pattern ---
    if let Some(captures) = try_parse_gitlab_url(input) {
        return Ok(captures);
    }

    // --- SSH git URL: git@host:owner/repo.git ---
    if input.starts_with("git@") {
        let slug = input
            .split('/')
            .next_back()
            .map(|s| s.trim_end_matches(".git"))
            .unwrap_or("skill")
            .to_string();
        let slug = slug_from_name(&slug);
        return Ok((
            SkillSourceKind::Git {
                url: input.to_string(),
            },
            slug,
        ));
    }

    // --- owner/repo shorthand (no colon, no leading . or /) ---
    if !input.contains(':') && !input.starts_with('.') && !input.starts_with('/') {
        if let Some(result) = try_parse_github_shorthand(input) {
            return Ok(result);
        }
    }

    // --- Well-known: arbitrary HTTP(S) URL not already matched ---
    if input.starts_with("http://") || input.starts_with("https://") {
        let slug = url_hostname_slug(input);
        return Ok((
            SkillSourceKind::WellKnown {
                url: input.to_string(),
            },
            slug,
        ));
    }

    Err(format!("Unrecognized skill source format: {:?}", input))
}

// --- helpers ---

fn is_local_path(s: &str) -> bool {
    std::path::Path::new(s).is_absolute()
        || s.starts_with("./")
        || s.starts_with("../")
        || s == "."
        || s == ".."
        || (s.len() >= 3
            && s.as_bytes()[1] == b':'
            && (s.as_bytes()[2] == b'\\' || s.as_bytes()[2] == b'/'))
}

fn slug_from_name(name: &str) -> String {
    // Lowercase, replace non-alphanumeric-hyphen with hyphen, collapse runs
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    // Collapse consecutive hyphens and trim
    let mut slug = String::new();
    let mut prev_hyphen = false;
    for c in s.chars() {
        if c == '-' {
            if !prev_hyphen && !slug.is_empty() {
                slug.push(c);
            }
            prev_hyphen = true;
        } else {
            slug.push(c);
            prev_hyphen = false;
        }
    }
    slug.trim_end_matches('-').to_string()
}

fn url_hostname_slug(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    slug_from_name(host)
}

/// Try to parse GitHub URLs: https://github.com/owner/repo[/tree/ref[/subpath]]
fn try_parse_github_url(input: &str) -> Option<(SkillSourceKind, String)> {
    if !input.contains("github.com/") {
        return None;
    }
    // With /tree/ref/subpath
    if let Some(m) = regex_github_tree_path(input) {
        return Some(m);
    }
    // With /tree/ref only
    if let Some(m) = regex_github_tree(input) {
        return Some(m);
    }
    // Plain https://github.com/owner/repo
    let after = input.split("github.com/").nth(1)?;
    let parts: Vec<&str> = after.splitn(2, '/').collect();
    let owner = parts.first()?.trim_end_matches(".git");
    let repo = parts
        .get(1)
        .map(|r| r.trim_end_matches(".git"))
        .unwrap_or("");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let slug = slug_from_name(repo);
    Some((
        SkillSourceKind::GitHub {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_: None,
            subpath: None,
            skill_filter: None,
        },
        slug,
    ))
}

/// `https://github.com/owner/repo/tree/ref/subpath`
fn regex_github_tree_path(input: &str) -> Option<(SkillSourceKind, String)> {
    // Match: github.com/{owner}/{repo}/tree/{ref}/{subpath}
    let after = input.split("github.com/").nth(1)?;
    let parts: Vec<&str> = after.splitn(5, '/').collect();
    if parts.len() < 5 || parts[2] != "tree" {
        return None;
    }
    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    let ref_ = parts[3];
    let subpath = parts[4];
    let slug = subpath
        .split('/')
        .next_back()
        .map(slug_from_name)
        .unwrap_or_else(|| slug_from_name(repo));
    Some((
        SkillSourceKind::GitHub {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_: Some(ref_.to_string()),
            subpath: Some(subpath.to_string()),
            skill_filter: None,
        },
        slug,
    ))
}

/// `https://github.com/owner/repo/tree/ref`
fn regex_github_tree(input: &str) -> Option<(SkillSourceKind, String)> {
    let after = input.split("github.com/").nth(1)?;
    let parts: Vec<&str> = after.splitn(4, '/').collect();
    if parts.len() < 4 || parts[2] != "tree" {
        return None;
    }
    let owner = parts[0];
    let repo = parts[1].trim_end_matches(".git");
    let ref_ = parts[3];
    let slug = slug_from_name(repo);
    Some((
        SkillSourceKind::GitHub {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_: Some(ref_.to_string()),
            subpath: None,
            skill_filter: None,
        },
        slug,
    ))
}

/// Try to parse GitLab URLs (any host with `/-/tree/` pattern)
fn try_parse_gitlab_url(input: &str) -> Option<(SkillSourceKind, String)> {
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return None;
    }
    if input.contains("github.com") {
        return None;
    }
    // /-/tree/ref/subpath
    if input.contains("/-/tree/") {
        let (base, rest) = input.split_once("/-/tree/")?;
        let (ref_, subpath) = rest.split_once('/').unwrap_or((rest, ""));
        let normalized = format!("{}.git", base.trim_end_matches(".git"));
        let slug = if !subpath.is_empty() {
            subpath
                .split('/')
                .next_back()
                .map(slug_from_name)
                .unwrap_or_else(|| slug_from_name(base))
        } else {
            base.split('/')
                .next_back()
                .map(slug_from_name)
                .unwrap_or_else(|| slug_from_name(base))
        };
        return Some((
            SkillSourceKind::GitLab {
                url: normalized,
                ref_: Some(ref_.to_string()),
                subpath: if subpath.is_empty() {
                    None
                } else {
                    Some(subpath.to_string())
                },
            },
            slug,
        ));
    }
    // Plain gitlab URL: detect gitlab.com or common self-hosted
    if input.contains("gitlab.com") {
        let after = input.split("gitlab.com/").nth(1)?;
        let path = after.trim_end_matches(".git").trim_end_matches('/');
        if path.contains('/') {
            let slug = path
                .split('/')
                .next_back()
                .map(slug_from_name)
                .unwrap_or_else(|| slug_from_name(path));
            let normalized = format!("https://gitlab.com/{}.git", path);
            return Some((
                SkillSourceKind::GitLab {
                    url: normalized,
                    ref_: None,
                    subpath: None,
                },
                slug,
            ));
        }
    }
    None
}

/// Try to parse GitHub shorthand: `owner/repo`, `owner/repo/subpath`, `owner/repo@filter`
fn try_parse_github_shorthand(input: &str) -> Option<(SkillSourceKind, String)> {
    // owner/repo@filter
    if let Some((prefix, filter)) = input.split_once('@') {
        let mut parts = prefix.splitn(2, '/');
        let owner = parts.next()?;
        let repo = parts.next()?.trim_end_matches(".git");
        if owner.is_empty() || repo.is_empty() {
            return None;
        }
        let slug = slug_from_name(filter);
        return Some((
            SkillSourceKind::GitHub {
                owner: owner.to_string(),
                repo: repo.to_string(),
                ref_: None,
                subpath: None,
                skill_filter: Some(filter.to_string()),
            },
            slug,
        ));
    }
    // owner/repo or owner/repo/subpath
    let mut parts = input.splitn(3, '/');
    let owner = parts.next()?;
    let repo_raw = parts.next()?;
    let subpath = parts.next();
    let repo = repo_raw.trim_end_matches(".git");
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let slug = if let Some(sp) = subpath {
        sp.split('/')
            .next_back()
            .map(slug_from_name)
            .unwrap_or_else(|| slug_from_name(repo))
    } else {
        slug_from_name(repo)
    };
    Some((
        SkillSourceKind::GitHub {
            owner: owner.to_string(),
            repo: repo.to_string(),
            ref_: None,
            subpath: subpath.map(|s| s.to_string()),
            skill_filter: None,
        },
        slug,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parse(s: &str) -> (SkillSourceKind, String) {
        parse_skill_source(s, Path::new("/tmp/field-dir"))
            .unwrap_or_else(|e| panic!("parse_skill_source({:?}) failed: {}", s, e))
    }

    fn parse_err(s: &str) -> String {
        parse_skill_source(s, Path::new("/tmp/field-dir"))
            .expect_err(&format!("expected error for {:?}", s))
    }

    // -----------------------------------------------------------------------
    // GitHub shorthand
    // -----------------------------------------------------------------------

    #[test]
    fn github_shorthand_owner_repo() {
        let (kind, slug) = parse("anthropics/my-skill");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "anthropics".into(),
                repo: "my-skill".into(),
                ref_: None,
                subpath: None,
                skill_filter: None,
            }
        );
        assert_eq!(slug, "my-skill");
    }

    #[test]
    fn github_shorthand_with_subpath() {
        let (kind, slug) = parse("owner/repo/skills/formatter");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "owner".into(),
                repo: "repo".into(),
                ref_: None,
                subpath: Some("skills/formatter".into()),
                skill_filter: None,
            }
        );
        assert_eq!(slug, "formatter");
    }

    #[test]
    fn github_shorthand_with_skill_filter() {
        let (kind, slug) = parse("owner/repo@my-formatter");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "owner".into(),
                repo: "repo".into(),
                ref_: None,
                subpath: None,
                skill_filter: Some("my-formatter".into()),
            }
        );
        assert_eq!(slug, "my-formatter");
    }

    #[test]
    fn github_prefix_strips_and_reparses() {
        let (kind_prefix, slug_prefix) = parse("github:owner/repo");
        let (kind_bare, slug_bare) = parse("owner/repo");
        assert_eq!(kind_prefix, kind_bare);
        assert_eq!(slug_prefix, slug_bare);
    }

    // -----------------------------------------------------------------------
    // GitHub URLs
    // -----------------------------------------------------------------------

    #[test]
    fn github_full_url() {
        let (kind, slug) = parse("https://github.com/anthropics/claude-skills");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "anthropics".into(),
                repo: "claude-skills".into(),
                ref_: None,
                subpath: None,
                skill_filter: None,
            }
        );
        assert_eq!(slug, "claude-skills");
    }

    #[test]
    fn github_url_with_tree_and_path() {
        let (kind, slug) = parse("https://github.com/owner/skills-repo/tree/main/skills/my-skill");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "owner".into(),
                repo: "skills-repo".into(),
                ref_: Some("main".into()),
                subpath: Some("skills/my-skill".into()),
                skill_filter: None,
            }
        );
        assert_eq!(slug, "my-skill");
    }

    #[test]
    fn github_url_with_tree_no_path() {
        let (kind, slug) = parse("https://github.com/owner/repo/tree/develop");
        assert_eq!(
            kind,
            SkillSourceKind::GitHub {
                owner: "owner".into(),
                repo: "repo".into(),
                ref_: Some("develop".into()),
                subpath: None,
                skill_filter: None,
            }
        );
        assert_eq!(slug, "repo");
    }

    // -----------------------------------------------------------------------
    // GitLab
    // -----------------------------------------------------------------------

    #[test]
    fn gitlab_prefix_routes_to_gitlab_com() {
        let (kind, _) = parse("gitlab:owner/repo");
        match kind {
            SkillSourceKind::GitLab { url, .. } => {
                assert!(url.contains("gitlab.com/owner/repo"), "url was: {}", url);
            }
            other => panic!("expected GitLab, got {:?}", other),
        }
    }

    #[test]
    fn gitlab_url_with_tree_and_path() {
        let (kind, slug) = parse("https://gitlab.com/owner/repo/-/tree/main/skills/tool");
        match kind {
            SkillSourceKind::GitLab { url, ref_, subpath } => {
                assert!(url.contains("gitlab.com/owner/repo"));
                assert_eq!(ref_, Some("main".into()));
                assert_eq!(subpath, Some("skills/tool".into()));
            }
            other => panic!("expected GitLab, got {:?}", other),
        }
        assert_eq!(slug, "tool");
    }

    // -----------------------------------------------------------------------
    // ClawHub
    // -----------------------------------------------------------------------

    #[test]
    fn clawhub_name_only() {
        let (kind, slug) = parse("clawhub:my-skill");
        assert_eq!(
            kind,
            SkillSourceKind::ClawHub {
                org: None,
                name: "my-skill".into(),
            }
        );
        assert_eq!(slug, "my-skill");
    }

    #[test]
    fn clawhub_org_and_name() {
        let (kind, slug) = parse("clawhub:portlang/formatter");
        assert_eq!(
            kind,
            SkillSourceKind::ClawHub {
                org: Some("portlang".into()),
                name: "formatter".into(),
            }
        );
        assert_eq!(slug, "formatter");
    }

    // -----------------------------------------------------------------------
    // Well-known
    // -----------------------------------------------------------------------

    #[test]
    fn well_known_https_url() {
        let (kind, slug) = parse("https://skills.example.com");
        assert_eq!(
            kind,
            SkillSourceKind::WellKnown {
                url: "https://skills.example.com".into(),
            }
        );
        assert_eq!(slug, "skills-example-com");
    }

    // -----------------------------------------------------------------------
    // SSH git URL
    // -----------------------------------------------------------------------

    #[test]
    fn ssh_git_url() {
        let (kind, slug) = parse("git@github.com:owner/my-skills.git");
        assert_eq!(
            kind,
            SkillSourceKind::Git {
                url: "git@github.com:owner/my-skills.git".into(),
            }
        );
        assert_eq!(slug, "my-skills");
    }

    // -----------------------------------------------------------------------
    // Local paths
    // -----------------------------------------------------------------------

    #[test]
    fn local_relative_path() {
        let (kind, slug) = parse("./my-skill.md");
        match kind {
            SkillSourceKind::Local { path } => {
                assert!(path.ends_with("my-skill.md"), "path was: {:?}", path);
                // resolved against config_dir — should be absolute
                assert!(path.is_absolute(), "local path should be absolute");
            }
            other => panic!("expected Local, got {:?}", other),
        }
        assert_eq!(slug, "my-skill");
    }

    #[test]
    fn local_absolute_path() {
        let (kind, _slug) = parse("/absolute/path/to/skill.md");
        match kind {
            SkillSourceKind::Local { path } => {
                assert_eq!(path, std::path::PathBuf::from("/absolute/path/to/skill.md"));
            }
            other => panic!("expected Local, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // Slug normalisation
    // -----------------------------------------------------------------------

    #[test]
    fn slug_lowercases_and_replaces_special_chars() {
        let (_, slug) = parse("owner/My_Cool.Skill");
        assert_eq!(slug, "my-cool-skill");
    }

    #[test]
    fn slug_collapses_multiple_hyphens() {
        let (_, slug) = parse("clawhub:my--double--skill");
        assert_eq!(slug, "my-double-skill");
    }

    // -----------------------------------------------------------------------
    // Error cases
    // -----------------------------------------------------------------------

    #[test]
    fn empty_string_is_error() {
        let err = parse_err("");
        assert!(!err.is_empty());
    }

    #[test]
    fn bare_hostname_without_slash_is_error() {
        // "justarepo" with no slash is not a valid owner/repo
        let err = parse_err("justarepo");
        assert!(!err.is_empty());
    }

    // -----------------------------------------------------------------------
    // Spec compliance: source format portability
    // -----------------------------------------------------------------------

    /// Spec: `github:owner/repo` is exactly equivalent to `owner/repo` shorthand.
    #[test]
    fn spec_github_prefix_is_portable_alias_for_shorthand() {
        let (kind_prefix, slug_prefix) = parse("github:portofcontext/my-skill");
        let (kind_bare, slug_bare) = parse("portofcontext/my-skill");
        assert_eq!(
            kind_prefix, kind_bare,
            "github: prefix must produce identical result to bare shorthand"
        );
        assert_eq!(slug_prefix, slug_bare);
    }

    /// Spec: `gitlab:owner/repo` routes to gitlab.com.
    #[test]
    fn spec_gitlab_prefix_routes_to_gitlab_com() {
        let (kind, _) = parse("gitlab:owner/repo");
        match kind {
            SkillSourceKind::GitLab { ref url, .. } => {
                assert!(
                    url.contains("gitlab.com"),
                    "gitlab: prefix must resolve to gitlab.com, got: {url}"
                );
            }
            other => panic!("expected GitLab, got {other:?}"),
        }
    }

    /// Spec: `clawhub:name` → ClawHub registry with org=None.
    #[test]
    fn spec_clawhub_name_only_format() {
        let (kind, slug) = parse("clawhub:pdf-skill");
        assert_eq!(
            kind,
            SkillSourceKind::ClawHub {
                org: None,
                name: "pdf-skill".into(),
            }
        );
        assert_eq!(slug, "pdf-skill");
    }

    /// Spec: `clawhub:org/name` → ClawHub registry with org set.
    #[test]
    fn spec_clawhub_org_slash_name_format() {
        let (kind, slug) = parse("clawhub:acme/formatter");
        assert_eq!(
            kind,
            SkillSourceKind::ClawHub {
                org: Some("acme".into()),
                name: "formatter".into(),
            }
        );
        assert_eq!(slug, "formatter");
    }

    /// Spec: `./relative.md` is a local path resolved against the field's directory.
    #[test]
    fn spec_local_relative_path_resolved_to_absolute() {
        let (kind, _) = parse("./my-skill.md");
        match kind {
            SkillSourceKind::Local { ref path } => {
                assert!(
                    path.is_absolute(),
                    "local path must be resolved to absolute: {path:?}"
                );
            }
            other => panic!("expected Local, got {other:?}"),
        }
    }

    /// Spec: full GitHub URL with `/tree/<ref>/<subpath>` is parsed correctly.
    #[test]
    fn spec_github_full_url_with_tree_ref_and_subpath() {
        let (kind, slug) = parse("https://github.com/owner/skills-repo/tree/main/skills/my-skill");
        match kind {
            SkillSourceKind::GitHub {
                ref owner,
                ref repo,
                ref ref_,
                ref subpath,
                ..
            } => {
                assert_eq!(owner, "owner");
                assert_eq!(repo, "skills-repo");
                assert_eq!(ref_.as_deref(), Some("main"));
                assert_eq!(subpath.as_deref(), Some("skills/my-skill"));
            }
            other => panic!("expected GitHub, got {other:?}"),
        }
        assert_eq!(slug, "my-skill");
    }

    /// Spec: the `@skill-name` filter syntax selects a specific skill within a repo.
    #[test]
    fn spec_github_at_filter_selects_skill_within_repo() {
        let (kind, slug) = parse("owner/multi-skills@pdf-skill");
        match kind {
            SkillSourceKind::GitHub {
                ref skill_filter, ..
            } => {
                assert_eq!(
                    skill_filter.as_deref(),
                    Some("pdf-skill"),
                    "skill_filter must be set from @filter syntax"
                );
            }
            other => panic!("expected GitHub, got {other:?}"),
        }
        assert_eq!(slug, "pdf-skill");
    }

    /// Spec: HTTP(S) URL that is not GitHub/GitLab → WellKnown source
    /// (fetches `/.well-known/skills/index.json`).
    #[test]
    fn spec_https_non_github_url_is_well_known() {
        let (kind, _) = parse("https://skills.example.com");
        assert!(
            matches!(kind, SkillSourceKind::WellKnown { .. }),
            "generic HTTPS URL must resolve to WellKnown, got {kind:?}"
        );
    }

    /// Spec: `git@host:owner/repo.git` SSH URL → Git source kind.
    #[test]
    fn spec_ssh_git_url_is_git_source() {
        let (kind, _) = parse("git@github.com:owner/repo.git");
        assert!(
            matches!(kind, SkillSourceKind::Git { .. }),
            "SSH git URL must resolve to Git source, got {kind:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Spec compliance: slug normalisation
    // -----------------------------------------------------------------------

    /// Spec: slug derivation must produce lowercase-hyphen strings compatible
    /// with the name field constraints (no uppercase, no consecutive hyphens,
    /// no leading/trailing hyphen).
    #[test]
    fn spec_slug_is_always_lowercase() {
        let (_, slug) = parse("owner/MyCoolSkill");
        assert_eq!(slug, slug.to_lowercase(), "slug must be lowercase");
    }

    #[test]
    fn spec_slug_collapses_consecutive_hyphens() {
        let (_, slug) = parse("clawhub:my--double--skill");
        assert!(
            !slug.contains("--"),
            "slug must not contain consecutive hyphens: {slug:?}"
        );
    }

    #[test]
    fn spec_slug_no_leading_or_trailing_hyphen() {
        for source in &["owner/_leading-underscore", "clawhub:trailing-"] {
            let result = parse_skill_source(source, Path::new("/tmp"));
            if let Ok((_, slug)) = result {
                assert!(
                    !slug.starts_with('-') && !slug.ends_with('-'),
                    "slug derived from {source:?} must not start/end with hyphen: {slug:?}"
                );
            }
            // It's also acceptable for these to return an error
        }
    }
}
