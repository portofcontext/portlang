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

## Running portlang with an HTTP Backend

```bash
export PORTLANG_BACKEND_AUTHORIZATION="Bearer mytoken"

portlang run my-field.field \
  --runner claude-code \
  --backend http \
  --backend-url https://my-backend.example.com/ \
  --json
```
