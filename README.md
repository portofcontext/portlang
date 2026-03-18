
<div align="center">
  <img src=".github/assets/logo.png" alt="PCTX Logo" style="height: 100px">
  <h1>portlang</h1>

[![Made by](https://img.shields.io/badge/MADE%20BY-Port%20of%20Context-1e40af.svg?style=for-the-badge&labelColor=0c4a6e)](https://portofcontext.com)

</div>

<div align="center">

**Define the search space. The agent finds the path.**

</div>

Most agent frameworks manage loops. portlang manages environments. You declare what the agent can access, what counts as success, and hard limits on cost and scope. The runtime enforces all of it inside an isolated container and records every step.

## Install

```bash
brew tap portofcontext/homebrew-tap
brew install portlang
```

```bash
export ANTHROPIC_API_KEY=... 
# or
export OPENROUTER_API_KEY=...

portlang init --install --start

# Install the agent skill to get guided help while building fields
npx skills add https://github.com/portofcontext/skills --skill portlang
```

## Example

```toml
# hello-world.field
name = "hello-world"

[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 1.0

[prompt]
goal = "Write a Python script called hello.py that prints 'Hello, World!' and stop."

[boundary]
allow_write = ["*.py"]
max_steps = 5
max_cost = "$0.10"

[[verifier]]
name = "correct-output"
command = "cd /workspace && python3 hello.py | grep -qF 'Hello, World!'"
description = "Running the script must print: Hello, World!"
```

```bash
portlang run hello-world.field
portlang converge hello-world.field -n 10   # measure reliability
portlang view trajectory <id>               # replay any run step-by-step
```

### Eval suites with `parent.field`

Place a `parent.field` at the root of an eval directory to share model, tools, and boundary config across all child fields:

```
stripe-benchmark/
  parent.field          ← shared model + tools
  01-get-balance/
    get-balance.field   ← model = "inherit", tools = "inherit"
  02-list-customers/
    list-customers.field
```

```bash
portlang eval stripe-benchmark/
portlang view eval <eval-id>
```

---

## Core Primitives

| Primitive | Purpose |
|-----------|---------|
| **Field** | Self-contained unit of work — model, tools, goal, constraints, and verifiers in one file. Named after the physics concept: a region of space with properties defined at every point. The agent moves through the field; the field determines what's possible. |
| **Vars** | Template variables declared in `[vars]`, interpolated via `{{ name }}`, supplied at runtime with `--var` |
| **Boundary** | Hard limits enforced by sandbox — write paths, network policy, step/cost/token caps |
| **Verifier** | Success criteria that run on stop or on each tool call; failure feedback enters the context window |
| **Trajectory** | Complete event log — every step, tool call, cost, and outcome; replayable and diffable |
| **Eval** | Batch run of multiple fields with a persistent ID, resumable on failure |

## Security

Agent code runs in isolated containers via [Apple Container](https://developer.apple.com/documentation/virtualization). Network is denied by default. Write access is explicitly granted via glob patterns. Hard ceilings on steps, cost, and context size.

Treat `.field` files as code. Review tool definitions and boundary policies before running untrusted fields.

---

## Claude Code Runner

portlang can use [Claude Code](https://claude.ai/code) as its agent loop instead of the native loop. This gives the agent Edit (diff-based), Glob, Grep, LSP, WebSearch, and WebFetch — the full Claude Code toolset — inside of portlang.

```bash
portlang run --runner claude-code field.field
```

**Auth:** if you already use Claude Code, no setup is needed — portlang reads credentials from `~/.claude/.credentials.json` automatically. Otherwise, run `claude setup-token` to generate a long-lived OAuth token, or set `ANTHROPIC_API_KEY` to use the API directly.

**Field config mapping:**

| Field config | Behavior |
|---|---|
| `model.name` | Passed to Claude Code |
| `[[tool]]` (MCP) | Passed directly via `--mcp-config` |
| `[[tool]]` (shell/python) | Wrapped as MCP stdio servers, run in container |
| `boundary.allow_write` | Enforced via PostToolUse hook on Write/Edit |
| `boundary.max_steps/cost/tokens` | Monitored from stream; process killed on breach |
| `[[verifier]]` (shell, on_stop) | Run by portlang after agent exits |
| `[[verifier]]` (shell, always/on_tool) | Run as Claude Code PostToolUse hooks |
| `boundary.network` | Always enabled (Claude Code requires API access) |

**Limitations vs native runner:** `ToolCall` verifiers and boundary context tracing are not supported

---

## Examples & Reference

| | |
|---|---|
| [examples/](examples/) | Annotated examples covering all features |
| [field.structure](field.structure) | Full reference for every .field option |
| [CLI.md](CLI.md) | All commands and flags |
