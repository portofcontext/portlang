use anyhow::{Context, Result};
use portlang_core::{Skill, SkillSourceKind};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// Resolves skill content from all supported source types.
///
/// Skills are fetched and their SKILL.md content is stored in `skill.content`.
/// Remote skills are cached at `~/.portlang/skills_cache/` with a 24-hour TTL.
/// On fetch failure, a stale cache entry is used if available.
pub struct SkillResolver {
    cache_dir: PathBuf,
    http: reqwest::Client,
}

impl Default for SkillResolver {
    fn default() -> Self {
        let cache_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".portlang")
            .join("skills_cache");
        Self {
            cache_dir,
            http: reqwest::Client::new(),
        }
    }
}

impl SkillResolver {
    /// Resolve content for all skills in place. Logs warnings for failed
    /// resolutions but does not return an error — the run continues without
    /// the unresolved skill.
    pub async fn resolve_all(&self, skills: &mut [Skill]) -> Result<()> {
        for skill in skills.iter_mut() {
            if let Err(e) = self.resolve_one(skill).await {
                tracing::warn!(
                    "Failed to resolve skill {:?} (source: {}): {}",
                    skill.slug,
                    skill.source,
                    e
                );
            }
        }
        Ok(())
    }

    /// Resolve content for a single skill, populating `skill.content`.
    pub async fn resolve_one(&self, skill: &mut Skill) -> Result<()> {
        let content = match &skill.kind {
            SkillSourceKind::Local { path } => {
                if path.is_dir() {
                    // Directory-based skill: read SKILL.md inside it and scan resources.
                    let skill_md = path.join("SKILL.md");
                    if !skill_md.exists() {
                        anyhow::bail!("Skill directory contains no SKILL.md: {}", path.display());
                    }
                    let content = tokio::fs::read_to_string(&skill_md)
                        .await
                        .with_context(|| format!("Failed to read {}", skill_md.display()))?;
                    skill.resources = scan_skill_resources(path).await;
                    skill.content = Some(content);
                    return Ok(());
                } else {
                    tokio::fs::read_to_string(path).await.with_context(|| {
                        format!("Failed to read local skill at {}", path.display())
                    })?
                }
            }

            SkillSourceKind::GitHub {
                owner,
                repo,
                ref_,
                subpath,
                skill_filter,
            } => {
                let cache_key = github_cache_key(owner, repo, ref_.as_deref(), subpath.as_deref());
                self.fetch_with_cache(&cache_key, || {
                    let url = github_raw_url(
                        owner,
                        repo,
                        ref_.as_deref().unwrap_or("HEAD"),
                        subpath.as_deref(),
                    );
                    let http = self.http.clone();
                    let filter = skill_filter.clone();
                    Box::pin(async move {
                        fetch_skill_content_from_url(&http, &url, filter.as_deref()).await
                    })
                })
                .await?
            }

            SkillSourceKind::GitLab { url, ref_, subpath } => {
                let cache_key = format!(
                    "gitlab/{}",
                    sanitize_cache_key(&format!(
                        "{}-{}-{}",
                        url,
                        ref_.as_deref().unwrap_or("HEAD"),
                        subpath.as_deref().unwrap_or("")
                    ))
                );
                let raw_url =
                    gitlab_raw_url(url, ref_.as_deref().unwrap_or("HEAD"), subpath.as_deref());
                let http = self.http.clone();
                self.fetch_with_cache(&cache_key, || {
                    Box::pin(
                        async move { fetch_skill_content_from_url(&http, &raw_url, None).await },
                    )
                })
                .await?
            }

            SkillSourceKind::WellKnown { url } => {
                let cache_key = format!("wellknown/{}", sanitize_cache_key(url));
                let index_url = format!(
                    "{}/.well-known/skills/index.json",
                    url.trim_end_matches('/')
                );
                let http = self.http.clone();
                self.fetch_with_cache(&cache_key, || {
                    Box::pin(async move { fetch_well_known_skill(&http, &index_url).await })
                })
                .await?
            }

            SkillSourceKind::ClawHub { org, name } => {
                // ClawHub URL shape TBD — placeholder until confirmed
                let cache_key = format!("clawhub/{}-{}", org.as_deref().unwrap_or("_"), name);
                let url = clawhub_url(org.as_deref(), name);
                let http = self.http.clone();
                self.fetch_with_cache(&cache_key, || {
                    Box::pin(async move { fetch_skill_content_from_url(&http, &url, None).await })
                })
                .await?
            }

            SkillSourceKind::Git { url } => {
                anyhow::bail!(
                    "Git SSH/URL sources require sparse clone (not yet implemented). \
                     Use a GitHub or GitLab URL instead: {}",
                    url
                )
            }
        };

        skill.content = Some(content);
        Ok(())
    }

    /// Fetch from network, using cache with 24h TTL.
    /// Falls back to stale cache on error.
    async fn fetch_with_cache<F, Fut>(&self, key: &str, fetch: F) -> Result<String>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<String>>,
    {
        let cache_path = self.cache_path(key);

        // Check if fresh cache exists
        if let Some(cached) = self.read_cache(&cache_path) {
            return Ok(cached);
        }

        // Fetch from network
        match fetch().await {
            Ok(content) => {
                self.write_cache(&cache_path, &content);
                Ok(content)
            }
            Err(e) => {
                // Network failed — try stale cache as fallback
                if let Ok(stale) = tokio::fs::read_to_string(&cache_path).await {
                    tracing::warn!("Using stale cache for skill {:?}: {}", key, e);
                    return Ok(stale);
                }
                Err(e)
            }
        }
    }

    fn cache_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.md", key))
    }

    fn read_cache(&self, path: &PathBuf) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let modified = meta.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;
        if age > Duration::from_secs(86400) {
            return None; // Stale
        }
        std::fs::read_to_string(path).ok()
    }

    fn write_cache(&self, path: &PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(path, content);
    }
}

// --- Skill resource scanner ---

/// Scan a local skill directory for bundled resource files in `scripts/`,
/// `references/`, and `assets/` subdirectories.
/// Returns sorted relative paths like `"scripts/extract.py"`.
async fn scan_skill_resources(skill_dir: &std::path::Path) -> Vec<String> {
    let mut resources = Vec::new();
    for subdir in &["scripts", "references", "assets"] {
        let dir = skill_dir.join(subdir);
        let Ok(mut entries) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if matches!(entry.file_type().await, Ok(ft) if ft.is_file()) {
                resources.push(format!(
                    "{}/{}",
                    subdir,
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }
    resources.sort();
    resources
}

// --- URL builders ---

fn github_raw_url(owner: &str, repo: &str, ref_: &str, subpath: Option<&str>) -> String {
    let path = subpath
        .map(|sp| format!("{}/SKILL.md", sp.trim_end_matches('/')))
        .unwrap_or_else(|| "SKILL.md".to_string());
    format!(
        "https://raw.githubusercontent.com/{}/{}/{}/{}",
        owner, repo, ref_, path
    )
}

fn gitlab_raw_url(repo_url: &str, ref_: &str, subpath: Option<&str>) -> String {
    // repo_url is like https://gitlab.com/owner/repo.git
    let base = repo_url.trim_end_matches(".git");
    let path = subpath.unwrap_or("");
    if path.is_empty() {
        format!("{}/-/raw/{}/SKILL.md", base, ref_)
    } else {
        format!("{}/-/raw/{}/{}/SKILL.md", base, ref_, path)
    }
}

fn clawhub_url(org: Option<&str>, name: &str) -> String {
    // URL shape TBD — this is a placeholder
    match org {
        Some(o) => format!("https://clawhub.dev/skills/{}/{}/SKILL.md", o, name),
        None => format!("https://clawhub.dev/skills/{}/SKILL.md", name),
    }
}

// --- Fetch helpers ---

/// Fetch SKILL.md content from a direct URL.
async fn fetch_skill_content_from_url(
    http: &reqwest::Client,
    url: &str,
    _skill_filter: Option<&str>,
) -> Result<String> {
    let resp = http
        .get(url)
        .header("User-Agent", "portlang-skills/0.1")
        .send()
        .await
        .with_context(|| format!("HTTP request failed for {}", url))?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching skill from {}", resp.status(), url);
    }

    resp.text()
        .await
        .with_context(|| format!("Failed to read response body from {}", url))
}

/// Fetch from a well-known skills index and return all skill content concatenated.
async fn fetch_well_known_skill(http: &reqwest::Client, index_url: &str) -> Result<String> {
    let resp = http
        .get(index_url)
        .header("User-Agent", "portlang-skills/0.1")
        .send()
        .await
        .with_context(|| format!("HTTP request failed for {}", index_url))?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "HTTP {} fetching well-known index from {}",
            resp.status(),
            index_url
        );
    }

    let index: serde_json::Value = resp
        .json()
        .await
        .with_context(|| format!("Failed to parse JSON from {}", index_url))?;

    // index.json shape: { "skills": [{ "name": "...", "url": "..." }] }
    let base = index_url
        .trim_end_matches("/.well-known/skills/index.json")
        .trim_end_matches('/');

    let mut parts: Vec<String> = Vec::new();
    if let Some(skills) = index.get("skills").and_then(|s| s.as_array()) {
        for entry in skills {
            let skill_url = entry
                .get("url")
                .and_then(|u| u.as_str())
                .map(|u| {
                    if u.starts_with("http") {
                        u.to_string()
                    } else {
                        format!("{}/{}", base, u.trim_start_matches('/'))
                    }
                })
                .unwrap_or_default();

            if skill_url.is_empty() {
                continue;
            }

            match fetch_skill_content_from_url(http, &skill_url, None).await {
                Ok(content) => parts.push(content),
                Err(e) => tracing::warn!("Failed to fetch well-known skill {}: {}", skill_url, e),
            }
        }
    }

    if parts.is_empty() {
        anyhow::bail!("No skills found in well-known index at {}", index_url);
    }

    Ok(parts.join("\n\n---\n\n"))
}

// --- Cache key helpers ---

fn github_cache_key(owner: &str, repo: &str, ref_: Option<&str>, subpath: Option<&str>) -> String {
    format!(
        "github/{}/{}/{}/{}",
        owner,
        repo,
        ref_.unwrap_or("HEAD"),
        sanitize_cache_key(subpath.unwrap_or("root"))
    )
}

fn sanitize_cache_key(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use portlang_core::{Skill, SkillSourceKind};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn local_skill(path: PathBuf, slug: &str) -> Skill {
        Skill {
            source: path.to_string_lossy().to_string(),
            kind: SkillSourceKind::Local { path },
            slug: slug.to_string(),
            content: None,
            resources: Vec::new(),
        }
    }

    fn resolver_with_cache(cache_dir: &std::path::Path) -> SkillResolver {
        SkillResolver {
            cache_dir: cache_dir.to_path_buf(),
            http: reqwest::Client::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Local directory resolution (spec: skills are directories)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn resolve_local_directory_skill_reads_skill_md() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Does stuff.\n---\n\n# My Skill",
        )
        .unwrap();

        let cache_dir = dir.path().join("cache");
        let resolver = resolver_with_cache(&cache_dir);
        let mut skill = local_skill(skill_dir.clone(), "my-skill");

        resolver.resolve_one(&mut skill).await.unwrap();

        assert!(skill.content.as_deref().unwrap().contains("# My Skill"));
    }

    #[tokio::test]
    async fn resolve_local_directory_skill_scans_resources() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("my-skill");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: D.\n---\n",
        )
        .unwrap();
        let scripts = skill_dir.join("scripts");
        std::fs::create_dir(&scripts).unwrap();
        std::fs::write(scripts.join("run.sh"), "#!/bin/bash").unwrap();
        std::fs::write(scripts.join("helper.py"), "# helper").unwrap();
        let refs = skill_dir.join("references");
        std::fs::create_dir(&refs).unwrap();
        std::fs::write(refs.join("guide.md"), "# Guide").unwrap();

        let cache_dir = dir.path().join("cache");
        let resolver = resolver_with_cache(&cache_dir);
        let mut skill = local_skill(skill_dir.clone(), "my-skill");

        resolver.resolve_one(&mut skill).await.unwrap();

        assert!(skill.resources.contains(&"scripts/helper.py".to_string()));
        assert!(skill.resources.contains(&"scripts/run.sh".to_string()));
        assert!(skill.resources.contains(&"references/guide.md".to_string()));
        assert!(skill.resources.is_sorted(), "resources must be sorted");
    }

    #[tokio::test]
    async fn resolve_local_directory_without_skill_md_returns_error() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path().join("empty-skill");
        std::fs::create_dir(&skill_dir).unwrap();

        let resolver = resolver_with_cache(dir.path());
        let mut skill = local_skill(skill_dir, "empty-skill");

        let result = resolver.resolve_one(&mut skill).await;
        assert!(
            result.is_err(),
            "directory with no SKILL.md must return error"
        );
    }

    #[tokio::test]
    async fn resolve_local_flat_file_skill_has_no_resources() {
        let dir = TempDir::new().unwrap();
        let skill_path = dir.path().join("my-skill.md");
        std::fs::write(&skill_path, "---\nname: my-skill\ndescription: D.\n---\n").unwrap();

        let resolver = resolver_with_cache(dir.path());
        let mut skill = local_skill(skill_path, "my-skill");

        resolver.resolve_one(&mut skill).await.unwrap();

        assert!(
            skill.resources.is_empty(),
            "flat-file skills have no bundled resources"
        );
    }

    // -----------------------------------------------------------------------
    // Local file resolution
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn resolve_local_skill_reads_file_content() {
        let dir = TempDir::new().unwrap();
        let skill_path = dir.path().join("my-skill.md");
        std::fs::write(
            &skill_path,
            "---\nname: my-skill\ndescription: Does useful stuff\n---\n\n# My Skill\n\nInstructions here.",
        )
        .unwrap();

        let cache_dir = dir.path().join("cache");
        let resolver = resolver_with_cache(&cache_dir);
        let mut skill = local_skill(skill_path, "my-skill");

        resolver.resolve_one(&mut skill).await.unwrap();

        let content = skill.content.as_deref().unwrap();
        assert!(
            content.contains("# My Skill"),
            "content should contain skill body"
        );
        assert!(
            content.contains("Instructions here."),
            "content should contain instructions"
        );
    }

    #[tokio::test]
    async fn resolve_missing_local_file_returns_error() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist.md");
        let resolver = resolver_with_cache(dir.path());
        let mut skill = local_skill(missing, "gone");

        let result = resolver.resolve_one(&mut skill).await;
        assert!(result.is_err(), "missing local file should return an error");
    }

    #[tokio::test]
    async fn resolve_all_continues_on_missing_file() {
        // resolve_all logs warnings but does NOT propagate errors
        let dir = TempDir::new().unwrap();
        let good_path = dir.path().join("good.md");
        let bad_path = dir.path().join("missing.md");
        std::fs::write(&good_path, "# Good Skill\nContent.").unwrap();

        let cache_dir = dir.path().join("cache");
        let resolver = resolver_with_cache(&cache_dir);
        let mut skills = vec![
            local_skill(good_path, "good"),
            local_skill(bad_path, "missing"),
        ];

        resolver.resolve_all(&mut skills).await.unwrap(); // must not return Err

        assert!(skills[0].content.is_some(), "good skill should be resolved");
        assert!(
            skills[1].content.is_none(),
            "missing skill should remain unresolved"
        );
    }

    #[tokio::test]
    async fn resolve_all_with_empty_list_is_noop() {
        let dir = TempDir::new().unwrap();
        let resolver = resolver_with_cache(dir.path());
        let mut skills: Vec<Skill> = vec![];
        resolver.resolve_all(&mut skills).await.unwrap();
        assert!(skills.is_empty());
    }

    // -----------------------------------------------------------------------
    // Caching
    // -----------------------------------------------------------------------

    #[test]
    fn fresh_cache_is_returned() {
        let dir = TempDir::new().unwrap();
        let resolver = resolver_with_cache(dir.path());
        let cache_path = resolver.cache_path("test/key");

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&cache_path, "cached content").unwrap();

        let result = resolver.read_cache(&cache_path);
        assert_eq!(result.as_deref(), Some("cached content"));
    }

    #[test]
    fn missing_cache_returns_none() {
        let dir = TempDir::new().unwrap();
        let resolver = resolver_with_cache(dir.path());
        let cache_path = resolver.cache_path("nonexistent/key");
        assert!(resolver.read_cache(&cache_path).is_none());
    }

    #[test]
    fn cache_write_creates_parent_dirs() {
        let dir = TempDir::new().unwrap();
        let resolver = resolver_with_cache(dir.path());
        let cache_path = resolver.cache_path("deep/nested/key");

        assert!(!cache_path.parent().unwrap().exists());

        resolver.write_cache(&cache_path, "hello");

        assert!(cache_path.exists(), "cache file should be written");
        assert_eq!(std::fs::read_to_string(&cache_path).unwrap(), "hello");
    }

    // -----------------------------------------------------------------------
    // Cache key helpers
    // -----------------------------------------------------------------------

    #[test]
    fn github_cache_key_no_ref_no_subpath() {
        let key = github_cache_key("owner", "repo", None, None);
        assert_eq!(key, "github/owner/repo/HEAD/root");
    }

    #[test]
    fn github_cache_key_with_ref_and_subpath() {
        let key = github_cache_key("owner", "repo", Some("main"), Some("skills/my-skill"));
        assert_eq!(key, "github/owner/repo/main/skills_my-skill");
    }

    #[test]
    fn sanitize_cache_key_replaces_slashes_and_dots() {
        let s = sanitize_cache_key("https://example.com/path.json");
        assert!(!s.contains('/'), "slashes should be replaced");
        assert!(!s.contains('.'), "dots should be replaced");
        assert!(!s.contains(':'), "colons should be replaced");
    }

    // -----------------------------------------------------------------------
    // URL builders
    // -----------------------------------------------------------------------

    #[test]
    fn github_raw_url_no_subpath() {
        let url = github_raw_url("anthropics", "my-skill", "HEAD", None);
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/anthropics/my-skill/HEAD/SKILL.md"
        );
    }

    #[test]
    fn github_raw_url_with_subpath() {
        let url = github_raw_url("owner", "repo", "main", Some("skills/formatter"));
        assert_eq!(
            url,
            "https://raw.githubusercontent.com/owner/repo/main/skills/formatter/SKILL.md"
        );
    }

    #[test]
    fn gitlab_raw_url_no_subpath() {
        let url = gitlab_raw_url("https://gitlab.com/owner/repo.git", "main", None);
        assert_eq!(url, "https://gitlab.com/owner/repo/-/raw/main/SKILL.md");
    }

    #[test]
    fn clawhub_url_name_only() {
        let url = clawhub_url(None, "formatter");
        assert!(url.contains("formatter"), "URL should contain skill name");
    }

    #[test]
    fn clawhub_url_with_org() {
        let url = clawhub_url(Some("portlang"), "formatter");
        assert!(url.contains("portlang"), "URL should contain org");
        assert!(url.contains("formatter"), "URL should contain skill name");
    }
}
