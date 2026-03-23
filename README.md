
<div align="center">
  <img src=".github/assets/logo.png" alt="PCTX Logo" style="height: 100px">
  <h1>portlang</h1>

[![Made by](https://img.shields.io/badge/MADE%20BY-Port%20of%20Context-1e40af.svg?style=for-the-badge&labelColor=0c4a6e)](https://portofcontext.com)

</div>

<div align="center">

**Define the search space. The agent finds the path.**

</div>

Most agent frameworks manage loops. portlang manages environments. You declare what the agent can access, what counts as success, and hard limits on cost and scope. The runtime enforces all of it inside an isolated container and records every step.

What enters the context window determines how reliably your agent works. portlang makes the key levers explicit: `re_observation` pushes current state before each step rather than letting context rot build up, `[[verifier]]` gives the agent concrete pass/fail results rather than raw output to parse, and `[boundary]` caps tokens and steps before the model's instruction budget runs out.

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
# fix-bug.field
name = "fix-bug"

[model]
name = "anthropic/claude-sonnet-4.6"
temperature = 0.2

[prompt]
goal = """
There is a bug in src/ causing the test suite to fail. Find it and fix it.
"""
# Fresh test results before every step, not accumulated
re_observation = ["cd /workspace && python -m pytest tests/ -q 2>&1 | tail -10"]

[environment]
root = "./workspace"

[boundary]
allow_write = ["src/*.py"]   # sandbox-enforced: agent cannot touch tests/ to cheat
max_steps = 20
max_cost = "$0.50"

# Success criterion: the test suite must pass
[[verifier]]
name = "tests-pass"
command = "cd /workspace && python -m pytest tests/ -q"
```

```bash
portlang run fix-bug.field
portlang converge fix-bug.field -n 20   # run 20 times, measure convergence before shipping
portlang view trajectory <id>           # replay any run step-by-step
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
| **Field** | Self-contained unit of work: model, tools, goal, constraints, and verifiers in one file. Named after the physics concept, a region of space with properties defined at every point. The agent moves through it and the field determines what's possible. |
| **Vars** | Template variables declared in `[vars]`, interpolated via `{{ name }}`, supplied at runtime with `--var` |
| **Boundary** | Hard limits enforced by sandbox: write paths, network policy, step/cost/token caps. The token ceiling keeps the instruction budget under control. |
| **Verifier** | Deterministic pass/fail signals that run on stop or on each tool call. Gives the agent concrete results rather than raw output to parse. |
| **Trajectory** | Complete event log of every step, tool call, cost, and outcome. Replayable and diffable. |
| **Eval** | Batch run of multiple fields with a persistent ID, resumable on failure |
| **Skills** | Prompt packs loaded into the agent's context from local files, GitHub repos, or ClawHub |
| **Output** | File artifacts declared via `collect` in `[boundary]`, delivered via `--output-dir` or embedded in `--json` stdout |

### Skills

Declare skills in any field. The agent receives a brief metadata summary in its system prompt and reads the full guide on-demand from the workspace filesystem.

```toml
[[skill]]
source = "./skills/my-guide.md"        # local .md file

[[skill]]
source = "./skills/my-skill"           # local directory: SKILL.md + scripts/, references/, assets/ copied to workspace

[[skill]]
source = "owner/repo"                  # GitHub (skills.sh-compatible shorthand)

[[skill]]
source = "clawhub:name"                # ClawHub registry
```

You can reference a skill in the goal with `$slug` — portlang detects invocations and records them in the trajectory. See [examples/07-skills/](examples/07-skills/).

### Output Collection

Declare which files the agent produces with `collect` in `[boundary]`. These are delivered to the caller after the run — separate from write permission (`allow_write`):

```toml
[boundary]
allow_write = ["report.md", "results/*.json", "scratch/*.tmp"]
collect = ["report.md", "results/*.json"]   # omit scratch files
```

If `collect` is not set, all `allow_write` files are collected. Set `collect = []` to collect nothing.

Retrieve outputs after a run:

```bash
portlang run field.field --output-dir ./results/    # copy artifacts to ./results/
portlang run field.field --json                     # machine-readable JSON on stdout
portlang run field.field --json | jq .artifacts     # pipe to jq
```

`--json` embeds both `artifacts` (file contents, up to 512 KB/file) and `structured_output` (from `output_schema`) in one JSON object — the building block for a future HTTP API.

## Security

Agent code runs in isolated containers via [Apple Container](https://developer.apple.com/documentation/virtualization). Network is denied by default. Write access is explicitly granted via glob patterns. Hard ceilings on steps, cost, and context size.

Treat `.field` files as code. Review tool definitions and boundary policies before running untrusted fields.

---

## Claude Code Runner

portlang can use [Claude Code](https://claude.ai/code) as its agent loop instead of the native loop. This gives the agent the full Claude Code toolset inside of portlang.

```bash
portlang run --runner claude-code field.field
```

**Auth:** Run `claude setup-token` to generate a long-lived OAuth token, or set `ANTHROPIC_API_KEY` to use the API directly.

**Limitations vs native runner:** `ToolCall` verifiers and boundary context tracing are not supported

---

## Reflect

`portlang reflect` analyzes a trajectory and identifies concrete ways to improve your field — fewer steps, lower cost, better reliability. The analysis is grounded in the specific steps your agent took and the outcomes it achieved.

```bash
portlang run field.field --auto-reflect   # reflect automatically after each run
portlang reflect <trajectory-id>          # reflect on a past run
```

> `reflect` is itself a portlang field — see [reflect.field](crates/portlang-cli/src/reflect_tools/reflect.field).

**Example: tool naming is part of the environment**

The [examples/03-custom-python-tool](examples/03-custom-python-tool/) field uses a Python calculator tool with a function named `execute`. When run with `--runner claude-code`, Claude Code uses ToolSearch to discover tools lazily — so the agent searches for "calculator":

```
→ ToolSearch  {"query": "calculator"}
← ToolSearch  No matching deferred tools found

→ ToolSearch  {"query": "select:mcp__execute__execute"}
← ToolSearch  (empty — tool is loaded, not deferred)

→ mcp__execute__execute  {"expression": "144 * 259"}   ← finally works, by guessing
```

Two wasted round-trips because the prompt says "calculator" and the tool is named `execute`. Reflect surfaces this immediately:

> **HIGH** Add 'calculator', 'math', 'arithmetic' as keywords to the tool description so ToolSearch resolves it on the first try. Steps 1–2 are pure tool-hunting waste.

The fix is one word in the Python file:

```python
# before
def execute(expression: str) -> CalculatorResult:

# after
def calculate(expression: str) -> CalculatorResult:
```

portlang auto-extracts the function name as the tool name, so the rename propagates automatically. The agent now has a tool called `calculate` — semantically close enough to "calculator" that it can orient immediately without guessing.

This is what "environment-first" means in practice: the agent's behavior is a function of the environment you define. Reflect shows you which knobs to turn.

---

## Examples & Reference

| | |
|---|---|
| [examples/](examples/) | Annotated examples covering all features |
| [field.structure](field.structure) | Full reference for every .field option |
| [CLI.md](CLI.md) | All commands and flags |
