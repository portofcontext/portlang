# Command-Line Help for `portlang`

This document contains the help content for the `portlang` command-line program.

**Command Overview:**

* [`portlang`↴](#portlang)
* [`portlang new`↴](#portlang-new)
* [`portlang init`↴](#portlang-init)
* [`portlang run`↴](#portlang-run)
* [`portlang check`↴](#portlang-check)
* [`portlang converge`↴](#portlang-converge)
* [`portlang eval`↴](#portlang-eval)
* [`portlang list`↴](#portlang-list)
* [`portlang replay`↴](#portlang-replay)
* [`portlang diff`↴](#portlang-diff)
* [`portlang report`↴](#portlang-report)
* [`portlang view`↴](#portlang-view)
* [`portlang view trajectory`↴](#portlang-view-trajectory)
* [`portlang view eval`↴](#portlang-view-eval)
* [`portlang view diff`↴](#portlang-view-diff)
* [`portlang view field`↴](#portlang-view-field)
* [`portlang docs`↴](#portlang-docs)

## `portlang`

portlang - agent runtime with structured tools and verifiers

**Usage:** `portlang <COMMAND>`

###### **Subcommands:**

* `new` — Create a new field.toml
* `init` — Initialize and check portlang environment
* `run` — Run a field
* `check` — Check a field for errors
* `converge` — Run a field N times and measure convergence reliability
* `eval` — Run all fields in a directory and report aggregate accuracy
* `list` — List trajectories
* `replay` — Replay a trajectory step-by-step
* `diff` — Compare two trajectories
* `report` — Generate an adaptation report from existing trajectories
* `view` — View evals and trajectories as interactive HTML
* `docs` — Print CLI reference documentation as Markdown



## `portlang new`

Create a new field.toml

**Usage:** `portlang new [OPTIONS] [PATH]`

###### **Arguments:**

* `<PATH>` — Output path (file or directory); defaults to ./field.toml

###### **Options:**

* `--interactive` — Walk through field creation step by step
* `--name <NAME>` — Field name (required without --interactive)
* `--description <DESCRIPTION>` — Human-readable description of the field
* `--model <MODEL>` — Model identifier, e.g. "anthropic/claude-sonnet-4.6" or "openai/gpt-4o"

  Default value: `anthropic/claude-sonnet-4.6`
* `--temperature <TEMPERATURE>` — Sampling temperature 0.0–1.0

  Default value: `1.0`
* `--goal <GOAL>` — Agent goal / initial task prompt (required without --interactive)
* `--system <SYSTEM>` — System prompt prepended to every agent interaction
* `--re-observation <RE_OBSERVATION>` — Command run before each step to refresh agent context (repeatable)
* `--package <PACKAGE>` — APT packages to install in the container (repeatable; use "uv" to install uv via pip)
* `--allow-write <ALLOW_WRITE>` — Glob pattern the agent may write to (repeatable, e.g. --allow-write "*.txt")
* `--network <NETWORK>` — Network access policy: "allow" or "deny"

  Default value: `allow`
* `--max-steps <MAX_STEPS>` — Hard ceiling on total agent steps

  Default value: `20`
* `--max-cost <MAX_COST>` — Hard ceiling on total cost, e.g. "$1.00"

  Default value: `$1.00`
* `--max-tokens <MAX_TOKENS>` — Hard ceiling on total context tokens
* `--tool <TOOL>` — Tool definition as JSON (repeatable).

   Python tool: --tool '{"type":"python","file":"./tools/calc.py","function":"execute"}' Optional: "name", "description" (override auto-extracted values)

   Shell tool: --tool '{"type":"shell","name":"run_sql","description":"Run a SQL query","command":"sqlite3 db.sqlite"}'

   MCP tool (stdio): --tool '{"type":"mcp","name":"stripe","command":"npx","args":["-y","@stripe/mcp"],"env":{"STRIPE_SECRET_KEY":"${STRIPE_SECRET_KEY}"}}'

   MCP tool (http/sse): --tool '{"type":"mcp","name":"myserver","url":"https://example.com/mcp","headers":{"Authorization":"Bearer ${TOKEN}"},"transport":"sse"}'
* `--verifier <VERIFIER>` — Verifier definition as JSON (repeatable).

   Example: --verifier '{"name":"check-file","command":"test -f result.txt","trigger":"on_stop","description":"result.txt must exist"}'

   trigger: "on_stop" | "always" | "on_write" (default: "on_stop")



## `portlang init`

Initialize and check portlang environment

**Usage:** `portlang init [OPTIONS]`

###### **Options:**

* `--install` — Automatically download and install Apple Container
* `--start` — Start the container system service



## `portlang run`

Run a field

**Usage:** `portlang run <FIELD_PATH>`

###### **Arguments:**

* `<FIELD_PATH>` — Path to the field TOML file



## `portlang check`

Check a field for errors

**Usage:** `portlang check <FIELD_PATH>`

###### **Arguments:**

* `<FIELD_PATH>` — Path to the field TOML file



## `portlang converge`

Run a field N times and measure convergence reliability

**Usage:** `portlang converge [OPTIONS] <FIELD_PATH>`

###### **Arguments:**

* `<FIELD_PATH>` — Path to the field TOML file

###### **Options:**

* `-n`, `--runs <RUNS>` — Number of runs to execute

  Default value: `10`



## `portlang eval`

Run all fields in a directory and report aggregate accuracy

**Usage:** `portlang eval [OPTIONS] <DIRECTORY>`

###### **Arguments:**

* `<DIRECTORY>` — Directory containing field.toml files (searched recursively)

###### **Options:**

* `--html` — Generate HTML dashboard instead of CLI output



## `portlang list`

List trajectories

**Usage:** `portlang list [OPTIONS] [FIELD_NAME]`

###### **Arguments:**

* `<FIELD_NAME>` — Field name to filter by (optional)

###### **Options:**

* `--converged` — Show only converged trajectories
* `--failed` — Show only failed trajectories
* `--limit <LIMIT>` — Limit number of results



## `portlang replay`

Replay a trajectory step-by-step

**Usage:** `portlang replay [OPTIONS] <TRAJECTORY_ID>`

###### **Arguments:**

* `<TRAJECTORY_ID>` — Trajectory ID (filename without .json extension)

###### **Options:**

* `--format <FORMAT>` — Output format (text or json)

  Default value: `text`
* `--html` — Generate HTML viewer instead of CLI output



## `portlang diff`

Compare two trajectories

**Usage:** `portlang diff [OPTIONS] <TRAJECTORY_A> <TRAJECTORY_B>`

###### **Arguments:**

* `<TRAJECTORY_A>` — First trajectory ID
* `<TRAJECTORY_B>` — Second trajectory ID

###### **Options:**

* `--format <FORMAT>` — Output format (text or json)

  Default value: `text`
* `--html` — Generate HTML comparison view instead of CLI output



## `portlang report`

Generate an adaptation report from existing trajectories

**Usage:** `portlang report [OPTIONS] <FIELD_NAME>`

###### **Arguments:**

* `<FIELD_NAME>` — Field name to analyze

###### **Options:**

* `--converged` — Show only converged trajectories
* `--failed` — Show only failed trajectories
* `--limit <LIMIT>` — Limit number of trajectories to analyze



## `portlang view`

View evals and trajectories as interactive HTML

**Usage:** `portlang view <COMMAND>`

###### **Subcommands:**

* `trajectory` — View a single trajectory
* `eval` — View eval results dashboard
* `diff` — View comparison of two trajectories
* `field` — View field adaptation report



## `portlang view trajectory`

View a single trajectory

**Usage:** `portlang view trajectory [OPTIONS] <TRAJECTORY_ID>`

###### **Arguments:**

* `<TRAJECTORY_ID>` — Trajectory ID (filename without .json extension)

###### **Options:**

* `--no-open` — Don't automatically open in browser



## `portlang view eval`

View eval results dashboard

**Usage:** `portlang view eval [OPTIONS] <DIRECTORY>`

###### **Arguments:**

* `<DIRECTORY>` — Directory containing field.toml files

###### **Options:**

* `--no-open` — Don't automatically open in browser



## `portlang view diff`

View comparison of two trajectories

**Usage:** `portlang view diff [OPTIONS] <TRAJECTORY_A> <TRAJECTORY_B>`

###### **Arguments:**

* `<TRAJECTORY_A>` — First trajectory ID
* `<TRAJECTORY_B>` — Second trajectory ID

###### **Options:**

* `--no-open` — Don't automatically open in browser



## `portlang view field`

View field adaptation report

**Usage:** `portlang view field [OPTIONS] <FIELD_NAME>`

###### **Arguments:**

* `<FIELD_NAME>` — Field name to analyze

###### **Options:**

* `--converged` — Show only converged trajectories
* `--failed` — Show only failed trajectories
* `--limit <LIMIT>` — Limit number of trajectories to analyze
* `--no-open` — Don't automatically open in browser



## `portlang docs`

Print CLI reference documentation as Markdown

**Usage:** `portlang docs`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
