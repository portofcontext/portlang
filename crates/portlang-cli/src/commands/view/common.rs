use anyhow::Result;
use std::fs;
use std::path::PathBuf;

/// Generate the HTML head with title and inline CSS
pub fn render_head(title: &str) -> String {
    format!(
        r#"<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
{}
    </style>
</head>"#,
        title,
        get_css()
    )
}

/// Get the inline CSS with brand colors and typography
fn get_css() -> &'static str {
    r#"
:root {
  --primary: #002B56;
  --secondary: #184289;
  --tertiary: #1E6969;
  --box-bg: #AFDFFF;
  --dashed-line: #3C5683;
  --header-text: #012E58;
  --body-text: #012E58;
  --white: #FFFFFF;
  --light-gray: #F5F5F5;
  --error-red: #C41E3A;
}

* {
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
  color: var(--body-text);
  background: var(--white);
  margin: 0;
  padding: 2rem;
  line-height: 1.6;
}

h1, h2, h3 {
  color: var(--header-text);
  font-weight: 600;
  margin-top: 0;
}

h1 {
  font-size: 2rem;
  border-bottom: 2px solid var(--secondary);
  padding-bottom: 0.5rem;
  margin-bottom: 1.5rem;
}

h2 {
  font-size: 1.5rem;
  margin-bottom: 1rem;
}

h3 {
  font-size: 1.25rem;
  margin-bottom: 0.75rem;
}

code, pre, .mono {
  font-family: "IBM Plex Mono", monospace;
}

pre {
  background: #FAFAFA;
  padding: 1rem;
  overflow-x: auto;
  max-height: 500px;
  overflow-y: auto;
}

.container {
  max-width: 1200px;
  margin: 0 auto;
}

.section {
  padding: 1.5rem 0;
  margin: 1.5rem 0;
  border-bottom: 1px solid #E5E5E5;
}

.section:last-child {
  border-bottom: none;
}

.header-info {
  display: flex;
  flex-wrap: wrap;
  gap: 1.5rem;
  margin-bottom: 1rem;
}

.info-item {
  display: flex;
  flex-direction: column;
}

.info-label {
  font-size: 0.875rem;
  color: var(--secondary);
  font-weight: 500;
  margin-bottom: 0.25rem;
}

.info-value {
  font-size: 1.125rem;
  font-weight: 600;
  font-family: "IBM Plex Mono", monospace;
}

.converged {
  color: var(--tertiary);
}

.failed {
  color: var(--error-red);
}

.status-badge {
  display: inline-block;
  padding: 0.25rem 0.75rem;
  border-radius: 4px;
  font-size: 0.875rem;
  font-weight: 500;
}

.status-badge.converged {
  background: var(--tertiary);
  color: var(--white);
}

.status-badge.failed {
  background: var(--error-red);
  color: var(--white);
}

.navigation {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin: 2rem 0;
  padding: 1rem 0;
  border-top: 1px solid #E5E5E5;
  border-bottom: 1px solid #E5E5E5;
}

.nav-buttons {
  display: flex;
  gap: 0.5rem;
}

button {
  background: transparent;
  color: var(--primary);
  border: 1px solid var(--primary);
  padding: 0.5rem 1rem;
  cursor: pointer;
  font-size: 0.875rem;
  font-weight: 500;
  transition: all 0.2s;
}

button:hover:not(:disabled) {
  background: var(--primary);
  color: var(--white);
}

button:disabled {
  border-color: #D0D0D0;
  color: #D0D0D0;
  cursor: not-allowed;
}

.step-content {
  margin: 1.5rem 0;
}

.step-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.5rem;
  padding-bottom: 1rem;
  border-bottom: 2px solid #E5E5E5;
}

.step-title {
  font-size: 1.25rem;
  font-weight: 600;
  color: var(--header-text);
}

.step-meta {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.875rem;
  color: var(--secondary);
}

.json-container {
  background: #FAFAFA;
  border-left: 3px solid var(--secondary);
  padding: 1rem;
  margin: 1rem 0;
}

.json-label {
  font-weight: 600;
  color: var(--secondary);
  margin-bottom: 0.5rem;
  font-size: 0.875rem;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.json-content {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.875rem;
  white-space: pre-wrap;
  word-break: break-word;
}

.verifier-results {
  margin-top: 1rem;
}

.verifier-item {
  padding: 0.75rem 0;
  margin: 0.5rem 0;
  border-bottom: 1px solid #F0F0F0;
}

.verifier-item:last-child {
  border-bottom: none;
}

.verifier-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.5rem;
}

.verifier-name {
  font-weight: 600;
  font-family: "IBM Plex Mono", monospace;
}

.verifier-status {
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  font-size: 0.75rem;
  font-weight: 600;
}

.verifier-status.passed {
  background: var(--tertiary);
  color: var(--white);
}

.verifier-status.failed {
  background: var(--error-red);
  color: var(--white);
}

.verifier-output {
  font-family: "IBM Plex Mono", monospace;
  font-size: 0.75rem;
  color: #666;
  margin-top: 0.5rem;
  padding: 0.5rem 0;
}

.timeline {
  margin: 2rem 0 1rem 0;
  padding: 1rem 0;
}

.timeline-track {
  position: relative;
  height: 40px;
  display: flex;
  align-items: center;
}

.timeline-line {
  position: absolute;
  height: 2px;
  width: 100%;
  background: linear-gradient(to right, var(--secondary), var(--tertiary));
}

.timeline-markers {
  position: relative;
  width: 100%;
  display: flex;
  justify-content: space-between;
  z-index: 1;
}

.timeline-marker {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--primary);
  border: 2px solid var(--white);
  cursor: pointer;
  transition: transform 0.2s;
}

.timeline-marker:hover {
  transform: scale(1.3);
}

.timeline-marker.current {
  width: 16px;
  height: 16px;
  background: var(--tertiary);
  border: 3px solid var(--white);
  box-shadow: 0 0 0 2px var(--tertiary);
}

.timeline-marker.failed {
  background: var(--error-red);
}

.summary-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 1rem;
  margin: 1rem 0;
}

.summary-card {
  padding: 1rem;
  border-left: 3px solid #E5E5E5;
}

.summary-label {
  font-size: 0.875rem;
  color: var(--secondary);
  font-weight: 500;
  margin-bottom: 0.5rem;
}

.summary-value {
  font-size: 1.5rem;
  font-weight: 600;
  font-family: "IBM Plex Mono", monospace;
  color: var(--primary);
}

.table-container {
  overflow-x: auto;
  margin: 1rem 0;
}

table {
  width: 100%;
  border-collapse: collapse;
}

thead {
  border-bottom: 2px solid var(--primary);
}

th, td {
  padding: 0.75rem;
  text-align: left;
  border-bottom: 1px solid #F0F0F0;
}

th {
  font-weight: 600;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.1em;
  color: var(--primary);
  background: transparent;
}

td {
  font-size: 0.875rem;
}

tbody tr:hover {
  background: #FAFAFA;
}

.mono {
  font-family: "IBM Plex Mono", monospace;
}

a {
  color: var(--secondary);
  text-decoration: none;
}

a:hover {
  text-decoration: underline;
}

.collapsible {
  cursor: pointer;
  user-select: none;
}

.collapsible::before {
  content: "▼ ";
  display: inline-block;
  margin-right: 0.5rem;
  transition: transform 0.2s;
}

.collapsible.collapsed::before {
  transform: rotate(-90deg);
}

.collapsible-content {
  margin-top: 0.5rem;
}

.collapsible-content.hidden {
  display: none;
}

@media (max-width: 768px) {
  body {
    padding: 1rem;
  }

  .header-info {
    flex-direction: column;
    gap: 1rem;
  }

  .navigation {
    flex-direction: column;
    gap: 1rem;
  }

  .summary-grid {
    grid-template-columns: 1fr;
  }
}
"#
}

/// Escape HTML special characters
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Format JSON with syntax highlighting (simple approach)
pub fn format_json(json: &str) -> String {
    // For now, just escape and pretty-print
    // In the future, could add syntax highlighting
    escape_html(json)
}

/// Get the output directory for HTML files
pub fn get_output_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".portlang").join("html");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Generate a timestamped filename
pub fn get_output_filename(prefix: &str) -> String {
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    format!("{}-{}.html", timestamp, prefix)
}

/// Write HTML to file and optionally open in browser
pub fn write_and_open(html: String, filename: String, auto_open: bool) -> Result<PathBuf> {
    let output_dir = get_output_dir()?;
    let output_path = output_dir.join(&filename);

    fs::write(&output_path, html)?;

    if auto_open {
        open::that(&output_path)?;
    }

    Ok(output_path)
}

/// Render a basic page layout with header
pub fn render_page(title: &str, subtitle: &str, content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
{}
<body>
<div class="container">
    <h1>{}</h1>
    <p style="color: var(--secondary); margin-bottom: 2rem;">{}</p>
    {}
</div>
</body>
</html>"#,
        render_head(title),
        escape_html(title),
        escape_html(subtitle),
        content
    )
}
