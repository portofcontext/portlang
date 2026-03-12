
<div align="center">
  <img src=".github/assets/logo.png" alt="PCTX Logo" style="height: 100px">
  <h1>portlang</h1>

[![Made by](https://img.shields.io/badge/MADE%20BY-Port%20of%20Context-1e40af.svg?style=for-the-badge&labelColor=0c4a6e)](https://portofcontext.com)

</div>

<div align="center">

**Define the search space. The agent finds the path.**

</div>

Most agent frameworks manage loops. portlang manages environments. You declare what success looks like, what the agent can access, and what it cannot do. The runtime handles sandboxed execution, trajectory recording, and verification—so you can measure reliability, not just vibe-check outputs.

<details>
<summary><b>Design Philosophy</b></summary>

<br>

Agent behavior is search through a conditioned space. The policy is trained and opaque. The only variables you control are the environment and the context window. When tasks grow long—overnight refactors, multi-step API workflows, data pipelines—prompts stop being enough. You need structure: boundaries that eliminate bad trajectories, verifiers that steer the search, and trajectories you can replay and diff when things diverge.

A `field.toml` is a declarative unit of agent work. It specifies the model, tools, goal, filesystem access, network policy, cost and step limits, and verifiers that define success. The runtime enforces all of it inside an isolated container and records every step.

</details>

## Getting Started

### Install via Homebrew

```bash
brew tap portofcontext/homebrew-tap
brew install portlang
```

### Build from Source

```bash
git clone https://github.com/portofcontext/portlang
cd portlang
cargo build --release
```

### Setup

```bash
# Set an API key (Anthropic or OpenRouter)
export OPENROUTER_API_KEY=...
export ANTHROPIC_API_KEY=...

# Install Apple Container and start the container runtime
portlang init --install --start

# (Optional) Install the agent skill to guide you as you build fields
npx skills add https://github.com/portofcontext/skills --skill portlang

# Run after creating a field
portlang run field.toml
```

### CLI Reference

Complete [CLI Reference](./CLI.md) — all commands and flags.

---

## Example: Code Task with Verification

A field that writes a Python analyzer, then verifies all tests pass:

```toml
name = "code-task"

[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 1.0

[prompt]
goal = """
Create a Python program that reads a text file and outputs word statistics to JSON.
Include pytest tests in test_analyzer.py that verify all functionality.
Once all three files exist (analyzer.py, test_analyzer.py, requirements.txt), stop.
"""
re_observation = ["echo '=== workspace ===' && ls -1 *.py *.txt 2>/dev/null"]

[environment]
root = "./workspace"

[boundary]
allow_write = ["*.py", "*.txt", "requirements.txt"]
max_steps = 30
max_cost = "$1.00"
max_tokens = 80000

[[verifier]]
name = "pytest"
command = "python -m pytest test_analyzer.py -v 2>&1"
trigger = "on_stop"
description = "All tests must pass"
```

```bash
portlang run examples/02-code-task/field.toml
portlang view trajectory <id>       # HTML step-by-step replay
```

---

## Example: Structured Output with JSON Verifiers

Require the agent to submit a typed JSON response, then verify specific fields:

```toml
name = "analyze-workspace"

[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 0.0

[prompt]
goal = "Analyze the workspace files and report status, file count, and a summary."

[environment]
root = "./workspace"

[boundary]
network = "deny"
max_steps = 10
max_cost = "$1.00"
max_tokens = 100000
output_schema = """
{
  "type": "object",
  "required": ["status", "file_count", "files", "summary"],
  "properties": {
    "status":     { "type": "string", "enum": ["success", "failure"] },
    "file_count": { "type": "integer", "minimum": 0 },
    "files":      { "type": "array", "items": { "type": "string" } },
    "summary":    { "type": "string" }
  }
}
"""

[[verifier]]
name = "status-is-success"
type = "json"
schema = '{"properties": {"status": {"const": "success"}}}'
trigger = "on_stop"
description = "status must be success"
```

When `output_schema` is set, the agent must call `submit_output` with a matching JSON object. JSON verifiers (`type = "json"`) validate specific fields against a schema—no shell scripts needed.

---

## Example: MCP Tool Integration

Connect any MCP server and give the agent real API access. Here a parent `field.toml` defines the shared model and tool configuration:

```toml
# field.toml (parent — shared across all tasks in this directory)
[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 0.0

[[tool]]
type = "mcp"
name = "stripe"
command = "npx"
args  = ["-y", "@stripe/mcp"]
env   = { STRIPE_SECRET_KEY = "${STRIPE_SECRET_KEY}" }
```

```toml
# 01-get-balance/field.toml (child task — inherits model and tools)
name = "get-balance"

model = "inherit"
tools = "inherit"

[prompt]
goal = "Get the Stripe account balance. Return available and pending amounts."

[environment]
root = "./workspace"

[boundary]
allow_write = ["mcp_calls.json"]
network = "allow"
max_steps = 20
max_cost = "$2.00"
max_tokens = 150000
output_schema = """
{
  "type": "object",
  "required": ["available_amount", "pending_amount", "currency"],
  "properties": {
    "available_amount": { "type": "integer" },
    "pending_amount":   { "type": "integer" },
    "currency":         { "type": "string" }
  }
}
"""

[[verifier]]
name = "has-balance"
type = "json"
schema = '{"properties": {"available_amount": {"minimum": 0}}}'
trigger = "on_stop"
description = "Balance must be a non-negative integer"
```

```bash
# Run the full benchmark suite
portlang eval pctx-evals/stripe-benchmark

# Eval results get a persistent ID — view the HTML dashboard
portlang view eval <eval-id>

# Resume an eval that was interrupted, skipping tasks that already passed
portlang eval pctx-evals/stripe-benchmark --resume <eval-id>
```

---

## Measuring Reliability

Single runs tell you if an agent can solve a task. Convergence tells you if it does so reliably:

```bash
# Run 10 times, measure pass rate and token distribution
portlang converge examples/02-code-task/field.toml -n 10

# Run all tasks in a directory and get aggregate accuracy
portlang eval pctx-evals/stripe-benchmark

# Diff two trajectories to find where they diverged
portlang diff <id-a> <id-b>

# View adaptation report — which tool sequences correlate with success
portlang view field <field-name>
```

---

## Core Primitives

| Primitive | Purpose |
|-----------|---------|
| **Field** | Self-contained unit of work — model, tools, goal, constraints, and verifiers in one file |
| **Boundary** | Hard limits enforced by sandbox — write paths, network policy, step/cost/token caps |
| **Verifier** | Success criteria that run automatically and inject feedback into the context window |
| **Structured Output** | JSON schema the agent must match; verified with `type = "json"` verifiers |
| **Trajectory** | Complete event log — every step, tool call, cost, and outcome; replayable and diffable |
| **Eval Run** | A named run of multiple fields with a persistent ID, resumable on failure |

---

## Security

Agent code runs in isolated containers via [Apple Container](https://developer.apple.com/documentation/virtualization). Boundaries are enforced at runtime, not through prompts.

- Workspace is isolated from the host filesystem
- Network access denied by default (`network = "deny"`)
- Write permissions granted explicitly via glob patterns (`allow_write = ["*.json"]`)
- Hard ceilings on steps, cost, and context window size
- Path traversal blocked; boundary violations recorded in the trajectory

**Threat model:**
- ✓ Protects: unauthorized file access, data exfiltration, resource exhaustion
- ✗ Doesn't protect: API key exfiltration via MCP, prompt injection, malicious `field.toml`

Treat `field.toml` as code. Review tool definitions and boundary policies before running untrusted fields.

---

## Examples

| Directory | What it shows |
|-----------|---------------|
| [examples/01-hello-world](examples/01-hello-world/) | Minimal field |
| [examples/02-code-task](examples/02-code-task/) | Writing code with shell verifier |
| [examples/03-custom-shell-tool](examples/03-custom-shell-tool/) | Custom shell tool |
| [examples/04-custom-python-tool](examples/04-custom-python-tool/) | Custom Python tool with type inference |
| [examples/05-converge-and-report](examples/05-converge-and-report/) | Convergence measurement |
| [examples/06-builtin-verifiers](examples/06-builtin-verifiers/) | JSON verifiers and structured output |
| [examples/07-structured-output-example](examples/07-structured-output-example/) | Full structured output with schema |
| [pctx-evals/stripe-benchmark](pctx-evals/stripe-benchmark/) | 12-task MCP eval suite against live Stripe API |
