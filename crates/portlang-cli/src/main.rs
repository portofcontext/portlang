mod commands;
mod output;
mod output_collector;
mod progress;

use clap::{Parser, Subcommand};
use portlang_core::{InputSource, RuntimeContext};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process;

#[derive(Parser)]
#[command(name = "portlang")]
#[command(about = "portlang - agent runtime with structured tools and verifiers")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new .field file
    New {
        /// Output path (file or directory); defaults to ./{name}.field
        path: Option<PathBuf>,

        /// Walk through field creation step by step
        #[arg(short = 'i', long)]
        interactive: bool,

        /// Field name (required without --interactive)
        #[arg(short = 'n', long)]
        name: Option<String>,

        /// Human-readable description of the field
        #[arg(long)]
        description: Option<String>,

        /// Model identifier, e.g. "anthropic/claude-sonnet-4.6" or "openai/gpt-4o"
        #[arg(short = 'm', long, default_value = "anthropic/claude-sonnet-4.6")]
        model: String,

        /// Sampling temperature 0.0–1.0
        #[arg(long, default_value = "1.0")]
        temperature: f32,

        /// Agent goal / initial task prompt (required without --interactive)
        #[arg(short = 'g', long)]
        goal: Option<String>,

        /// System prompt prepended to every agent interaction
        #[arg(long)]
        system: Option<String>,

        /// Command run before each step to refresh agent context (repeatable)
        #[arg(long = "re-observation")]
        re_observation: Vec<String>,

        /// APT packages to install in the container (repeatable; use "uv" to install uv via pip)
        #[arg(long)]
        package: Vec<String>,

        /// Glob pattern the agent may write to (repeatable, e.g. --allow-write "*.txt")
        #[arg(long = "allow-write")]
        allow_write: Vec<String>,

        /// Network access policy: "allow" or "deny"
        #[arg(long, default_value = "allow")]
        network: String,

        /// Hard ceiling on total agent steps
        #[arg(long, default_value = "20")]
        max_steps: u64,

        /// Hard ceiling on total cost, e.g. "$1.00"
        #[arg(long, default_value = "$1.00")]
        max_cost: String,

        /// Hard ceiling on total context tokens
        #[arg(long)]
        max_tokens: Option<u64>,

        /// Tool definition as JSON (repeatable).
        ///
        /// Python tool:
        ///   --tool '{"type":"python","file":"./tools/calc.py","function":"execute"}'
        ///   Optional: "name", "description" (override auto-extracted values)
        ///
        /// Shell tool:
        ///   --tool '{"type":"shell","name":"run_sql","description":"Run a SQL query","command":"sqlite3 db.sqlite"}'
        ///
        /// MCP tool (stdio):
        ///   --tool '{"type":"mcp","name":"stripe","command":"npx","args":["-y","@stripe/mcp"],"env":{"STRIPE_SECRET_KEY":"${STRIPE_SECRET_KEY}"}}'
        ///
        /// MCP tool (http/sse):
        ///   --tool '{"type":"mcp","name":"myserver","url":"https://example.com/mcp","headers":{"Authorization":"Bearer ${TOKEN}"},"transport":"sse"}'
        #[arg(long)]
        tool: Vec<String>,

        /// Verifier definition as JSON (repeatable).
        ///
        /// Example:
        ///   --verifier '{"name":"check-file","command":"test -f result.txt","trigger":"on_stop","description":"result.txt must exist"}'
        ///
        /// trigger: "on_stop" | "always" | "on_tool:<tool_name>" (default: "on_stop")
        #[arg(long)]
        verifier: Vec<String>,
    },
    /// Initialize and check portlang environment
    Init {
        /// Automatically download and install Apple Container
        #[arg(long)]
        install: bool,

        /// Start the container system service
        #[arg(long)]
        start: bool,
    },
    /// Run a field
    Run {
        /// Path to the field file (.field or .toml)
        field_path: PathBuf,

        /// Validate field without running (parse, check template variables, show config)
        #[arg(long)]
        dry_run: bool,

        /// Run N times and report convergence reliability
        #[arg(short = 'n', long, default_value = "1")]
        runs: usize,

        /// Path to a parent field to inherit from (auto-detected from ../*.field if not set)
        #[arg(short = 'p', long)]
        parent_field: Option<PathBuf>,

        /// Template variable as KEY=VALUE (repeatable, e.g. --var customer_id=123)
        #[arg(long = "var", value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// JSON file containing template variables (key→value map)
        #[arg(long = "vars", value_name = "FILE")]
        vars: Option<PathBuf>,

        /// Input data to stage into the workspace: path to a file or inline JSON string
        #[arg(long = "input", value_name = "FILE_OR_JSON")]
        input: Option<String>,

        /// Agent loop runner: "native" (default) or "claude-code"
        #[arg(long = "runner", default_value = "native")]
        runner: String,

        /// Sandbox backend: "http" or "subprocess" (overrides local container auto-detection)
        #[arg(long = "backend")]
        backend: Option<String>,

        /// URL for the HTTP backend (required when --backend http)
        #[arg(long = "backend-url")]
        backend_url: Option<String>,

        /// Shell command for the subprocess backend (required when --backend subprocess)
        #[arg(long = "backend-command")]
        backend_command: Option<String>,

        /// After the run completes, automatically reflect on that trajectory
        #[arg(long)]
        auto_reflect: bool,

        /// Copy output artifacts to this directory after the run.
        ///
        /// Files collected are those matching `collect` patterns in `[boundary]`
        /// (defaults to all `allow_write` patterns when `collect` is not set).
        #[arg(long = "output-dir", value_name = "PATH")]
        output_dir: Option<PathBuf>,

        /// Emit a single JSON object to stdout instead of human-readable output.
        ///
        /// The JSON includes run metadata, `structured_output` (if `output_schema` was set),
        /// and `artifacts` with inline file contents (up to 512 KB per file / 2 MB total).
        #[arg(long)]
        json: bool,
    },
    /// List trajectories
    List {
        /// Field name to filter by (optional)
        field_name: Option<String>,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(short = 'f', long)]
        failed: bool,

        /// Limit number of results
        #[arg(short = 'l', long)]
        limit: Option<usize>,
    },
    /// Run evals and inspect results
    Eval {
        #[command(subcommand)]
        subcommand: EvalSubcommand,
    },
    /// View trajectories and field reports
    View {
        #[command(subcommand)]
        subcommand: ViewSubcommand,
    },
    /// Analyze trajectories and surface insights about a field
    Reflect {
        /// Field name to analyze (must match a subdirectory in ~/.portlang/trajectories/)
        #[arg(short = 'f', long)]
        field: Option<String>,

        /// Analyze a specific trajectory by ID instead of the N most recent
        #[arg(short = 't', long = "trajectory-id")]
        trajectory_id: Option<String>,

        /// Number of recent trajectories to analyze (default: 5)
        #[arg(short = 'n', long = "trajectories", default_value = "5")]
        trajectories: usize,

        /// Agent loop runner: "native" (default) or "claude-code"
        #[arg(long = "runner", default_value = "native")]
        runner: String,
    },

    /// Print CLI reference documentation as Markdown
    Docs,
}

#[derive(Subcommand)]
enum EvalSubcommand {
    /// Run all fields in a directory and report aggregate accuracy
    Run {
        /// Directory containing .field files (searched recursively)
        directory: PathBuf,

        /// Path to a parent field to inherit from (defaults to <directory>/field.field if present)
        #[arg(short = 'p', long)]
        parent_field: Option<PathBuf>,

        /// Resume a previous eval run, skipping fields that already passed
        #[arg(long)]
        resume: Option<String>,

        /// Agent loop runner: "native" (default) or "claude-code"
        #[arg(long = "runner", default_value = "native")]
        runner: String,

        /// Sandbox backend: "http" or "subprocess" (overrides local container auto-detection)
        #[arg(long = "backend")]
        backend: Option<String>,

        /// URL for the HTTP backend (required when --backend http)
        #[arg(long = "backend-url")]
        backend_url: Option<String>,

        /// Shell command for the subprocess backend (required when --backend subprocess)
        #[arg(long = "backend-command")]
        backend_command: Option<String>,

        /// Template variable as KEY=VALUE (repeatable)
        #[arg(long = "var", value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// JSON file containing template variables (key→value map)
        #[arg(long = "vars", value_name = "FILE")]
        vars: Option<PathBuf>,
    },
    /// List eval runs
    List {
        /// Filter by directory (substring match)
        dir: Option<String>,

        /// Limit number of results
        #[arg(short = 'l', long)]
        limit: Option<usize>,
    },
    /// View eval results dashboard as interactive HTML
    View {
        /// Eval run ID or directory path
        id_or_dir: String,

        /// Don't automatically open in browser
        #[arg(long)]
        no_open: bool,
    },
}

#[derive(Subcommand)]
enum ViewSubcommand {
    /// View a trajectory
    Trajectory {
        /// Trajectory ID (filename without .json extension)
        trajectory_id: String,

        /// Output format: "html" (default, opens browser) or "text" (interactive replay) or "json"
        #[arg(short = 'f', long, default_value = "html")]
        format: String,

        /// Don't automatically open in browser (html format only)
        #[arg(long)]
        no_open: bool,
    },
    /// Compare two trajectories
    Diff {
        /// First trajectory ID
        trajectory_a: String,

        /// Second trajectory ID
        trajectory_b: String,

        /// Output format: "html" (default, opens browser) or "text" or "json"
        #[arg(short = 'f', long, default_value = "html")]
        format: String,

        /// Don't automatically open in browser (html format only)
        #[arg(long)]
        no_open: bool,
    },
    /// View field adaptation report
    Field {
        /// Field name to analyze
        field_name: String,

        /// Output format: "html" (default, opens browser) or "text"
        #[arg(short = 'f', long, default_value = "html")]
        format: String,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(long)]
        failed: bool,

        /// Limit number of trajectories to analyze
        #[arg(short = 'l', long)]
        limit: Option<usize>,

        /// Don't automatically open in browser (html format only)
        #[arg(long)]
        no_open: bool,
    },
}

/// Parse --var KEY=VALUE flags and optional --vars FILE into a RuntimeContext.
fn build_runtime_context(
    var_flags: Vec<String>,
    vars_file: Option<PathBuf>,
    input_arg: Option<String>,
) -> anyhow::Result<RuntimeContext> {
    let mut vars: HashMap<String, String> = HashMap::new();

    // Load from --vars file first (lower priority)
    if let Some(ref path) = vars_file {
        let content = std::fs::read_to_string(path).map_err(|e| {
            anyhow::anyhow!("Failed to read --vars file '{}': {}", path.display(), e)
        })?;
        let file_map: HashMap<String, String> = serde_json::from_str(&content).map_err(|e| {
            anyhow::anyhow!(
                "--vars file '{}' must be a JSON object with string values: {}",
                path.display(),
                e
            )
        })?;
        vars.extend(file_map);
    }

    // --var flags override file values
    for kv in var_flags {
        let (k, v) = kv
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--var must be KEY=VALUE, got: {:?}", kv))?;
        vars.insert(k.to_string(), v.to_string());
    }

    // Parse --input: detect file vs inline JSON
    let input = match input_arg {
        None => None,
        Some(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                serde_json::from_str::<serde_json::Value>(trimmed).map_err(|e| {
                    anyhow::anyhow!("--input value looks like JSON but is not valid: {}", e)
                })?;
                Some(InputSource::Inline(trimmed.to_string()))
            } else {
                Some(InputSource::File(PathBuf::from(s)))
            }
        }
    };

    Ok(RuntimeContext { vars, input })
}

#[tokio::main]
async fn main() {
    // Initialize tracing with a progress layer that drives the run spinner
    use tracing_subscriber::prelude::*;
    let env_filter = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive(tracing::Level::WARN.into())
        .add_directive("portlang_adapt=info".parse().unwrap())
        .add_directive("portlang_check=info".parse().unwrap())
        .add_directive("portlang_cli=info".parse().unwrap())
        .add_directive("portlang_config=info".parse().unwrap())
        .add_directive("portlang_core=info".parse().unwrap())
        .add_directive("portlang_pipeline=info".parse().unwrap())
        .add_directive("portlang_provider_anthropic=info".parse().unwrap())
        .add_directive("portlang_provider_openai=info".parse().unwrap())
        .add_directive("portlang_provider_openrouter=info".parse().unwrap())
        .add_directive("portlang_runner_claudecode=info".parse().unwrap())
        .add_directive("portlang_runtime=info".parse().unwrap())
        .add_directive("portlang_trajectory=info".parse().unwrap());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(env_filter))
        .with(crate::progress::ProgressTracingLayer)
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::New {
            path,
            interactive,
            name,
            description,
            model,
            temperature,
            goal,
            system,
            re_observation,
            package,
            allow_write,
            network,
            max_steps,
            max_cost,
            max_tokens,
            tool,
            verifier,
        } => commands::new::new_command(commands::new::NewArgs {
            path,
            interactive,
            name,
            description,
            model,
            temperature,
            goal,
            system,
            re_observation,
            packages: package,
            allow_write,
            network,
            max_steps,
            max_cost,
            max_tokens,
            tools: tool,
            verifiers: verifier,
        }),
        Commands::Init { install, start } => {
            if install {
                commands::init::init_install_command().await
            } else if start {
                commands::init::init_start_command()
            } else {
                commands::init::init_command()
            }
        }
        Commands::Run {
            field_path,
            dry_run,
            runs,
            parent_field,
            var,
            vars,
            input,
            runner,
            backend,
            backend_url,
            backend_command,
            auto_reflect,
            output_dir,
            json,
        } => match build_runtime_context(var, vars, input) {
            Ok(ctx) => {
                commands::run::run_command(
                    field_path,
                    parent_field,
                    ctx,
                    runner,
                    backend,
                    backend_url,
                    backend_command,
                    dry_run,
                    runs,
                    auto_reflect,
                    output_dir,
                    json,
                )
                .await
            }
            Err(e) => Err(e),
        },
        Commands::List {
            field_name,
            converged,
            failed,
            limit,
        } => commands::list::list_command(field_name, converged, failed, limit),
        Commands::Eval { subcommand } => match subcommand {
            EvalSubcommand::Run {
                directory,
                parent_field,
                resume,
                runner,
                backend,
                backend_url,
                backend_command,
                var,
                vars,
            } => match build_runtime_context(var, vars, None) {
                Ok(ctx) => {
                    commands::eval::eval_command(
                        directory,
                        parent_field,
                        resume,
                        ctx,
                        runner,
                        backend,
                        backend_url,
                        backend_command,
                    )
                    .await
                }
                Err(e) => Err(e),
            },
            EvalSubcommand::List { dir, limit } => commands::evals::evals_command(dir, limit),
            EvalSubcommand::View { id_or_dir, no_open } => {
                commands::view::view_eval(id_or_dir, !no_open)
            }
        },
        Commands::View { subcommand } => match subcommand {
            ViewSubcommand::Trajectory {
                trajectory_id,
                format,
                no_open,
            } => commands::view::view_trajectory(trajectory_id, &format, !no_open),
            ViewSubcommand::Diff {
                trajectory_a,
                trajectory_b,
                format,
                no_open,
            } => commands::view::view_diff(trajectory_a, trajectory_b, &format, !no_open),
            ViewSubcommand::Field {
                field_name,
                format,
                converged,
                failed,
                limit,
                no_open,
            } => commands::view::view_field_report(
                field_name, converged, failed, limit, &format, !no_open,
            ),
        },
        Commands::Reflect {
            field,
            trajectory_id,
            trajectories,
            runner,
        } => commands::reflect::reflect_command(field, trajectory_id, trajectories, runner).await,
        Commands::Docs => {
            let markdown = clap_markdown::help_markdown::<Cli>();
            std::fs::write("CLI.md", &markdown).expect("failed to write CLI.md");
            println!("CLI.md written.");
            return;
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
