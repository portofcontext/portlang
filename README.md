

<div align="center">
  <img src=".github/assets/logo.png" alt="PCTX Logo" style="height: 100px">
  <h1>portlang</h1>

[![Made by](https://img.shields.io/badge/MADE%20BY-Port%20of%20Context-1e40af.svg?style=for-the-badge&labelColor=0c4a6e)](https://portofcontext.com)

</div>

<div align="center">

**Define the search space. The agent finds the path.**

</div>

Most agent frameworks manage loops. portlang manages environments. You declare what success looks like, what the agent can observe, and what it cannot do. The runtime executes the search and records the trajectory.

## Why

Agent behavior is search through a conditioned space. The policy is trained and opaque. The only variables you control are the environment and the context window. When tasks grow long—overnight refactors, multi-file changes, data pipelines—prompts stop being enough. You need structure: boundaries that eliminate bad trajectories, verifiers that steer the search, and trajectories you can replay when things diverge.

## Getting Started

**Prerequisites:** Rust 1.89+, Anthropic API key, [Apple Container](https://developer.apple.com/documentation/virtualization)

```bash
git clone https://github.com/portofcontext/portlang
cd portlang
cargo build --release
export ANTHROPIC_API_KEY=sk-ant-...

# Run a field
./target/release/portlang run examples/02-code-task/field.toml

# Check environment
./target/release/portlang init
```

## Example: Scoped Bug Fix with Verification

```toml
name = "fix-jwt-validation"
goal = "Fix JWT expiration bug in auth.py—exp claim compared as string, not int"

[model]
name = "anthropic/claude-sonnet-4.6"

[environment]
type = "local"
root = "./workspace"

[boundary]
allow_write = ["auth.py", "tests/**"]  # Only these files
network = "deny"
max_steps = 30
max_cost = "$2.00"

[context]
max_tokens = 32000                      # Hard ceiling
re_observation = ["git diff --stat"]    # Keep context fresh

[[verifiers]]
name = "tests-pass"
command = "pytest tests/test_auth.py -x"
trigger = "on_stop"                     # Runs when agent stops

[[verifiers]]
name = "scope-guard"
command = "git diff --name-only | grep -qvE '^(auth\\.py|tests/)'"
trigger = "on_stop"
```

**What this does:**

- **Boundaries**: Agent can only write to auth.py and tests/—violations physically blocked by sandbox
- **Verifiers**: Tests must pass AND no files outside scope can change
- **Budget**: Hard 32k token ceiling—when reached, run terminates
- **Trajectory**: Every step recorded, replayable with `portlang replay <id>`

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

## Commands

```bash
portlang run <field>              # Execute once
portlang check <field>            # Validate before running
portlang eval <directory>         # Run all fields, report aggregate stats
portlang converge <field> -n N    # Run N times, measure convergence rate
portlang replay <id>              # Step through trajectory
portlang diff <id-a> <id-b>       # Find divergence point
portlang report <field>           # Adaptation analysis across runs
```

## More

- [Examples](examples/) - Five examples from minimal to complex
