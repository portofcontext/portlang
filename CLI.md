# Command-Line Help for `portlang`

This document contains the help content for the `portlang` command-line program.

**Command Overview:**

* [`portlang`↴](#portlang)
* [`portlang new`↴](#portlang-new)
* [`portlang init`↴](#portlang-init)
* [`portlang run`↴](#portlang-run)
* [`portlang list`↴](#portlang-list)
* [`portlang eval`↴](#portlang-eval)
* [`portlang eval run`↴](#portlang-eval-run)
* [`portlang eval list`↴](#portlang-eval-list)
* [`portlang eval view`↴](#portlang-eval-view)
* [`portlang view`↴](#portlang-view)
* [`portlang view trajectory`↴](#portlang-view-trajectory)
* [`portlang view diff`↴](#portlang-view-diff)
* [`portlang view field`↴](#portlang-view-field)
* [`portlang reflect`↴](#portlang-reflect)
* [`portlang docs`↴](#portlang-docs)

## `portlang`

portlang - agent runtime with structured tools and verifiers

**Usage:** `portlang <COMMAND>`

###### **Subcommands:**

* `new` — Create a new .field file
* `init` — Initialize and check portlang environment
* `run` — Run a field
* `list` — List trajectories
* `eval` — Run evals and inspect results
* `view` — View trajectories and field reports
* `reflect` — Analyze trajectories and surface insights about a field
* `docs` — Print CLI reference documentation as Markdown



## `portlang new`

Create a new .field file

**Usage:** `portlang new [OPTIONS] [PATH]`

###### **Arguments:**

* `<PATH>` — Output path (file or directory); defaults to ./{name}.field

###### **Options:**

* `-i`, `--interactive` — Walk through field creation step by step
* `-n`, `--name <NAME>` — Field name (required without --interactive)
* `--description <DESCRIPTION>` — Human-readable description of the field
* `-m`, `--model <MODEL>` — Model identifier, e.g. "anthropic/claude-sonnet-4.6" or "openai/gpt-4o"

  Default value: `anthropic/claude-sonnet-4.6`
* `--temperature <TEMPERATURE>` — Sampling temperature 0.0–1.0

  Default value: `1.0`
* `-g`, `--goal <GOAL>` — Agent goal / initial task prompt (required without --interactive)
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

   trigger: "on_stop" | "always" | "on_tool:<tool_name>" (default: "on_stop")



## `portlang init`

Initialize and check portlang environment

**Usage:** `portlang init [OPTIONS]`

###### **Options:**

* `--install` — Automatically download and install Apple Container
* `--start` — Start the container system service



## `portlang run`

Run a field

**Usage:** `portlang run [OPTIONS] <FIELD_PATH>`

###### **Arguments:**

* `<FIELD_PATH>` — Path to the field file (.field or .toml)

###### **Options:**

* `--dry-run` — Validate field without running (parse, check template variables, show config)
* `-n`, `--runs <RUNS>` — Run N times and report convergence reliability

  Default value: `1`
* `-p`, `--parent-field <PARENT_FIELD>` — Path to a parent field to inherit from (auto-detected from ../*.field if not set)
* `--var <KEY=VALUE>` — Template variable as KEY=VALUE (repeatable, e.g. --var customer_id=123)
* `--vars <FILE>` — JSON file containing template variables (key→value map)
* `--input <FILE_OR_JSON>` — Input data to stage into the workspace: path to a file or inline JSON string
* `--runner <RUNNER>` — Agent loop runner: "native" (default) or "claude-code"

  Default value: `native`
* `--auto-reflect` — After the run completes, automatically reflect on that trajectory



## `portlang list`

List trajectories

**Usage:** `portlang list [OPTIONS] [FIELD_NAME]`

###### **Arguments:**

* `<FIELD_NAME>` — Field name to filter by (optional)

###### **Options:**

* `--converged` — Show only converged trajectories
* `-f`, `--failed` — Show only failed trajectories
* `-l`, `--limit <LIMIT>` — Limit number of results



## `portlang eval`

Run evals and inspect results

**Usage:** `portlang eval <COMMAND>`

###### **Subcommands:**

* `run` — Run all fields in a directory and report aggregate accuracy
* `list` — List eval runs
* `view` — View eval results dashboard as interactive HTML



## `portlang eval run`

Run all fields in a directory and report aggregate accuracy

**Usage:** `portlang eval run [OPTIONS] <DIRECTORY>`

###### **Arguments:**

* `<DIRECTORY>` — Directory containing .field files (searched recursively)

###### **Options:**

* `-p`, `--parent-field <PARENT_FIELD>` — Path to a parent field to inherit from (defaults to <directory>/field.field if present)
* `--resume <RESUME>` — Resume a previous eval run, skipping fields that already passed
* `--runner <RUNNER>` — Agent loop runner: "native" (default) or "claude-code"

  Default value: `native`
* `--var <KEY=VALUE>` — Template variable as KEY=VALUE (repeatable)
* `--vars <FILE>` — JSON file containing template variables (key→value map)



## `portlang eval list`

List eval runs

**Usage:** `portlang eval list [OPTIONS] [DIR]`

###### **Arguments:**

* `<DIR>` — Filter by directory (substring match)

###### **Options:**

* `-l`, `--limit <LIMIT>` — Limit number of results



## `portlang eval view`

View eval results dashboard as interactive HTML

**Usage:** `portlang eval view [OPTIONS] <ID_OR_DIR>`

###### **Arguments:**

* `<ID_OR_DIR>` — Eval run ID or directory path

###### **Options:**

* `--no-open` — Don't automatically open in browser



## `portlang view`

View trajectories and field reports

**Usage:** `portlang view <COMMAND>`

###### **Subcommands:**

* `trajectory` — View a trajectory
* `diff` — Compare two trajectories
* `field` — View field adaptation report



## `portlang view trajectory`

View a trajectory

**Usage:** `portlang view trajectory [OPTIONS] <TRAJECTORY_ID>`

###### **Arguments:**

* `<TRAJECTORY_ID>` — Trajectory ID (filename without .json extension)

###### **Options:**

* `-f`, `--format <FORMAT>` — Output format: "html" (default, opens browser) or "text" (interactive replay) or "json"

  Default value: `html`
* `--no-open` — Don't automatically open in browser (html format only)



## `portlang view diff`

Compare two trajectories

**Usage:** `portlang view diff [OPTIONS] <TRAJECTORY_A> <TRAJECTORY_B>`

###### **Arguments:**

* `<TRAJECTORY_A>` — First trajectory ID
* `<TRAJECTORY_B>` — Second trajectory ID

###### **Options:**

* `-f`, `--format <FORMAT>` — Output format: "html" (default, opens browser) or "text" or "json"

  Default value: `html`
* `--no-open` — Don't automatically open in browser (html format only)



## `portlang view field`

View field adaptation report

**Usage:** `portlang view field [OPTIONS] <FIELD_NAME>`

###### **Arguments:**

* `<FIELD_NAME>` — Field name to analyze

###### **Options:**

* `-f`, `--format <FORMAT>` — Output format: "html" (default, opens browser) or "text"

  Default value: `html`
* `--converged` — Show only converged trajectories
* `--failed` — Show only failed trajectories
* `-l`, `--limit <LIMIT>` — Limit number of trajectories to analyze
* `--no-open` — Don't automatically open in browser (html format only)



## `portlang reflect`

Analyze trajectories and surface insights about a field

**Usage:** `portlang reflect [OPTIONS]`

###### **Options:**

* `-f`, `--field <FIELD>` — Field name to analyze (must match a subdirectory in ~/.portlang/trajectories/)
* `-t`, `--trajectory-id <TRAJECTORY_ID>` — Analyze a specific trajectory by ID instead of the N most recent
* `-n`, `--trajectories <TRAJECTORIES>` — Number of recent trajectories to analyze (default: 5)

  Default value: `5`
* `--runner <RUNNER>` — Agent loop runner: "native" (default) or "claude-code"

  Default value: `native`



## `portlang docs`

Print CLI reference documentation as Markdown

**Usage:** `portlang docs`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
