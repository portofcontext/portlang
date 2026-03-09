# portlang

An environment-first agent framework.

Agent behavior is search. The developer's job is engineering the search space, not the searcher.

---

# PART I: THEORY & DESIGN

This part describes the theoretical foundations and design principles. Every implementation decision traces back to these.

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

Current syntax (as implemented):

```toml
name = "my-task"
goal = "Create hello.py that prints 'Hello, World!'"

[model]
name = "anthropic/claude-sonnet-4.6"
max_tokens = 4000

[environment]
type = "local"
root = "./workspace"

[boundary]
allow_write = ["hello.py"]
network = "deny"

[context]
max_tokens = 80000
max_cost = "$1.00"
max_steps = 10

[[verifiers]]
name = "works"
command = "python hello.py 2>&1 | grep -q 'Hello, World!'"
trigger = "on_stop"
description = "Must print 'Hello, World!'"
```

The developer declares a search space: what the agent can see (environment), what success looks like (verifiers), how much context is available (budget), and what is physically impossible (boundary). The runtime executes the search.

### Configuration semantics

**Top-level fields:**
- `name`: Field identifier (used in trajectory storage)
- `goal`: Initial prompt that enters context window at step 0
- `description`: Optional metadata
- `re_observation`: Commands that run before each step to keep context fresh

**[model]:**
- `name`: Model identifier (e.g., `anthropic/claude-sonnet-4.6`)
- `temperature`: Sampling temperature (default 1.0)
- `max_tokens`: Max tokens per API call (not total budget)

**[environment]:**
- `type`: Currently only "local" supported
- `root`: Working directory for agent (maps to /workspace in container)

**[boundary]:**
- `allow_write`: Glob patterns for writable files
- `allow_read`: Optional glob patterns for readable files
- `network`: "deny" or "allow"
- Built-in tools are always available (read, write, glob)

**[context]:**
- `max_tokens`: Hard ceiling on total tokens across entire run
- `max_cost`: Hard ceiling on total cost
- `max_steps`: Hard ceiling on step count

**[[verifiers]]:**
- `name`: Identifier
- `command`: Shell command to run
- `trigger`: When to run ("on_stop" currently implemented)
- `description`: Injected into context on failure

**[[tool]]:**
- `type`: "shell", "python", or "mcp"
- `script`: Path to script (for shell/python)
- `command`/`args`: For MCP servers

### Parsing

The `field.toml` parser is strict: unknown keys are errors. The configuration is the contract between developer and runtime.

## VI. The Runtime

The runtime turns a field definition into a trajectory.

### Sandbox architecture

Each field executes in an isolated Apple Container sandbox:
- Filesystem: Copy-on-write layer over read-only base, scoped to boundary
- Network: Isolated namespace, deny by default
- Tools: Only explicitly allowed tools are available
- Container lifecycle: Created on run start, destroyed on completion

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

Core implementation is in `portlang-runtime/src/loop_runner.rs`.

## VII. Structural Checks

Before a field is executed, `portlang check` runs structural checks on the field definition. These are limited to properties that can be verified without running inference or understanding natural language.

### What can be checked

**Uncovered mutation paths.** If the boundary allows writes to a path that no verifier observes, the agent can mutate state with no feedback signal. The checker requires every writable path to be covered by at least one verifier.

**Unreachable verifiers.** If a verifier references a file or tool not in the environment, it can never produce a signal. Error.

**Budget arithmetic.** Estimate re-observation cost (tokens per re-observation × max steps) and warn if it exceeds a significant fraction of the context budget.

**Boundary completeness.** If filesystem restrictions are set but network policy is not, warn.

**Tool count.** If the tool manifest exceeds a configurable threshold, warn.

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

Stored in: `~/.portlang/trajectories/<field-name>/<id>.json`

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

`portlang converge <field> -n N` runs a field N times and reports convergence rate, average trajectory length, cost distribution, and divergence clusters.

`portlang eval <directory>` runs all `field.toml` files found recursively in a directory (one run each) and reports aggregate accuracy: pass rate, total/average cost, tokens, and steps across all tasks. This is the cross-task evaluation command; `converge` is the single-field reliability command.

Running a field 100 times is expensive. If each run costs $0.50, benchmarking costs $50 per iteration. The framework is honest about this. Start with 5 runs, check for structural issues, adjust, then benchmark with more.


## IX. Composition & Multi-Agent

**Status: Not implemented**

Composition through fields, not through agent communication.

### Pipelines

A pipeline is a sequence of fields connected by artifacts. Each stage is a self-contained field. The output of one stage is a file (the artifact) that becomes an input to the next stage's environment.

Each field in a pipeline gets a fresh context window. The noise from stage 1's execution does not enter stage 2's context window. Only the artifact does.

Proposed syntax:

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

---

# PART II: IMPLEMENTATION STATUS

This part tracks what's implemented, what's in progress, and what needs to be built.

---

## Implementation Roadmap

### Phase 1: Core Runtime ✅ COMPLETE

**Goal:** Working `portlang run` for fields with structured tools (read, write, glob), verifiers, and boundaries.

**Components:**
- Core agent loop with step-by-step execution
- Structured tools: Read, Write, Glob
- Boundary enforcement with glob patterns
- Path escape detection and prevention
- Verifier system
  - Trigger types: Always, OnStop, OnWrite (only OnStop currently used)
  - Shell command execution
  - Result capture (stdout/stderr/exit_code)
  - Integration with termination logic
- Token budget tracking and enforcement
- Cost budget tracking and enforcement
- Step limit enforcement
- Trajectory recording (complete event log)
- Loop detection with helpful error messages
- Environment context auto-discovery
- Context window management
- Re-observation commands
- Configuration parsing from TOML
- CLI commands: `run`, `check`, `init`

**Provider Integrations:**
- Anthropic provider (Claude models via direct API)
- OpenRouter provider (multi-model support)

**Trajectory Storage:**
- Filesystem storage backend
- JSON serialization
- Unique ID generation (timestamp + short hash)
- Directory organization by field name

**Status:** All features complete and stable.

---

### Phase 2: Observability & Adaptation ✅ COMPLETE

**Goal:** Trajectory analysis, debugging tools, and adaptation reports.

**Components:**
- Trajectory querying and filtering
  - Filter by field name
  - Filter by outcome (converged/failed)
  - Limit results
- Trajectory replay
  - Interactive step-by-step navigation
  - JSON output format
  - Context reconstruction at any step
- Trajectory diff
  - Structural comparison of two runs
  - Divergence point detection
  - Action type alignment
  - Text and JSON output formats
- Adaptation reports
  - Convergence rate calculation
  - Tool usage pattern analysis
  - Token/cost/step distributions (median, p90, p99)
  - Verifier signal quality analysis
  - Percentile calculations
- Benchmarking
  - Multi-run execution
  - Progress tracking
  - Aggregate statistics
- CLI commands: `list`, `replay`, `diff`, `report`, `converge`, `eval`

**Status:** All features complete. `replay` has known bug (infinite loop at end).

---

### Phase 3: Container Sandbox ✅ COMPLETE

**Goal:** Hardware-isolated execution via Apple Container.

**Components:**
- Apple Container integration
- OCI-compatible image support
- Volume mounting at /workspace
- Network isolation (--network none)
- Hardware VM isolation per container
- Sub-second startup performance (<1s)
- Automatic cleanup via Drop trait
- Container availability detection
- Init command for setup guidance

**Platform support:**
- macOS only (Apple Container is macOS-specific)
- Linux support deferred (would use different container runtime)

**Status:** Complete and stable. All fields run in containers.

---

### Phase 4: Tool Extensibility ⚠️ IN PROGRESS

**Goal:** Allow users to define custom tools without modifying core.

**Custom Tool Types:**

1. **Shell Tools** ✅ COMPLETE
   - Define via `[[tool]]` with `type = "shell"` and `script` path
   - Script receives parameters as arguments, outputs JSON to stdout
   - Executable permission required
   - Example: word_count, file_copy, http_get

2. **Python Tools** ✅ COMPLETE
   - Define via `[[tool]]` with `type = "python"` and `script` path
   - Function parameters with type hints are automatically converted to JSON Schema
   - Docstrings provide descriptions for the tool and parameters
   - PEP 723 inline dependencies support (`# /// script` block)
   - Automatic venv creation and dependency installation
   - Example: data_processor, json_validator, text_analyzer

3. **MCP Servers** ✅ 80% COMPLETE
   - Define via `[[tool]]` with `type = "mcp"`
   - Supports stdio and HTTP/SSE transports
   - Environment variable substitution for secrets
   - Example: filesystem, github, postgres
   - **Missing:** Full resource and prompt support (only tools currently exposed)

**Documentation:**
- ✅ CUSTOM_TOOLS.md created
- ✅ Example scripts in examples/
- ❌ Need better error messages for tool failures
- ❌ Need tool debugging workflow documentation

**Status:** Core functionality complete. Need polish and better docs.

---

### Phase 5: Code Mode ⚠️ 80% COMPLETE

**Goal:** Allow agents to write and execute code that processes data outside the context window.

**What is Code Mode:**
- Agent writes TypeScript code that calls tools
- Code executes in sandbox, results summarized in context
- Bypasses token limit for large data processing
- Tools defined in Python are exposed as TypeScript functions

**Implementation:**
- ✅ Basic code execution engine
- ✅ Tool exposure to TypeScript runtime
- ✅ Sandboxed execution
- ✅ Result capture and summarization
- ❌ Type generation for tools (currently manual)
- ❌ Better error handling for code failures
- ❌ Code caching for repeated operations

**Configuration:**
```toml
[code_mode]
enabled = true

[[tool]]
type = "python"
script = "./tools/data_tools.py"  # Exposed as TypeScript functions
```

**Status:** Works but rough edges. Needs type generation and better error messages.

---

### Phase 6: AI Ecosystem Integration

**Goal:** Make portlang easy to learn and integrate with AI coding tools.

**Skills System (External)** ✅ COMPLETE
- Created portlang skill for Claude Code
- Install: `npx skills add https://github.com/portofcontext/skills --skill portlang`
- Covers: field creation, verifiers, debugging, convergence testing
- Reference docs: verifier patterns, custom tools, trajectory analysis, field recipes

**MCP Integration** ✅ 80% COMPLETE
- MCP servers work as custom tools
- stdio transport: ✅ Complete
- HTTP/SSE transport: ✅ Complete
- Tool exposure: ✅ Complete
- Resource exposure: ❌ Not implemented
- Prompt exposure: ❌ Not implemented
- **Missing:** Full MCP spec support beyond tools

**Status:** Skills complete. MCP partial (tools only, no resources/prompts).

---

## Future Phases (Not Started)

### Phase 7: Pipelines & Composition ❌ NOT STARTED

**Goal:** Sequential and parallel field composition.

**Needed:**
- Pipeline configuration format (`pipeline.toml`)
- Artifact passing between stages
- Fresh context windows per stage
- Parallel field execution
- Gating (block until all verifiers pass)
- Shared state management for parallel fields

**Design questions:**
- How to handle failures in pipeline stages?
- Should pipelines be fields themselves (recursive)?
- How to visualize pipeline execution?

**Estimated scope:** 2-3 weeks of implementation.

---

### Phase 8: Multi-Agent Coordination ❌ NOT STARTED

**Goal:** Multiple agents working on same task with coordination.

**Approaches under consideration:**
1. **Hierarchical** - Supervisor agent delegates to worker agents
2. **Peer-to-peer** - Agents negotiate and coordinate directly
3. **Market-based** - Agents bid on subtasks

**Needed:**
- Agent communication protocol
- Shared state management
- Message passing primitives
- Consensus mechanisms
- Conflict resolution

**Design questions:**
- Is this actually needed? Pipelines might be sufficient.
- How to avoid coordination overhead dominating task time?
- How to handle agent disagreements?

**Status:** Design exploration phase. May defer indefinitely.

---

### Phase 9: Structured Output ✅ COMPLETE

**Goal:** Type-safe, validated responses from agents.

**Implementation:**
- ✅ JSON Schema validation using `jsonschema` crate
- ✅ Automatic `submit_output` tool when `[output_schema]` is defined
- ✅ Real-time validation with error feedback to agent
- ✅ Output written to `/workspace/output.json` for verifiers
- ✅ Schema and output stored in trajectory
- ✅ HTML viewer shows both schema and agent output
- ✅ jq included in default container for easy JSON verification
- ✅ Works without special goal text (automatic system prompt injection)

**Syntax:**
```toml
output_schema = '''
{
  "type": "object",
  "required": ["status", "changes"],
  "properties": {
    "status": {"type": "string", "enum": ["success", "failure"]},
    "changes": {"type": "array", "items": {"type": "string"}}
  }
}
'''
```

**Verifiers become simple:**
```toml
[[verifiers]]
name = "status-success"
command = "jq -e '.status == \"success\"' /workspace/output.json"
```

**Status:** Complete. Makes verifiers much simpler to write.

---

## Current Focus

**Production readiness:**
- System is stable for real-world use
- All core features (Phases 1-3) complete
- Tool extensibility (Phase 4) functional but needs polish

**Next priorities:**
1. Fix `replay` infinite loop bug
2. Better error messages for tool failures
3. Code Mode type generation
4. MCP resource/prompt support
5. Decide: Pipelines (Phase 7) or Multi-Agent (Phase 8)?

**Recently completed:**
- ✅ Structured Output (Phase 9) - makes verifiers much simpler

**Deferred:**
- Multi-Agent (might not be necessary)
- Linux support (would require different container runtime)

---

## Feature Completeness

| Category | Status | Notes |
|----------|--------|-------|
| **Core Runtime** | ✅ 100% | All features complete |
| **Sandbox Isolation** | ✅ 100% | Container-only, Apple Container (macOS) |
| **Boundary Enforcement** | ✅ 100% | Path checking, network isolation |
| **Verifier System** | ✅ 100% | OnStop trigger working, Always/OnWrite not used |
| **Budget Management** | ✅ 100% | Token, cost, step limits enforced |
| **Trajectory Recording** | ✅ 100% | Complete event logs, delta-based storage |
| **Observability** | ✅ 100% | Replay, diff, reports, convergence testing |
| **CLI** | ✅ 100% | 9 commands working |
| **Providers** | ✅ 40% | Anthropic + OpenRouter (OpenAI deferred) |
| **Custom Tools** | ✅ 80% | Shell, Python, MCP (needs polish) |
| **Code Mode** | ✅ 80% | Works but needs type generation |
| **MCP Integration** | ✅ 80% | Tools only, no resources/prompts |
| **Skills** | ✅ 100% | External portlang skill complete |
| **Structured Output** | ✅ 100% | Complete with schema validation, submit_output tool |
| **Pipelines** | ❌ 0% | Not implemented |
| **Multi-Agent** | ❌ 0% | Not implemented, may not be needed |

---

## Known Issues

1. **`portlang replay` infinite loop** - Replay gets stuck at end of trajectory, repeating "Already at the end" forever. Need to fix exit condition.

2. **Tool error messages unclear** - When a custom tool fails, error message doesn't clearly indicate which tool or why. Need better error context.

3. **MCP resource/prompt support missing** - MCP servers can expose resources (data sources) and prompts (templates), but we only expose tools currently.

4. **No tool debugging workflow** - Hard to test custom tools in isolation. Should have `portlang test-tool <script>` command.

5. **Code Mode type generation manual** - Python tools aren't automatically translated to TypeScript types. Developer has to write them manually.

6. **No Windows/Linux support** - Relies on Apple Container (macOS only). Would need different container runtime for cross-platform.

---

## Design Decisions Log

**Why container-only sandbox?**
- Originally had DispatchSandbox (no isolation) as fallback
- Removed it: partial isolation is worse than none
- Forces honest conversation about security boundaries
- Apple Container is fast enough (<1s startup) that overhead is acceptable

**Why TOML for configuration?**
- Human-readable, diff-friendly
- Strict parsing (unknown keys are errors)
- Same reasoning as Cargo

**Why trajectory deltas instead of snapshots?**
- 30-step run at 32k tokens/step = 960k tokens if fully snapshotted
- Deltas are 10-100x smaller
- Replay reconstructs full context by replaying deltas from step 0

**Why no prompt compression/summarization?**
- Lossy compression changes the distribution
- Framework doesn't pretend lossless compression exists
- Developer sets hard token budget, run terminates when exceeded
- Honest about trade-offs

**Why verifiers inject into context, not just pass/fail?**
- Verifier output is the reward signal
- Agent sees *why* it failed, not just that it failed
- Steers behavior at runtime, not just terminal verdict
- Aligns with RL theory (R(s,a) shapes policy)

**Why glob patterns for boundaries?**
- Familiar syntax (gitignore, .dockerignore)
- Expressive enough for most use cases
- Enforced at runtime by sandbox, not suggestions

**Why no multi-agent by default?**
- Pipelines handle most composition needs
- Multi-agent coordination overhead often exceeds benefit
- Unclear what problems it solves that pipelines don't
- May implement later if clear use case emerges
