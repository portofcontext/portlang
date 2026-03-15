
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
export ANTHROPIC_API_KEY=...   # or OPENROUTER_API_KEY
portlang init --install --start
```

## Example

This field calls a Stripe MCP server, returns typed JSON, and verifies the result — parameterized so the same definition works for any customer:

```toml
# field.toml (parent — defines shared model and tools)
[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 0.0

[[tool]]
type = "mcp"
name = "stripe"
command = "npx"
args  = ["-y", "@stripe/mcp"]
env   = { STRIPE_SECRET_KEY = "${STRIPE_SECRET_KEY}" }

# get-balance/field.toml
name = "get-balance"

model = "inherit"
tools = "inherit"

[vars]
currency = { required = false, default = "usd", description = "Currency to report" }

[prompt]
goal = "Get the Stripe account balance and return available and pending amounts in {{ currency }}."

[boundary]
network = "allow"
allow_write = ["output.json"]
max_steps = 10
max_cost = "$0.50"
output_schema = """
{
  "type": "object",
  "required": ["available", "pending", "currency"],
  "properties": {
    "available": { "type": "integer" },
    "pending":   { "type": "integer" },
    "currency":  { "type": "string" }
  }
}
"""

[[verifier]]
name = "correct-currency"
type = "json"
schema = '{"properties": {"currency": {"const": "{{ currency }}"}}}'
trigger = "on_stop"
description = "Response must use the requested currency"
```

```bash
# Run once
portlang run get-balance/field.toml --var currency=gbp

# Inject input data into the workspace before the agent starts
portlang run field.toml --input ./customers.csv
portlang run field.toml --input '{"customer_id": "cus_123"}'

# Measure reliability across N runs
portlang converge get-balance/field.toml -n 10

# Run a full eval suite, view results as an HTML dashboard
portlang eval stripe-benchmark/
portlang view eval <eval-id>

# Replay any run step-by-step
portlang view trajectory <id>
```

Key concepts: `[vars]` declares `{{ placeholders }}` resolved at runtime via `--var`. `[boundary]` enforces hard limits in the sandbox. `[[verifier]]` defines success criteria that run automatically and inject feedback into the context window. `output_schema` requires the agent to submit typed JSON via `submit_output`.

---

## Core Primitives

| Primitive | Purpose |
|-----------|---------|
| **Field** | Self-contained unit of work — model, tools, goal, constraints, and verifiers in one file |
| **Vars** | Template variables declared in `[vars]`, interpolated via `{{ name }}`, supplied at runtime with `--var` |
| **Boundary** | Hard limits enforced by sandbox — write paths, network policy, step/cost/token caps |
| **Verifier** | Success criteria that run on stop or on each tool call; failure feedback enters the context window |
| **Trajectory** | Complete event log — every step, tool call, cost, and outcome; replayable and diffable |
| **Eval** | Batch run of multiple fields with a persistent ID, resumable on failure |

## Security

Agent code runs in isolated containers via [Apple Container](https://developer.apple.com/documentation/virtualization). Network is denied by default. Write access is explicitly granted via glob patterns. Hard ceilings on steps, cost, and context size.

Treat `field.toml` as code. Review tool definitions and boundary policies before running untrusted fields.

---

## Examples & Reference

| | |
|---|---|
| [examples/](examples/) | Annotated examples covering all features |
| [field.toml.structure](field.toml.structure) | Full reference for every field.toml option |
| [CLI.md](CLI.md) | All commands and flags |
