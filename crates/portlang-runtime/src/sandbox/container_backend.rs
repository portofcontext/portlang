use super::traits::{ChildHandle, CommandOutput, ScriptHandle};
use async_trait::async_trait;
use base64::Engine as _;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Abstracts over container runtimes that share a Docker-compatible CLI interface,
/// as well as external backends reachable over HTTP or subprocess.
///
/// Adding a new backend means implementing this trait — the rest of the sandbox
/// machinery in [`ContainerSandbox`] stays unchanged.
#[async_trait]
pub trait ContainerBackend: Send + Sync {
    /// Short name used in logs and trajectory metadata.
    fn name(&self) -> &str;

    /// The CLI binary used to exec into containers (e.g. "container", "docker").
    /// Returns `""` for non-CLI backends.
    fn cli(&self) -> &str;

    /// Build an image from a Dockerfile.
    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String>;

    /// Start a detached container with the workspace bind-mounted at `/workspace`.
    /// Returns the container identifier for subsequent `exec` and `stop` calls.
    async fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String>;

    /// Execute a shell command inside a running container (buffered, non-streaming).
    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String>;

    /// Stop a running container. Failures are silently ignored (best-effort cleanup).
    /// Kept synchronous so it can be called from `Drop`.
    fn stop(&self, container_id: &str);

    /// Stage `script_content` into the container and execute it, returning live
    /// stdout/stderr streams and a handle for kill/wait.
    ///
    /// **Default implementation** (for CLI backends): writes the script to
    /// `host_workspace/.portlang_cc_runner.sh` and spawns
    /// `{cli} exec {container_id} sh /workspace/.portlang_cc_runner.sh`.
    ///
    /// Override for backends that don't have a local CLI (HTTP, subprocess).
    async fn exec_streaming(
        &self,
        container_id: &str,
        script_content: &str,
        host_workspace: &Path,
    ) -> Result<ScriptHandle, String> {
        use std::process::Stdio;
        use tokio::process::Command as AsyncCommand;

        tokio::fs::write(
            host_workspace.join(".portlang_cc_runner.sh"),
            script_content,
        )
        .await
        .map_err(|e| format!("Failed to write runner script: {e}"))?;

        let mut child = AsyncCommand::new(self.cli())
            .args([
                "exec",
                container_id,
                "sh",
                "/workspace/.portlang_cc_runner.sh",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn container exec: {e}"))?;

        let stdout = Box::new(child.stdout.take().expect("stdout piped"));
        let stderr = Box::new(child.stderr.take().expect("stderr piped"));

        Ok(ScriptHandle {
            stdout,
            stderr,
            exec: Box::new(ChildHandle(child)),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers shared by external backends
// ---------------------------------------------------------------------------

/// Read all files in `workspace` (top-level only) and return them as a map of
/// relative path → base64-encoded bytes.  Sent to remote backends so they can
/// stage portlang helper files (goal, settings, MCP config, etc.) before running
/// the agent script.
fn collect_workspace_files(workspace: &Path) -> HashMap<String, String> {
    let mut files = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(workspace) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(bytes) = std::fs::read(&path) {
                    files.insert(
                        name,
                        base64::engine::general_purpose::STANDARD.encode(&bytes),
                    );
                }
            }
        }
    }
    files
}

/// `ScriptExecHandle` for buffered (non-streaming) remote backends.
/// `kill()` is a no-op; `wait()` returns the recorded exit code immediately.
struct BufferedHandle {
    exit_code: Option<i32>,
}

#[async_trait]
impl super::traits::ScriptExecHandle for BufferedHandle {
    async fn kill(&mut self) -> std::io::Result<()> {
        Ok(()) // already finished
    }
    async fn wait(&mut self) -> std::io::Result<Option<i32>> {
        Ok(self.exit_code)
    }
}

/// Parse buffered NDJSON streaming output into stdout/stderr byte vecs and exit code.
fn parse_streaming_ndjson(text: &str) -> (Vec<u8>, Vec<u8>, Option<i32>) {
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    let mut exit_code = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("stdout") => {
                if let Some(d) = v["data"].as_str() {
                    stdout_buf.extend_from_slice(d.as_bytes());
                }
            }
            Some("stderr") => {
                if let Some(d) = v["data"].as_str() {
                    stderr_buf.extend_from_slice(d.as_bytes());
                }
            }
            Some("exit") => {
                exit_code = v["code"].as_i64().map(|c| c as i32);
            }
            _ => {}
        }
    }

    (stdout_buf, stderr_buf, exit_code)
}

fn buffered_script_handle(
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
    exit_code: Option<i32>,
) -> ScriptHandle {
    ScriptHandle {
        stdout: Box::new(std::io::Cursor::new(stdout_buf)),
        stderr: Box::new(std::io::Cursor::new(stderr_buf)),
        exec: Box::new(BufferedHandle { exit_code }),
    }
}

// ---------------------------------------------------------------------------
// Apple Container backend
// ---------------------------------------------------------------------------

/// Container backend using Apple's native `container` CLI (macOS only).
pub struct AppleContainerBackend;

impl AppleContainerBackend {
    /// Returns `true` if the `container` binary is present and responsive.
    pub fn is_available() -> bool {
        Command::new("container")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl ContainerBackend for AppleContainerBackend {
    fn name(&self) -> &str {
        "apple-container"
    }

    fn cli(&self) -> &str {
        "container"
    }

    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("container")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("container build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("container")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("container run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("container")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("container exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("container")
            .args(["stop", container_id])
            .output();
    }
}

// ---------------------------------------------------------------------------
// Podman backend
// ---------------------------------------------------------------------------

/// Container backend using the `podman` CLI (Docker-compatible interface).
pub struct PodmanBackend;

impl PodmanBackend {
    /// Returns `true` if the `podman` binary is present and responsive.
    pub fn is_available() -> bool {
        Command::new("podman")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl ContainerBackend for PodmanBackend {
    fn name(&self) -> &str {
        "podman"
    }

    fn cli(&self) -> &str {
        "podman"
    }

    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("podman")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("podman build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("podman")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("podman run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("podman")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("podman exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("podman").args(["stop", container_id]).output();
    }
}

// ---------------------------------------------------------------------------
// Docker backend
// ---------------------------------------------------------------------------

/// Container backend using the standard `docker` CLI.
pub struct DockerBackend;

impl DockerBackend {
    /// Returns `true` if the Docker daemon is reachable (`docker info` succeeds).
    pub fn is_available() -> bool {
        Command::new("docker")
            .arg("info")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[async_trait]
impl ContainerBackend for DockerBackend {
    fn name(&self) -> &str {
        "docker"
    }

    fn cli(&self) -> &str {
        "docker"
    }

    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let output = Command::new("docker")
            .args(["build", "-f", dockerfile_path, "-t", tag, context])
            .output()
            .map_err(|e| format!("docker build failed: {e}"))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn run(
        &self,
        container_name: &str,
        image: &str,
        host_workspace: &Path,
    ) -> Result<String, String> {
        let volume = format!("{}:/workspace", host_workspace.display());
        let output = Command::new("docker")
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "--workdir",
                "/workspace",
                "--volume",
                &volume,
                image,
                "sleep",
                "infinity",
            ])
            .output()
            .map_err(|e| format!("docker run failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).into_owned())
        }
    }

    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let output = Command::new("docker")
            .args(["exec", container_id, "sh", "-c", cmd])
            .output()
            .map_err(|e| format!("docker exec failed: {e}"))?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
            success: output.status.success(),
        })
    }

    fn stop(&self, container_id: &str) {
        let _ = Command::new("docker").args(["stop", container_id]).output();
    }
}

// ---------------------------------------------------------------------------
// HTTP backend
// ---------------------------------------------------------------------------

/// Backend that speaks a JSON-over-HTTP protocol to a remote shim
///
/// All batch ops (build, run, exec, stop) POST `{"op": "...", ...}` to `url`
/// and expect a JSON response.  Streaming exec POSTs and reads NDJSON lines:
/// `{"type":"stdout","data":"..."}`, `{"type":"stderr","data":"..."}`,
/// `{"type":"exit","code":0}`.
pub struct HttpBackend {
    pub url: String,
    client: reqwest::Client,
}

impl HttpBackend {
    /// Reads `PORTLANG_BACKEND_AUTHORIZATION` at construction time and sets it
    /// as the default `Authorization` header on every request.  The value is
    /// the full header value, e.g. `"Bearer mytoken"` or `"ApiKey mykey"`.
    pub fn new(url: String) -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        if let Ok(auth) = std::env::var("PORTLANG_BACKEND_AUTHORIZATION") {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&auth) {
                headers.insert(reqwest::header::AUTHORIZATION, val);
            }
        }
        Self {
            url,
            client: reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .unwrap_or_default(),
        }
    }

    async fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .post(&self.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "HTTP {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("Failed to parse HTTP response JSON: {e}"))
    }
}

#[async_trait]
impl ContainerBackend for HttpBackend {
    fn name(&self) -> &str {
        "http"
    }

    fn cli(&self) -> &str {
        ""
    }

    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let resp = self
            .call(serde_json::json!({
                "op": "build",
                "dockerfile_path": dockerfile_path,
                "tag": tag,
                "context": context,
            }))
            .await?;

        if resp["ok"].as_bool().unwrap_or(false) {
            Ok(())
        } else {
            Err(resp["error"].as_str().unwrap_or("build failed").to_string())
        }
    }

    async fn run(
        &self,
        _container_name: &str,
        image: &str,
        _host_workspace: &Path,
    ) -> Result<String, String> {
        let resp = self
            .call(serde_json::json!({
                "op": "run",
                "image": image,
            }))
            .await?;

        resp["container_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("HTTP run: missing container_id in response: {resp}"))
    }

    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let resp = self
            .call(serde_json::json!({
                "op": "exec",
                "container_id": container_id,
                "cmd": cmd,
            }))
            .await?;

        Ok(CommandOutput {
            stdout: resp["stdout"].as_str().unwrap_or("").to_string(),
            stderr: resp["stderr"].as_str().unwrap_or("").to_string(),
            exit_code: resp["exit_code"].as_i64().unwrap_or(-1) as i32,
            success: resp["exit_code"].as_i64().unwrap_or(-1) == 0,
        })
    }

    fn stop(&self, container_id: &str) {
        // Fire-and-forget: spawn a task if inside a tokio runtime.
        let url = self.url.clone();
        let id = container_id.to_string();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let client = reqwest::Client::new();
                let _ = client
                    .post(&url)
                    .json(&serde_json::json!({"op": "stop", "container_id": id}))
                    .send()
                    .await;
            });
        }
    }

    async fn exec_streaming(
        &self,
        container_id: &str,
        script_content: &str,
        host_workspace: &Path,
    ) -> Result<ScriptHandle, String> {
        let workspace_files = collect_workspace_files(host_workspace);

        let resp = self
            .client
            .post(&self.url)
            .json(&serde_json::json!({
                "op": "exec_streaming",
                "container_id": container_id,
                "script_content": script_content,
                "workspace_files": workspace_files,
            }))
            .send()
            .await
            .map_err(|e| format!("HTTP exec_streaming failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!(
                "HTTP exec_streaming {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read streaming response: {e}"))?;

        let (stdout_buf, stderr_buf, exit_code) = parse_streaming_ndjson(&text);
        Ok(buffered_script_handle(stdout_buf, stderr_buf, exit_code))
    }
}

// ---------------------------------------------------------------------------
// Subprocess backend
// ---------------------------------------------------------------------------

/// Backend that delegates to an external process speaking JSON on stdin/stdout.
///
/// Each batch op spawns `{command}`, writes one JSON request to its stdin, and
/// reads one JSON response from its stdout.  Streaming exec reads NDJSON lines
/// (`{"type":"stdout","data":"..."}` / `{"type":"stderr","data":"..."}` /
/// `{"type":"exit","code":0}`) until EOF.
///
/// Example field config:
/// ```toml
/// [environment]
/// backend = "subprocess"
/// backend_command = "python3 modal_backend.py"
/// ```
pub struct SubprocessBackend {
    pub command: String,
}

impl SubprocessBackend {
    pub fn new(command: String) -> Self {
        Self { command }
    }

    async fn call(&self, body: serde_json::Value) -> Result<serde_json::Value, String> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command as AsyncCommand;

        let mut child = AsyncCommand::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn backend subprocess: {e}"))?;

        let mut stdin = child.stdin.take().expect("stdin piped");
        let request = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize request: {e}"))?;
        stdin
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to subprocess stdin: {e}"))?;
        drop(stdin);

        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let first_line = lines
            .next_line()
            .await
            .map_err(|e| format!("Failed to read subprocess response: {e}"))?
            .ok_or_else(|| "Subprocess produced no output".to_string())?;

        let _ = child.wait().await;

        serde_json::from_str(&first_line)
            .map_err(|e| format!("Failed to parse subprocess response JSON: {e}"))
    }
}

#[async_trait]
impl ContainerBackend for SubprocessBackend {
    fn name(&self) -> &str {
        "subprocess"
    }

    fn cli(&self) -> &str {
        ""
    }

    async fn build(&self, dockerfile_path: &str, tag: &str, context: &str) -> Result<(), String> {
        let resp = self
            .call(serde_json::json!({
                "op": "build",
                "dockerfile_path": dockerfile_path,
                "tag": tag,
                "context": context,
            }))
            .await?;

        if resp["ok"].as_bool().unwrap_or(false) {
            Ok(())
        } else {
            Err(resp["error"].as_str().unwrap_or("build failed").to_string())
        }
    }

    async fn run(
        &self,
        _container_name: &str,
        image: &str,
        _host_workspace: &Path,
    ) -> Result<String, String> {
        let resp = self
            .call(serde_json::json!({
                "op": "run",
                "image": image,
            }))
            .await?;

        resp["container_id"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Subprocess run: missing container_id in response: {resp}"))
    }

    async fn exec(&self, container_id: &str, cmd: &str) -> Result<CommandOutput, String> {
        let resp = self
            .call(serde_json::json!({
                "op": "exec",
                "container_id": container_id,
                "cmd": cmd,
            }))
            .await?;

        Ok(CommandOutput {
            stdout: resp["stdout"].as_str().unwrap_or("").to_string(),
            stderr: resp["stderr"].as_str().unwrap_or("").to_string(),
            exit_code: resp["exit_code"].as_i64().unwrap_or(-1) as i32,
            success: resp["exit_code"].as_i64().unwrap_or(-1) == 0,
        })
    }

    fn stop(&self, container_id: &str) {
        // Best-effort sync stop — spawn subprocess and ignore errors.
        let body = serde_json::json!({"op": "stop", "container_id": container_id});
        let req = serde_json::to_string(&body).unwrap_or_default();
        if let Ok(mut child) = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            if let Some(ref mut stdin) = child.stdin {
                use std::io::Write;
                let _ = stdin.write_all(req.as_bytes());
            }
            let _ = child.wait();
        }
    }

    async fn exec_streaming(
        &self,
        container_id: &str,
        script_content: &str,
        host_workspace: &Path,
    ) -> Result<ScriptHandle, String> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::process::Command as AsyncCommand;

        let workspace_files = collect_workspace_files(host_workspace);

        let body = serde_json::json!({
            "op": "exec_streaming",
            "container_id": container_id,
            "script_content": script_content,
            "workspace_files": workspace_files,
        });
        let request = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize exec_streaming request: {e}"))?;

        let mut child = AsyncCommand::new("sh")
            .arg("-c")
            .arg(&self.command)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn backend subprocess: {e}"))?;

        let mut stdin = child.stdin.take().expect("stdin piped");
        stdin
            .write_all(request.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to subprocess stdin: {e}"))?;
        drop(stdin);

        // Collect all NDJSON lines from stdout.
        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut all_lines = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            all_lines.push_str(&line);
            all_lines.push('\n');
        }
        let _ = child.wait().await;

        let (stdout_buf, stderr_buf, exit_code) = parse_streaming_ndjson(&all_lines);
        Ok(buffered_script_handle(stdout_buf, stderr_buf, exit_code))
    }
}
