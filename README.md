# portlang

**An environment-first agent framework. Define the search space. The agent finds the path.**

---

Most agent frameworks are loop orchestrators. They manage the turn-taking between a model and a set of tools: call the model, parse the output, dispatch a tool call, append the result, repeat. The developer's job becomes prompt engineering.

portlang is built on a different premise: agent behavior is search, and the developer's primary job is engineering the search space, not the searcher. You declare what success looks like (verifiers), what the agent can touch (boundaries), and how much context it gets (budget). The runtime executes the search and records the trajectory. When things go wrong, you don't improve the prompt — you sharpen the search space.

---

## Use case: ship a bug fix

```toml
[field]
name = "fix-jwt-expiry"
model = "claude-sonnet-4-6"
prompt = "Fix the JWT expiration bug in auth.py — the exp claim is compared as a string."

[environment]
root = "."

[[verifier]]
name = "tests-pass"
run = "pytest tests/test_auth.py -x"
when = "on_stop"

[boundary]
allow_write = ["auth.py", "tests/**"]
network = "deny_all"
max_cost = "$1.00"
max_steps = 20
```

The agent reads the codebase, finds the bug, writes a fix, and stops. The verifier runs automatically. If tests fail, the agent sees the output and tries again — within the same run. You get a trajectory log of every step.

## Use case: run an eval suite

```bash
portlang eval examples/

  ✓  [1/5] hello-world          2 steps    4,103 tokens   $0.05
  ✓  [2/5] code-task            7 steps   18,902 tokens   $0.23
  ✓  [3/5] custom-shell-tool    4 steps    6,218 tokens   $0.08
  ✓  [4/5] custom-python-tool   2 steps    3,891 tokens   $0.05
  ✗  [5/5] code-task            budget exhausted

══════════════════════════════════════════════════════
Eval Results
══════════════════════════════════════════════════════
Tasks:   6
Passed:  5  (83.3%)
Failed:  1  (16.7%)

Cost:    $0.52 total   $0.09 avg
Tokens:  38,555 total  6,426 avg
Steps:   3.3 avg

Failed:
  ✗  code-task — budget exhausted
```

Drop your field definitions in a directory. `portlang eval` runs them all and reports accuracy.

---

## Getting started

**Prerequisites:** Rust, an Anthropic API key, and [Apple Container](https://developer.apple.com/documentation/virtualization) (macOS 26+) for sandbox isolation.

```bash
git clone https://github.com/portofcontext/portlang
cd portlang
cargo build --release
export ANTHROPIC_API_KEY=sk-ant-...

# Check environment
./target/release/portlang init

# Run the simplest example
./target/release/portlang run examples/01-hello-world/field.toml
```

---

## Examples

| Example | What it shows |
|---------|---------------|
| [01-hello-world](examples/01-hello-world/) | Minimal field — no verifiers, five steps |
| [02-code-task](examples/02-code-task/) | Verifier-driven coding task — agent writes code, tests run automatically |
| [03-custom-shell-tool](examples/03-custom-shell-tool/) | Custom tools via shell commands — extend the agent without touching Rust |
| [04-custom-python-tool](examples/04-custom-python-tool/) | Custom tools via Python scripts — full language support |
| [05-converge-and-report](examples/05-converge-and-report/) | Convergence analysis — run N times, inspect adaptation report |

### Using OpenRouter

Any model name containing `/` is routed through [OpenRouter](https://openrouter.ai), giving access to 100+ models. Set `OPENROUTER_API_KEY` instead of `ANTHROPIC_API_KEY`, then change one line in your field:

```toml
[model]
name = "openai/gpt-4-turbo"      # or "google/gemini-pro", "meta-llama/llama-3-70b-instruct", ...
```

---

## Commands

| Command | What it does |
|---------|-------------|
| `portlang run <field>` | Execute a field once |
| `portlang check <field>` | Validate field configuration without running |
| `portlang eval <directory>` | Run all fields in a directory, report aggregate accuracy |
| `portlang converge <field> -n N` | Run a field N times, measure convergence reliability |
| `portlang list [field]` | List saved trajectories, optionally filtered |
| `portlang replay <id>` | Step through a trajectory interactively |
| `portlang diff <id-a> <id-b>` | Find where two trajectories diverged |
| `portlang report <field>` | Adaptation analysis across all saved runs |
| `portlang init` | Check environment and container setup |

---

## Trajectory analysis

After several runs, `portlang report` tells you which tools correlate with success, where token budgets are being spent, and whether your verifiers are discriminating or noisy.

```
$ portlang converge examples/06-converge-and-report/field.toml -n 10
$ portlang report code-task

=== Adaptation Report: code-task ===
Runs analyzed: 10
Convergence rate: 80.0%

Token Usage:
  Mean: 14382
  Median: 13901
  P90: 21440
  P99: 24103

Cost:
  Mean: $0.1821
  Median: $0.1763
  P90: $0.2744

Steps:
  Mean: 6.2
  Median: 6
  Max: 12

Tool Usage:
  write:
    Invocations: 48
    Used in: 10/10 runs
    Convergence when used: 80.0%
    Convergence when NOT used: 0.0%
  read:
    Invocations: 22
    Used in: 9/10 runs
    Convergence when used: 88.9%
    Convergence when NOT used: 0.0%
  glob:
    Invocations: 11
    Used in: 7/10 runs
    Convergence when used: 71.4%
    Convergence when NOT used: 100.0%

Verifier Signals:
  tests-pass:
    Invocations: 10
    Pass rate: 80.0%
    Pass rate in converged: 100.0%
    Pass rate in failed: 0.0%
```

This tells you: `glob` is being used in runs that fail. `tests-pass` is a high-signal verifier — it perfectly separates converged from failed runs. You change the boundary, tighten the prompt, run again.

---

## How a field works

A field is a declared search space. The six primitives:

```toml
name = "my-field"
goal  = "..."            # initial prompt — conditions the model's distribution at step 0

[model]
name = "claude-sonnet-4-6"

[environment]
root = "./workspace"     # territory — what the agent can observe

[boundary]
allow_write = ["*.py"]   # topology — makes unauthorized trajectories impossible
max_steps = 20
max_cost = "$1.00"

[[verifiers]]
name = "tests-pass"
command = "pytest"       # reward signal — output enters the context window
trigger = "on_stop"

[context]
max_tokens = 50000       # hard ceiling — when exhausted, the run terminates
```

The runtime executes an 8-step loop: re-observe → invoke model → boundary check → dispatch → run verifiers → budget check → record → check termination. Every run produces a trajectory log in `~/.portlang/trajectories/`.

---

## Architecture

```
crates/
  portlang-core/               # Field, Action, Cost, Trajectory types
  portlang-config/             # TOML parser (strict — unknown keys are errors)
  portlang-runtime/            # Agent loop, sandbox, tool dispatch
  portlang-trajectory/         # Storage, replay, diff, query
  portlang-adapt/              # Statistical analysis, adaptation reports
  portlang-provider-anthropic/ # Anthropic API client
  portlang-provider-openrouter/# OpenRouter client (100+ models)
  portlang-cli/                # CLI — run, eval, converge, report, replay, diff
```

---

## Testing

```bash
cargo test --workspace
```
