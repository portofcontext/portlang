# portlang HTTP Backend Protocol

This document specifies the JSON-over-HTTP protocol that portlang uses to communicate with remote sandbox backends. Implement this protocol to run portlang agent tasks on any cloud platform (Modal, Lambda, Fly, custom servers, etc.).

## Overview

portlang sends all sandbox operations as HTTP POST requests to a single URL. The backend manages the container lifecycle and streams execution output back. Authentication is handled via a standard `Authorization` header.

## Transport

- **Method:** `POST` for all operations
- **URL:** single endpoint (all ops go to the same URL)
- **Request body:** JSON
- **Response body:** JSON (except `exec_streaming`, which returns NDJSON)
- **Auth:** Set `PORTLANG_BACKEND_AUTHORIZATION` env var on the portlang process. Its value is sent as the `Authorization` header on every request (e.g. `Bearer mytoken` or `ApiKey mykey`).

## Operations

### `build`

Build a container image from Dockerfile content.

**Request:**
```json
{
  "op": "build",
  "dockerfile_content": "FROM debian:bookworm-slim\nRUN apt-get install -y python3\n",
  "tag": "portlang-a3f2c1b4"
}
```

**Response:**
```json
{"ok": true}
```
On failure:
```json
{"ok": false, "error": "build failed: ..."}
```

**Notes:**
- The `tag` is a content-hash derived from the Dockerfile, so identical Dockerfiles reuse the same tag.
- Stateless backends (see [Stateless Backend Pattern](#stateless-backend-pattern) below) can respond `{"ok":true}` immediately without building — the Dockerfile content will arrive again in the subsequent `run` request.

---

### `run`

Start a detached container. Returns a `container_id` used in all subsequent operations.

**Request (basic):**
```json
{
  "op": "run",
  "image": "portlang-a3f2c1b4"
}
```

**Request (stateless — includes dockerfile):**
```json
{
  "op": "run",
  "image": "portlang-a3f2c1b4",
  "dockerfile_content": "FROM debian:bookworm-slim\nRUN apt-get install -y python3\n"
}
```

**Response:**
```json
{"container_id": "sandbox-xyz-789"}
```

**Notes:**
- The container should have `/workspace` available as its working directory.
- `container_id` is an opaque string that portlang echoes back in all subsequent ops; it can be any identifier meaningful to your backend.
- When `dockerfile_content` is present, the backend should build the image and start the container atomically (see [Stateless Backend Pattern](#stateless-backend-pattern)).

---

### `exec`

Execute a shell command inside a running container and return buffered output.

**Request:**
```json
{
  "op": "exec",
  "container_id": "sandbox-xyz-789",
  "cmd": "ls /workspace"
}
```

**Response:**
```json
{
  "stdout": "goal.txt\nsettings.json\n",
  "stderr": "",
  "exit_code": 0
}
```

---

### `exec_streaming`

Stage workspace files into the container, execute a script, and stream output back as NDJSON.

**Request:**
```json
{
  "op": "exec_streaming",
  "container_id": "sandbox-xyz-789",
  "script_content": "#!/bin/sh\nclaude --print ...\n",
  "workspace_files": {
    ".portlang_cc_goal.txt": "<base64-encoded bytes>",
    ".portlang_cc_settings.json": "<base64-encoded bytes>",
    ".portlang_cc_system.txt": "<base64-encoded bytes>"
  }
}
```

**Response (NDJSON stream):**

One JSON object per line, connection stays open until the process exits:
```
{"type": "stdout", "data": "some output line\n"}
{"type": "stderr", "data": "warning: ...\n"}
{"type": "stdout", "data": "more output\n"}
{"type": "exit", "code": 0}
```

**Notes:**
- `workspace_files` keys are filenames (no directory separators), values are standard base64-encoded file contents.
- Stage these files into `/workspace/` before running the script.
- The script is the agent runner (typically the `claude` CLI invoked with `--output-format stream-json`). Its stdout/stderr should be forwarded line-by-line as they arrive.
- The final line must be `{"type":"exit","code":<N>}` after the process terminates.

---

### `stop`

Terminate a container. This is a best-effort cleanup call — portlang does not check the response.

**Request:**
```json
{
  "op": "stop",
  "container_id": "sandbox-xyz-789"
}
```

**Response:** Any JSON (ignored).

---

## Stateless Backend Pattern

Cloud platforms like Modal run each HTTP request as an isolated function invocation. The default `build` + `run` two-call sequence requires persisting state between them (e.g. a distributed key-value store to pass the Dockerfile between calls).

**To avoid this**, portlang forwards `dockerfile_content` in the `run` request whenever a prior `build` was called. Stateless backends can use this to build and start the container atomically in a single request:

```python
# Stateless run handler
elif op == "run":
    dockerfile_content = data.get("dockerfile_content")
    if dockerfile_content:
        # Build image from inline content + start container atomically
        image = build_image_from_dockerfile(dockerfile_content)
    else:
        # No custom Dockerfile — use a base image
        image = base_image()
    container_id = start_container(image)
    return {"container_id": container_id}

# build can be a no-op for stateless backends
if op == "build":
    return {"ok": True}
```

Stateful backends (existing implementations that handle `build` and store the image by tag) are unaffected — they simply ignore the `dockerfile_content` field in `run`.

---

## Example: Minimal Python Backend

```python
from fastapi import FastAPI, Request, Response
from fastapi.responses import StreamingResponse
import json, base64, os

app = FastAPI()

def auth_ok(request):
    token = os.environ.get("PORTLANG_BACKEND_AUTHORIZATION", "")
    return request.headers.get("authorization", "") == token

@app.post("/")
async def handler(request: Request):
    if not auth_ok(request):
        return Response(status_code=401)

    data = await request.json()
    op = data["op"]

    if op == "build":
        return {"ok": True}  # stateless: dockerfile arrives with run

    elif op == "run":
        dockerfile = data.get("dockerfile_content")
        container_id = start_container(dockerfile)  # your impl
        return {"container_id": container_id}

    elif op == "exec":
        stdout, stderr, code = exec_in_container(
            data["container_id"], data["cmd"]
        )
        return {"stdout": stdout, "stderr": stderr, "exit_code": code}

    elif op == "exec_streaming":
        # Stage workspace files
        for name, content_b64 in data.get("workspace_files", {}).items():
            write_file(data["container_id"], f"/workspace/{name}",
                       base64.b64decode(content_b64))

        async def stream():
            async for kind, line in stream_script(
                data["container_id"], data["script_content"]
            ):
                yield json.dumps({"type": kind, "data": line}) + "\n"
            code = await get_exit_code(data["container_id"])
            yield json.dumps({"type": "exit", "code": code}) + "\n"

        return StreamingResponse(stream(), media_type="application/x-ndjson")

    elif op == "stop":
        stop_container(data["container_id"])
        return {"ok": True}
```

---

## Modal Migration Guide (v0.1.17 → v0.1.18+)

These are the concrete changes needed to the Modal `app.py` after upgrading to portlang v0.1.18+.

### 1. Remove `image_store`

The `modal.Dict` is no longer needed. Delete it entirely.

```python
# DELETE
image_store = modal.Dict.from_name("portlang-image-store", create_if_missing=True)
```

### 2. Make `build` a no-op

```python
# BEFORE
if op == "build":
    print(f"[sandbox] op=build tag={data.get('tag')} ...")
    tag = data["tag"]
    await image_store.put.aio(tag, data["dockerfile_content"])
    return Response(content=_json.dumps({"ok": True}), media_type="application/json")

# AFTER
if op == "build":
    return Response(content=_json.dumps({"ok": True}), media_type="application/json")
```

### 3. Receive `dockerfile_content` inline in `run`

```python
# BEFORE
elif op == "run":
    tag = data["image"]
    dockerfile_content = await image_store.get.aio(tag)
    if dockerfile_content:
        with tempfile.NamedTemporaryFile(suffix="Dockerfile", mode="w", delete=False) as f:
            f.write(dockerfile_content)
            tmp_path = f.name
        image = modal.Image.from_dockerfile(tmp_path)
    else:
        image = modal.Image.debian_slim()
    ...

# AFTER
elif op == "run":
    dockerfile_content = data.get("dockerfile_content")
    if dockerfile_content:
        with tempfile.NamedTemporaryFile(suffix="Dockerfile", mode="w", delete=False) as f:
            f.write(dockerfile_content)
            tmp_path = f.name
        image = modal.Image.from_dockerfile(tmp_path)
    else:
        image = modal.Image.debian_slim()
    ...
```

### 4. Simplify JSON parsing in `run_field`

Tracing now goes to stderr, so stdout contains only the JSON result.

```python
# BEFORE — fragile backwards scan
data = None
for line in reversed(stdout_lines):
    line_stripped = line.strip()
    if line_stripped.startswith("{"):
        try:
            data = json.loads(line_stripped)
            break
        except json.JSONDecodeError:
            continue
if data is None:
    detail = (result.stdout or "no output").strip()[-500:]
    _post_to_slack(f"✗ `{field_name}` failed\n```{detail}```")
    return

# AFTER
stdout_data = "".join(stdout_lines)
try:
    data = json.loads(stdout_data.strip())
except json.JSONDecodeError:
    detail = stdout_data.strip()[-500:]
    _post_to_slack(f"✗ `{field_name}` failed\n```{detail}```")
    return
```

### 5. Remove the dead `_Result` class

```python
# BEFORE
class _Result:
    returncode = process.returncode
    stdout = stdout_data
result = _Result()
# ... uses result.returncode and result.stdout

# AFTER — use the variables directly
status = "✓" if process.returncode == 0 else "✗"
# replace result.stdout with stdout_data everywhere
```

### 6. Fix shell injection in `exec_streaming`

```python
# BEFORE
await asyncio.to_thread(sb.exec, "bash", "-c", f"mkdir -p $(dirname {full_path})")

# AFTER
import shlex
await asyncio.to_thread(sb.exec, "bash", "-c", f"mkdir -p $(dirname {shlex.quote(full_path)})")
```

### 7. Align timeouts

Sandbox timeout must be >= function timeout, or the sandbox dies before the function can report the result.

```python
# BEFORE
sb = await modal.Sandbox.create.aio(..., timeout=600)
@app.function(..., timeout=900)

# AFTER — match them
sb = await modal.Sandbox.create.aio(..., timeout=900)
@app.function(..., timeout=900)
```

### 8. Pin `PORTLANG_VERSION` as a top-level constant

Makes it easy to find and bump on each release.

```python
# Add near the top of app.py
PORTLANG_VERSION = "v0.1.18"

# Replace the hardcoded URL in runner_image:
# BEFORE
"curl ... https://github.com/portofcontext/portlang/releases/download/v0.1.17/portlang-installer.sh | sh",

# AFTER
f"curl --proto '=https' --tlsv1.2 -LsSf https://github.com/portofcontext/portlang/releases/download/{PORTLANG_VERSION}/portlang-installer.sh | sh",
```

Note: Modal image definitions are evaluated at import time, so `f-string` interpolation of a module-level constant works fine here.

---

## Running portlang with an HTTP Backend

```bash
export PORTLANG_BACKEND_AUTHORIZATION="Bearer mytoken"

portlang run my-field.field \
  --runner claude-code \
  --backend http \
  --backend-url https://my-backend.example.com/ \
  --json
```
