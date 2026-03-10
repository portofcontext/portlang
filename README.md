

<div align="center">
  <img src=".github/assets/logo.png" alt="PCTX Logo" style="height: 100px">
  <h1>portlang</h1>

[![Made by](https://img.shields.io/badge/MADE%20BY-Port%20of%20Context-1e40af.svg?style=for-the-badge&labelColor=0c4a6e)](https://portofcontext.com)

</div>

<div align="center">

**Define the search space. The agent finds the path.**

</div>

We're currently using portlang to evaluate code mode implementations and measure how environment design impacts model performance on complex tasks. The framework started as an evaluation harness but is growing into a broader platform for building reliable agents—where you declare constraints and success criteria, and the runtime handles execution, sandboxing, and trajectory recording.

<details>
<summary><b>Design Philosophy</b></summary>

<br>

Most agent frameworks manage loops. portlang manages environments. You declare what success looks like, what the agent can observe, and what it cannot do. The runtime executes the search and records the trajectory.

**Why This Approach**

Agent behavior is search through a conditioned space. The policy is trained and opaque. The only variables you control are the environment and the context window. When tasks grow long—overnight refactors, multi-file changes, data pipelines—prompts stop being enough. You need structure: boundaries that eliminate bad trajectories, verifiers that steer the search, and trajectories you can replay when things diverge.

</details>

## Getting Started

```bash
git clone https://github.com/portofcontext/portlang
cd portlang
cargo build --release

# Set an API Key
export OPENROUTER_API_KEY=...
export ANTHROPIC_API_KEY=...

# Helps install apple container dependency
./target/release/portlang init --start

# Run a field
./target/release/portlang run examples/02-code-task/field.toml
```

### portlang skill

Install the portlang skill for interactive guidance:

```bash
npx skills add https://github.com/portofcontext/skills --skill portlang
```

## Example: Data Processing with Multi-Layer Verification

```toml
name = "process-sales-data"
goal = "Read sales.csv, calculate revenue per region, output summary.json"

[model]
name = "anthropic/claude-sonnet-4.6"

[boundary]
allow_write = ["summary.json"]          # Only output file
allow_read = ["sales.csv"]              # Only input file
network = "deny"                        # No external calls
max_steps = 20
max_cost = "$1.00"

[context]
max_tokens = 32000                      # Hard ceiling

[[verifiers]]
name = "output-exists"
command = "test -f summary.json"
trigger = "on_stop"
description = "Output file must exist"

[[verifiers]]
name = "valid-json"
command = "python -m json.tool summary.json > /dev/null"
trigger = "on_stop"
description = "Output must be valid JSON"

[[verifiers]]
name = "correct-schema"
command = """
python -c "
import json
with open('summary.json') as f:
    data = json.load(f)
required = ['North', 'South', 'East', 'West']
assert all(k in data for k in required), 'Missing regions'
assert all(isinstance(v, (int, float)) for v in data.values()), 'Invalid values'
"
"""
trigger = "on_stop"
description = "JSON must have all regions with numeric revenues"
```

**What this does:**

- **Boundaries**: Agent can only write summary.json, read sales.csv, zero network access
- **Verifiers**: Three-layer verification (file exists → valid JSON → correct schema)
- **Budget**: Hard 32k token ceiling, terminates when exceeded
- **Trajectory**: Every step recorded, replayable with `portlang replay <id>`

**Verifiers run in order and stop on first failure**, giving precise feedback about what went wrong.

Run 10 times to measure reliability:

```bash
portlang converge examples/02-code-task/field.toml -n 10
```

Get convergence rate, token distribution, and adaptation report showing which tools correlate with success.

## Core Primitives

| Primitive | Purpose |
|-----------|---------|
| **Field** | Self-contained unit of work—like a function with declared constraints |
| **Environment** | What the agent can observe—filesystem snapshot, tools, network policy |
| **Boundary** | Hard walls enforced by sandbox—write permissions, cost limits, step limits |
| **Verifier** | Success criteria that run and inject feedback into context window |
| **Context Policy** | Token budget (hard ceiling) and re-observation schedule |
| **Trajectory** | Complete event log—replayable, diffable, queryable |

## Security

Agent code runs in isolated containers using [Apple Container](https://developer.apple.com/documentation/virtualization). Boundaries are enforced at runtime, not through prompts.

**Sandboxing:**
- Workspace isolated from host filesystem
- Network access denied by default (`network = "deny"`)
- Write permissions granted via glob patterns in `[boundary]`
- Hard limits on steps, cost, and tokens

**Enforcement:**
- Invalid writes rejected before execution
- Path traversal (`../`) blocked
- Custom Python/shell tools run with normalized paths
- Boundary violations recorded in trajectory with context trace

**Threat Model:**
- ✓ Protects: Unauthorized file access, data exfiltration, resource exhaustion
- ✗ Doesn't protect: API key exfiltration, prompt injection, malicious `field.toml`

Treat `field.toml` as code. Review tool definitions and boundary policies before running untrusted fields.

## Commands

```bash
portlang run <field>              # Execute once
portlang check <field>            # Validate before running
portlang eval <directory>         # Run all fields, report aggregate stats
portlang converge <field> -n N    # Run N times, measure convergence rate
portlang replay <id>              # Step through trajectory
portlang diff <id-a> <id-b>       # Find divergence point
portlang view trajectory <field>  # Adaptation analysis across runs
portlang view eval <directory>.   # Analytics for an eval run
```

## Examples

[Basic Examples](examples/) Five examples from minimal to complex

[Code Mode Evals](pctx-evals/) Evals specific to Code Mode
