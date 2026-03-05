# portlang

An environment-first agent framework.

Agent behavior is search. The developer's job is engineering the search space, not the searcher.

---

## How to read this document

This document serves two purposes. Sections I through III describe the theory and design principles. Sections IV through X describe the implementation. The theory sections exist so that every implementation decision can be traced back to a reason. If a design question comes up during development, the answer should be derivable from the principles.

---

## I. The Problem

The current generation of agent-building frameworks (LangChain, CrewAI, AutoGen, Semantic Kernel) are loop orchestrators. They manage the turn-taking between a language model and a set of tools: call the model, parse the output, dispatch a tool call, append the result, repeat. The core abstraction is always the same: the agent is the unit of work, and the framework's job is to run the agent's loop.

This rests on an implicit mental model: that the agent is reasoning its way through a task, and the developer's job is to give it good instructions. The developer's work becomes prompt engineering.

This mental model stops being useful when tasks get hard. When tasks are short (under ten steps), the loop-orchestration model works adequately. But as tasks grow longer (multi-hour autonomous runs, overnight builds, complex multi-file refactors) the model degrades in ways that better prompts cannot fix. The context window fills with noise. Early observations compound into persistent drift. The agent optimizes for whatever signal is easiest to chase. Shortcuts get exploited. The map goes stale while the territory keeps changing.

These are structural consequences of how these systems work, consequences that follow from the mathematics of pre-training and reinforcement learning. They require structural solutions, not better prompts.

portlang is built on a different premise: agent behavior is search, and the developer's primary job is engineering the search space, not the searcher.


## II. Theoretical Foundations

Two results are load-bearing for every design decision.

### The Prompt as Conditional Distribution

A pre-trained language model learns a distribution over token sequences. Given a prefix `c = (x₁, x₂, …, xₖ)`, the model generates continuations by sampling from `P(xₖ₊₁, xₖ₊₂, … | c)`. The prompt is a conditioning variable. It selects which region of the model's learned distribution we sample from.

Three consequences matter for framework design:

**Sensitivity.** Atomic formatting changes, even a single token difference, can cause accuracy swings of up to 76 percentage points (Sclar et al., 2023). Different prompts create different distributions, which create different outputs. This is conditional probability working as specified, not model fragility.

**Narrowing.** More tokens on a topic further constrain the reachable output space. Each token in the context window participates in determining what comes next.

**No persistence.** The model has no memory beyond the context window, no persistent state, no independent knowledge retrieval. The weights are static. Whatever the model "knows" is mediated entirely through the tokens it can see right now.

### Reinforcement Learning as Search

Pre-training determines what the model can produce. Reinforcement learning determines what it will produce. In the RL formulation, the model becomes a policy `πθ` operating in an environment where the state is the current context window, the action is the model's output, the transition is the environment's response appended to context, and the reward is a signal indicating action quality.

The policy is optimized to maximize expected cumulative reward over trajectories. For coding agents, the reward is typically a verifier: did tests pass, is the code syntactically valid, did the linter pass.

Three properties matter:

**Reward-chasing.** The model maximizes whatever proxy was used to define the reward function. Whatever that proxy measured becomes the model's de facto objective. Whatever it left unmeasured remains open space the search can wander into.

**Proxy mismatch.** The reward function measures what the model provider could measure, not what you want. METR has documented models tracing through Python call stacks to find pre-computed answers in a scoring system's memory. The search follows the path of least resistance to reward.

**Policy opacity.** You did not design the reward function, you do not know its full specification, and you can only observe the behavior it produces. The trained policy is the one thing you cannot change at runtime.

### The Joint Inference

At inference time, the prompt conditions the distribution (narrowing what's reachable), and the policy navigates that conditioned space (selecting trajectories that maximize reward). The full trajectory is a sequence of `(state, action, feedback)` tuples, where each state is the accumulated context window.

This is what we mean when we say agents are searching, not thinking. Chain-of-thought tokens, planning steps, and internal deliberation are part of the trajectory, not separate from it. They enter the context window and re-condition the policy's next action like any other token.


## III. Design Principles

Each principle is a direct consequence of the theoretical foundations. They are design constraints that every component of the framework must satisfy.

**Principle 1: The environment is the product, not the agent.** The trained policy is opaque and approximately the same for everyone. The only variables under the developer's control are the context window and the environment. The agent is a parameter you pass in, not a thing you build.

**Principle 2: Declare convergence criteria, don't script behavior.** The trajectory is non-deterministic. The framework lets developers declare what success looks like (verifiers), what the agent can observe (environment), and what the agent cannot do (boundaries). The agent finds the trajectory.

**Principle 3: Context is a finite resource.** Noise in the context window warps the distribution the model samples from. The framework treats the context window as a finite resource with a hard ceiling. When the ceiling is reached, the run ends. There is no magic compression. There is a budget, and when it's spent, the trajectory terminates.

**Principle 4: Boundaries are topology, not policy.** Telling an agent "don't do X" through a prompt is a suggestion. Making X physically impossible through permissions is a guarantee. The framework enforces boundaries at the runtime level, making unauthorized trajectories structurally impossible rather than discouraged.

**Principle 5: Feedback is the runtime reward signal, not a post-hoc check.** Test results, linter output, and build signals enter the context window and reshape the agent's behavior at every step. Weak feedback steers the agent toward weak solutions. The framework makes feedback a continuous component of the execution loop.

**Principle 6: Every run is a trajectory, and trajectories are data.** The framework records complete trajectories as structured event logs. Without observability at this level, you cannot reason about non-deterministic systems.

**Principle 7: The framework learns from its own runs.** Which tools get used? Which observations cause divergence? How many tokens does a typical run consume? These questions are answered by trajectory data. The framework accumulates trajectory data across runs and surfaces patterns. The framework counts, correlates, and reports. The developer decides what to change.


## IV. The Six Primitives

Every concept in the framework reduces to one of six primitives. Each corresponds to a variable in the theoretical model that the developer can control.

| Primitive | Definition | Theoretical Basis |
|-----------|-----------|-------------------|
| **Field** | A declared search space. The unit of work. Contains an environment, verifiers, context policy, and boundary. | The conditioned distribution at step t: the reachable behavior space given the current context window and trained policy. |
| **Environment** | The territory. Filesystem snapshot, available tools, network policy. Immutable except through explicitly allowed mutations. Versioned and composable. | The state space and transition function. Determines what observations can enter the context window and what trajectories are physically reachable. |
| **Verifier** | A function that returns pass, fail, or a numeric score. The runtime reward signal. Results enter the context window automatically. | The inference-time reward signal `R(sₜ, aₜ)`. Shapes the field by providing feedback the policy was trained to respond to. |
| **Context Policy** | A hard token budget and a re-observation schedule. When the budget is exhausted, the run terminates. | Direct constraint on `sₜ`. Prevents unbounded context growth that causes drift. |
| **Boundary** | Hard walls on the search space. Scoped filesystem access, network egress policy, tool restrictions, cost and step limits. Enforced by the sandbox. | Eliminates trajectories from the reachable space. Removes entries from the action space entirely, rather than discouraging them through the context window. |
| **Trajectory** | The recorded event log of (state, action, feedback) tuples. Structured, queryable. Every run produces one. | The complete episode `τ = (s₀, a₀, s₁, a₁, …, sₜ)`. The unit of observability. |

## V. The Configuration Layer

A field is defined in `field.toml`, using TOML for the same reason Cargo does: human-readable, diff-friendly, clear structure-to-semantics mapping.

```toml
[field]
name = "fix-jwt-validation"
model = "claude-sonnet-4-5"
prompt = """
Fix the JWT expiration validation bug in auth.py.
The exp claim is being compared as a string instead of
an integer. Only modify auth.py and the test file.
"""

[environment]
snapshot = "git:HEAD"
tools = ["read", "write", "bash"]
ephemeral = true

[[verifier]]
name = "tests-pass"
run = "pytest tests/test_auth.py -x"
when = "after_each_write"

[[verifier]]
name = "scope-guard"
run = "git diff --name-only | grep -qvE '^(auth\\.py|tests/)' && exit 1 || exit 0"
when = "before_terminal"

[context]
budget = 32_000
re_observe = ["git diff --stat"]

[boundary]
fs_write = ["auth.py", "tests/**"]
network = "deny_all"
max_steps = 30
max_cost = "$2.00"
sandbox = "container"
```

The developer declares a search space: what the agent can see (environment), what success looks like (verifiers), how much context is available (budget), and what is physically impossible (boundary). The runtime executes the search.

### Configuration semantics

`[field]` declares metadata and the initial conditioning: the prompt and model. The prompt enters the context window at `s₀` and persists throughout. The model is the policy `πθ`.

`[environment]` declares the territory. The snapshot specifies what exists on disk. The tool list specifies what system calls exist. Ephemeral environments are destroyed after the run.

`[[verifier]]` declares reward signals. Each verifier has a name, a command, and a trigger condition. The runtime dispatches verifiers at the specified points and injects their output into the context window. Multiple verifiers compose conjunctively: all must pass for convergence.

`[context]` declares the token budget and re-observation schedule. The budget is a hard ceiling. When reached, the run terminates. The re-observation schedule keeps the context fresh but consumes budget. There is no hidden summarization.

`[boundary]` declares hard walls. Filesystem write permissions are glob patterns. Network policy is enforced at the sandbox level. Cost and step limits are circuit breakers. The `sandbox` field specifies the enforcement mechanism (container isolation for fields that include bash).

### Parsing

The `field.toml` parser should be strict: unknown keys are errors, not warnings. The configuration is the contract between the developer and the runtime.

## VI. The Runtime

The runtime turns a field definition into a trajectory.

### Sandbox architecture

Each field executes in an isolated sandbox constructed from the environment definition: the filesystem snapshot is mounted (read-only base with a copy-on-write layer for allowed mutations), the tool manifest determines which system call handlers are registered, and the network policy is enforced at the network namespace level.

### The agent loop

On each step:

1. Execute any scheduled re-observations. Inject into context window. Check token budget.
2. Invoke the policy with the current context window. Receive an action.
3. Check the action against the boundary. If it violates, reject it and inject the rejection into context. Return to step 2.
4. Dispatch the action to the sandbox. Receive the response.
5. Dispatch any triggered verifiers. Inject their results into context.
6. Check token budget. If exceeded, terminate.
7. Record the step to the trajectory log.
8. Check termination conditions (agent stopped, all verifiers pass, step limit, cost limit). If met, end. Otherwise, return to step 1.

```rust
pub async fn run_field(field: &Field, provider: &dyn ModelProvider) -> Trajectory {
    let sandbox = create_sandbox(&field.environment, &field.boundary);
    let mut context = ContextWindow::new(&field.prompt, field.context.budget_tokens);
    let mut trajectory = Trajectory::new(field);
    let mut step = 0u32;

    loop {
        // 1. Re-observe
        for cmd in &field.context.re_observe {
            let output = sandbox.run_command(cmd).await;
            context.append_observation(&output);
        }
        if context.token_count() >= field.context.budget_tokens {
            trajectory.finish(RunOutcome::BudgetExhausted);
            break;
        }

        // 2. Invoke policy
        let action = provider.complete(&context).await;

        // 3. Boundary check
        if let Err(violation) = sandbox.check_boundary(&action) {
            context.append_rejection(&violation);
            continue;
        }

        // 4. Dispatch
        let response = sandbox.dispatch(&action).await;
        context.append_response(&response);

        // 5. Verifiers
        let verifier_results = run_triggered_verifiers(
            &field.verifiers, &action, &sandbox
        ).await;
        for (name, result) in &verifier_results {
            context.append_verifier_result(name, result);
        }

        // 6. Budget check
        if context.token_count() >= field.context.budget_tokens {
            trajectory.finish(RunOutcome::BudgetExhausted);
            break;
        }

        // 7. Record
        trajectory.record_step(step, &action, &response, &verifier_results, &context);
        step += 1;

        // 8. Termination
        if action.is_stop() && all_verifiers_pass(&verifier_results) {
            trajectory.finish(RunOutcome::Converged);
            break;
        }
        if step >= field.boundary.max_steps {
            trajectory.finish(RunOutcome::StepLimitReached);
            break;
        }
        if trajectory.total_cost >= field.boundary.max_cost {
            trajectory.finish(RunOutcome::CostLimitReached);
            break;
        }
    }

    sandbox.destroy();
    trajectory
}
```


## VII. Structural Checks

Before a field is executed, `portlang check` runs structural checks on the field definition. These are limited to properties that can be verified without running inference or understanding natural language.

### What can be checked

**Uncovered mutation paths.** If the boundary allows writes to a path that no verifier observes, the agent can mutate state with no feedback signal. The checker requires every writable path to be covered by at least one verifier.

**Unreachable verifiers.** If a verifier references a file or tool not in the environment, it can never produce a signal. Error.

**Budget arithmetic.** Estimate re-observation cost (tokens per re-observation × max steps) and warn if it exceeds a significant fraction of the context budget.

**Boundary completeness.** If the boundary includes bash without specifying `sandbox = "container"`, warn. If filesystem restrictions are set but network policy is not, warn.

**Tool count.** If the tool manifest exceeds a configurable threshold, warn. The checker does not know which tools are relevant (that requires understanding the prompt). It only counts.

### What cannot be checked statically

These require either inference or historical trajectory data. The framework does not pretend otherwise.

**Prompt-tool relevance.** Determining whether a tool is relevant to a prompt requires understanding the prompt. The framework addresses this through trajectory data: after several runs, the adaptation system reports which tools were never invoked or which tools correlate with divergence.

**Optimal context budget.** The right budget depends on the task, the model, and the environment. The checker can warn about budgets exceeding known model degradation thresholds (a lookup table), but the right budget comes from observing token count distributions across runs.

**Verifier adequacy.** Whether verifiers are strong enough to prevent proxy satisfaction without intent satisfaction is a semantic question. The framework reports verifier pass rates across runs and flags cases where verifiers pass but trajectories diverge, but the judgment belongs to the developer.


## VIII. Observability, Adaptation & Debugging

Agent systems are non-deterministic. You need to reason about distributions of trajectories, not individual runs.

The observability layer has two purposes: debugging individual runs (what went wrong this time?) and adapting field definitions over time (what goes wrong in general?).

### Trajectory storage

Trajectories store incremental deltas, not full context window snapshots. A 30-step run at 32k tokens per step would produce ~960k tokens of state data if snapshotted fully. Instead, each step records what entered the context window. The full context at step N is reconstructed by replaying from step 0.

### Trajectory replay

`portlang replay <trajectory-id>` loads a trajectory and lets the developer step through it. At each step: the action taken, the environment's response, verifier results, and running token count. Context window at any step is reconstructed by replaying deltas from step 0.

### Trajectory comparison

`portlang diff <id-a> <id-b>` compares two trajectories at the structural level. It aligns by action type (which files were read, which tools were called, which verifiers fired) and identifies the first point of structural divergence.

This is structural comparison, not semantic comparison. The framework compares action types, target paths, tool names, and verifier results. If run A read `auth.py` at step 3 and run B read `database.yml` at step 3, that's a divergence point. Whether two different edits to `auth.py` are functionally equivalent requires inference and is not attempted.

### Adaptation through trajectory data

The framework accumulates trajectory data across runs of the same field and surfaces patterns as reports. The developer makes the decisions.

**Tool usage patterns.** After N runs, report which tools were never invoked and which tools correlate with divergence (runs that called tool X had a Y% convergence rate; runs that didn't had Z%).

**Budget utilization.** Report distribution of token consumption: median, p90, p99, and frequency of budget-exhaustion terminations.

**Divergence clustering.** Across many runs, identify common divergence points: "60% of failures diverge at step 5 through 8, and 80% of those involve reading a file matching *.log."

**Verifier signal quality.** Track correlation between verifier results and trajectory outcomes. If a verifier passes on 95% of steps across all runs (both successful and failing), it provides almost no signal.

None of this requires inference. It requires counting, correlating, and presenting.

### Convergence benchmarking

`portlang converge --runs 20` runs a field N times and reports convergence rate, average trajectory length, cost distribution, and divergence clusters.

`portlang eval <directory>` runs all `field.toml` files found recursively in a directory (one run each) and reports aggregate accuracy: pass rate, total/average cost, tokens, and steps across all tasks. This is the cross-task evaluation command; `converge` is the single-field reliability command.

Running a field 100 times is expensive. If each run costs $0.50, benchmarking costs $50 per iteration. The framework is honest about this. Start with 5 runs, check for structural issues, adjust, then benchmark with more. The bench command accepts a count parameter and reports confidence intervals.


## IX. Composition & Multi-Agent

Composition through fields, not through agent communication.

### Pipelines

A pipeline is a sequence of fields connected by artifacts. Each stage is a self-contained field. The output of one stage is a file (the artifact) that becomes an input to the next stage's environment.

Each field in a pipeline gets a fresh context window. The noise from stage 1's execution does not enter stage 2's context window. Only the artifact does.

```toml
# pipeline.toml
[pipeline]
name = "feature-branch"

[[stage]]
field = "./plan/field.toml"
output = "plan.md"

[[stage]]
field = "./implement/field.toml"
input = "plan.md"
parallel = 4                    # 4 independent fields, scoped by file

[[stage]]
field = "./review/field.toml"
input = ["plan.md", "git:diff"]
gate = true                     # blocks until all verifiers pass
```

### Parallel execution

Independent fields run in parallel. The framework handles sandbox creation and teardown. Parallel fields share nothing by default. If they need a shared artifact, it's through an explicit shared volume with boundary constraints.

### Gating

A gate is a verifier that blocks pipeline progression. All verifiers must pass before the pipeline advances.


## X. What This Does Not Solve

**Proxy mismatch is inherent.** Verifiers are reward proxies. The gap between "all verifiers pass" and "the result is correct" will always exist. The framework makes this gap visible, but cannot close it.

**Policy opacity remains.** The trained policy is a black box. The framework gives you every lever except the one inside the model.

**This is not a safety framework.** Boundaries eliminate trajectories, which contributes to safety. But a complete safety solution requires formal verification, adversarial testing, and runtime monitoring beyond what a developer framework provides.

**Novel tasks remain hard.** The framework works well on tasks with clear verifiers. If you cannot write a good verifier, the search will be blind.

**Coordination at scale is unsolved.** The pipeline and parallel execution patterns handle simple composition. Complex multi-agent coordination remains an open problem.

**Context management is lossy by nature.** The framework imposes a hard token budget. When exhausted, the run terminates. Developers who want summarization or pruning can build it, but the framework will not pretend that lossless context compression is a configuration option.



### Implementation phases

**Phase 1: Core loop.** `portlang-core`, `portlang-config`, `portlang-runtime` (dispatch sandbox only), `portlang-trajectory` (filesystem store), `portlang-provider-anthropic`, `portlang-cli` (run and check only). This gets a working `portlang run` for fields with structured tools (read, write, glob) and no bash.

**Phase 2: Observability.** `portlang-trajectory` (replay, diff, query), `portlang-adapt`, `portlang-cli` (replay, diff, bench). This gets trajectory analysis working.

**Phase 3: Container sandbox.** `portlang-sandbox-container`. This unlocks bash in field definitions.

**Phase 4: Composition.** `portlang-pipeline`. Pipelines, parallel execution, gating.

---

## Implementation Status Checklist

### Phase 1: Core Agent Runtime ✅ COMPLETE

- [x] Core agent loop with step-by-step execution
- [x] Structured tools: Read, Write, Glob
- [x] Boundary enforcement with glob patterns
- [x] Path escape detection and prevention
- [x] Container sandbox (AppleContainerSandbox)
  - [x] Container creation with unique IDs
  - [x] Volume mounting at /workspace
  - [x] Network isolation (--network none)
  - [x] Automatic cleanup via Drop trait
  - [x] Command execution via container exec
- [x] Dispatch sandbox (DispatchSandbox) - REMOVED, Container only
- [x] Sandbox factory with runtime detection
- [x] Verifier system
  - [x] Trigger types: Always, OnStop, OnWrite
  - [x] Shell command execution
  - [x] Result capture (stdout/stderr/exit_code)
  - [x] Integration with termination logic
- [x] Token budget tracking and enforcement
- [x] Cost budget tracking and enforcement
- [x] Step limit enforcement
- [x] Trajectory recording (complete event log)
- [x] Loop detection with helpful error messages
- [x] Environment context auto-discovery
- [x] Context window management
- [x] Re-observation commands
- [x] Configuration parsing from TOML
- [x] CLI command: `run`
- [x] CLI command: `check`
- [x] CLI command: `init`

**Provider Integrations:**
- [x] Anthropic provider (Claude models)
- [x] OpenRouter provider (multi-model support)
- [ ] OpenAI provider (Phase 5 - out of scope)

**Trajectory Storage:**
- [x] Filesystem storage backend
- [x] JSON serialization
- [x] Unique ID generation
- [x] Directory organization by field name

---

### Phase 2: Observability & Adaptation ✅ COMPLETE

- [x] Trajectory querying and filtering
  - [x] Filter by field name
  - [x] Filter by outcome (converged/failed)
  - [x] Limit results
- [x] Trajectory replay
  - [x] Interactive step-by-step
  - [x] JSON output format
  - [x] Context reconstruction
- [x] Trajectory diff
  - [x] Structural comparison
  - [x] Divergence point detection
  - [x] Action type alignment
  - [x] Text and JSON output formats
- [x] Adaptation reports
  - [x] Convergence rate calculation
  - [x] Tool usage pattern analysis
  - [x] Token/cost/step distributions
  - [x] Verifier signal quality analysis
  - [x] Percentile calculations (p90, p99)
- [x] Benchmarking
  - [x] Multi-run execution
  - [x] Progress tracking
  - [x] Aggregate statistics
- [x] CLI command: `list`
- [x] CLI command: `replay`
- [x] CLI command: `diff`
- [x] CLI command: `report`
- [x] CLI command: `converge` (formerly `benchmark` — runs one field N times, measures convergence reliability)
- [x] CLI command: `eval` (runs all field.toml files in a directory, reports aggregate accuracy across tasks)

---

### Phase 3: Container Sandbox ✅ COMPLETE

- [x] Apple Container integration
- [x] OCI-compatible image support (python:3.11-alpine)
- [x] Volume mounting
- [x] Network isolation
- [x] Hardware VM isolation per container
- [x] Sub-second startup performance
- [x] Automatic cleanup
- [x] Container availability detection
- [x] Init command for setup guidance
- [x] Fallback error handling - REMOVED, fail fast instead

**Container-Only Mode:**
- [x] Removed SandboxKind enum
- [x] Removed dispatch sandbox option
- [x] Always require container sandbox
- [x] Fail with clear error if unavailable

---

### Phase 4: Composition & Pipelines ❌ NOT IMPLEMENTED

- [ ] Pipeline definition in TOML
- [ ] Stage sequencing
- [ ] Artifact passing between stages
- [ ] Parallel field execution
- [ ] Gating (block until verifiers pass)
- [ ] Shared volume support

**Status:** Deferred - not needed for current use cases

---


## Feature Completeness Summary

| Category | Status | Notes |
|----------|--------|-------|
| **Core Runtime** | ✅ 100% | All features complete |
| **Sandbox Isolation** | ✅ 100% | Container-only, Apple Containerization |
| **Boundary Enforcement** | ✅ 100% | Path checking, network isolation |
| **Verifier System** | ✅ 100% | All trigger types working |
| **Budget Management** | ✅ 100% | Token, cost, step limits |
| **Trajectory Recording** | ✅ 100% | Complete event logs |
| **Storage & Querying** | ✅ 100% | Filesystem backend with filters |
| **Observability** | ✅ 100% | Replay, diff, reports |
| **Adaptation** | ✅ 100% | Statistical analysis |
| **CLI** | ✅ 100% | 9 commands: run, check, init, list, replay, diff, report, converge, eval |
| **Providers** | ✅ 66% | Anthropic + OpenRouter (OpenAI deferred) |
| **Tool Extensibility** | ✅ 50% | Shell tools complete, Python/Rust planned |
| **Pipelines** | ❌ 0% | Not implemented (Phase 4) |
| **MCP Integration** | ❌ 0% | Not implemented (Phase 3.5) |
| **Skills System** | ❌ 0% | Not implemented (Phase 3.5) |

---

## Current Focus Areas

1. **Production Deployment**: System is ready for real-world use
2. **Custom Tool Support**: Users can define shell-based tools without modifying core
3. **Documentation**: README.md and CUSTOM_TOOLS.md cover setup and usage
4. **Testing**: Integration tests pass, demo fields work
5. **Performance**: Container startup <1s, cleanup automatic

## Recent Additions

### Custom Tool System (Phase 3.5 - March 2026)
- **Extensible tool architecture**: Replace hardcoded 3-tool enum with registry-based system
- **Shell command tools**: Define custom tools using shell commands with template substitution
- **Runtime registration**: Tools registered dynamically from field.toml configuration
- **Security**: Parameter escaping prevents shell injection
- **Future-ready**: Foundation for Python/Rust/HTTP tools


### MCP (Model Context Protocol) Integration ❌ NOT IMPLEMENTED

**What is MCP:**
- Anthropic's standard protocol for connecting LLMs to data sources
- Allows tools, resources, and prompts to be exposed via servers
- Standard interface: stdio, HTTP, or SSE

**Needed:**
- [ ] MCP client implementation
- [ ] MCP server discovery
- [ ] Tool conversion (MCP tools → portlang tools)
- [ ] Resource access (MCP resources → context window)
- [ ] Prompt templates from MCP servers

**Example:**
```toml
[environment]
mcp_servers = [
    { name = "filesystem", command = "npx", args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"] },
    { name = "github", command = "mcp-server-github", env = { GITHUB_TOKEN = "..." } },
]
```

**Reference:** https://modelcontextprotocol.io/

---

### Skills System ❌ NOT IMPLEMENTED

**What are Skills:**
- Reusable, composable capabilities
- Higher-level than tools (can invoke multiple tools)
- Parameterized and typed
- Examples: "code review", "test generation", "refactoring"

**Needed:**
- [ ] Skill definition format
- [ ] Skill composition (skills can call other skills)
- [ ] Skill library/registry
- [ ] Skill parameter validation
- [ ] Skill result schemas
- [ ] Built-in skill library

**Example:**
```toml
[[skill]]
name = "test-generator"
description = "Generate pytest tests for Python functions"
parameters = { file_path = "string", function_name = "string" }
implementation = "./skills/test_generator.py"

[[skill]]
name = "code-reviewer"
description = "Review code changes and suggest improvements"
parameters = { diff = "string" }
implementation = "./skills/code_reviewer.py"
```

---

### Structured Output ❌ NOT IMPLEMENTED

**What is Structured Output:**
- Type-safe, validated responses from the agent
- JSON Schema validation
- Pydantic-like models for outputs
- Ensures agents return data in expected formats

**Needed:**
- [ ] Output schema definition in field.toml
- [ ] Schema validation on agent responses
- [ ] Retry logic when schema validation fails
- [ ] Structured output in trajectory
- [ ] Type generation from schemas (optional)

**Example:**
```toml
[output_schema]
type = "object"
required = ["status", "changes"]
properties.status = { type = "string", enum = ["success", "failure"] }
properties.changes = { type = "array", items = { type = "string" } }
properties.reasoning = { type = "string" }

# Agent must return JSON matching this schema when it stops
```

### Multi-Agent Coordination ❌ NOT IMPLEMENTED

**Current State:**
- Only single-agent execution
- Pipelines (Phase 4) will allow sequential composition
- No parallel agent coordination
- No inter-agent communication

**Needed for Multi-Agent:**
- [ ] Agent Communication Protocol (ACP?)
- [ ] Shared state management
- [ ] Message passing between agents
- [ ] Coordination patterns:
  - [ ] Hierarchical (supervisor → workers)
  - [ ] Peer-to-peer (agents negotiate)
  - [ ] Market-based (agents bid on tasks)
- [ ] Consensus mechanisms
- [ ] Conflict resolution

**Note:** This overlaps with Phase 4 (Pipelines) but goes beyond sequential composition.

**Example:**
```toml
# Multi-agent field
[agents]
coordinator = { field = "./coordinator.toml", role = "supervisor" }
implementer1 = { field = "./implementer.toml", role = "worker" }
implementer2 = { field = "./implementer.toml", role = "worker" }
reviewer = { field = "./reviewer.toml", role = "validator" }

[coordination]
pattern = "hierarchical"
max_rounds = 5
consensus_required = 0.66  # 66% of agents must agree
```

---

## Revised Implementation Roadmap

### Phases

1. **Phase 1: Core Runtime** - ✅ COMPLETE
2. **Phase 2: Observability** - ✅ COMPLETE
3. **Phase 3: Container Sandbox** - ✅ COMPLETE
4. **Phase 3.5: Extensibility & Integration** - ⚠️ IN PROGRESS
   - ✅ Tool system extensibility (shell-based custom tools)
   - ❌ MCP integration
   - ❌ Skills system
   - ❌ Structured output validation
5. **Phase 4: Composition & Multi-Agent** - ❌ NOT STARTED
   - Pipelines (sequential composition)
   - Parallel execution
   - Multi-agent coordination
   - Gating and consensus