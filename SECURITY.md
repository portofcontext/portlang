# Security

portlang uses defense-in-depth to sandbox agent execution. No single mechanism is relied upon exclusively.

## Containerization

All agent tool execution routes through an Apple container sandbox. Each field run spawns a dedicated container with a unique UUID-based name (`portlang-{uuid}`). The host workspace is bind-mounted at `/workspace` inside the container. At the end of the run the container is stopped and removed via the `Drop` impl.

- Default image: `debian:bookworm-slim` (minimal attack surface)
- Custom images supported via `environment.image` or `environment.dockerfile`
- Packages installed via APT at image-build time, not at runtime

Custom tools (Python, shell), MCP stdio servers, and built-in tools (read, write, glob, bash) all execute inside this container via `container exec`.

## Boundary Enforcement

The `boundary` section of a field configuration restricts what the agent can do:

- **`allow_write`** — glob patterns that define the only paths the agent may write. Empty by default (deny all writes).
- **`bash`** — boolean flag (default `true`) that controls whether the bash tool is registered. Set to `false` to restrict the agent to read-only filesystem tools.
- **`network`** — network policy (`Allow`/`Deny`). Infrastructure is in place; full enforcement is planned.

Write boundaries are checked at two points:

1. **Pre-dispatch** — `sandbox.check_boundary()` is called before every tool execution. If a write path does not match any `allow_write` pattern, the action is rejected and fed back to the agent as an error.

2. **Post-execution** — the bash tool takes a timestamp snapshot before running a command, then finds any files modified after that snapshot, removes any that violate `allow_write`, and reports the violations to the agent.

## Path Canonicalization

The read and write tools canonicalize both the sandbox root and the requested path, then assert the requested path starts with the root. This prevents `../` escape attacks from reaching the host filesystem.

## Shell Escaping

All paths passed to shell commands are escaped with the `shell_escape` crate. The shell command tool additionally wraps template-substituted values in single quotes and escapes embedded single quotes.

## MCP Server Isolation

MCP stdio servers are started inside the container via `container exec -i`. All MCP tool calls execute inside the container. A 30-second initialization timeout prevents hangs on unresponsive servers.

## Code Mode

Code mode evaluates TypeScript in an embedded V8/Deno runtime on the host. The TypeScript control flow itself cannot be containerized.

**Side effects are fully sandboxed.** All tool callbacks registered for code mode dispatch through `sandbox.dispatch()`, not the tool registry directly. This means every file read, write, glob, bash call, or custom tool invocation originating from within code mode executes inside the Apple container and is subject to the same boundary enforcement as direct agent tool calls.

The only thing that runs on the host is the TypeScript evaluation itself which is sandboxed by Deno.

## Loop Detection

The `LoopDetector` tracks the last 10 agent actions and aborts with a feedback message if:

- The same action is repeated 3 or more times (detected by action signature + input hash)
- The same file is read repeatedly without a write in between
- 3 consecutive tool calls fail

## Execution Budgets

Hard limits prevent runaway agents from consuming unbounded resources:

- **`max_tokens`** — maximum context-window size (input tokens per API call)
- **`max_cost`** — maximum total spend in microdollars
- **`max_steps`** — maximum number of agent steps

The run terminates with `RunOutcome::BudgetExhausted` when any limit is reached.

## Input Validation

Each tool validates its required parameters before execution. Tools fail fast with a typed error if a required field is missing or has the wrong type, rather than proceeding with a partially-constructed command.

## Verifier Pre-flight

Shell verifiers are run once on the empty workspace before the agent starts. A verifier that exits with code 127 (command not found) aborts the entire run immediately, preventing wasted model budget on an environment that cannot satisfy the task.

## Structured Output Validation

When `output_schema` is defined, the agent's `submit_output` call is validated against the JSON schema before the trajectory is finalized. Coercion is attempted (up to 10 fixup rounds) and the agent is given feedback on failures.

## Boundary Violation Tracing

When a write boundary violation occurs, `ContextTracer` analyzes the model's context window (system prompt, tool definitions, environment context) to identify where the model may have learned the violating path. This is surfaced in the rejection message to aid debugging.
