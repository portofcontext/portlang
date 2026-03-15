mod commands;
mod output;

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
    /// Create a new field.toml
    New {
        /// Output path (file or directory); defaults to ./field.toml
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
        /// Path to the field TOML file
        field_path: PathBuf,

        /// Path to a parent field.toml to inherit from (auto-detected from ../field.toml if not set)
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
    },
    /// Check a field for errors
    Check {
        /// Path to the field TOML file
        field_path: PathBuf,

        /// Path to a parent field.toml to inherit from (auto-detected from ../field.toml if not set)
        #[arg(short = 'p', long)]
        parent_field: Option<PathBuf>,

        /// Template variable as KEY=VALUE (repeatable)
        #[arg(long = "var", value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// JSON file containing template variables (key→value map)
        #[arg(long = "vars", value_name = "FILE")]
        vars: Option<PathBuf>,
    },
    /// Run a field N times and measure convergence reliability
    Converge {
        /// Path to the field TOML file
        field_path: PathBuf,

        /// Number of runs to execute
        #[arg(short = 'n', long, default_value = "10")]
        runs: usize,

        /// Path to a parent field.toml to inherit from (auto-detected from ../field.toml if not set)
        #[arg(short = 'p', long)]
        parent_field: Option<PathBuf>,

        /// Template variable as KEY=VALUE (repeatable)
        #[arg(long = "var", value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// JSON file containing template variables (key→value map)
        #[arg(long = "vars", value_name = "FILE")]
        vars: Option<PathBuf>,

        /// Input data to stage into the workspace: path to a file or inline JSON string
        #[arg(long = "input", value_name = "FILE_OR_JSON")]
        input: Option<String>,
    },
    /// Run all fields in a directory and report aggregate accuracy
    Eval {
        /// Directory containing field.toml files (searched recursively)
        directory: PathBuf,

        /// Path to a parent field.toml to inherit from (defaults to <directory>/field.toml if present)
        #[arg(short = 'p', long)]
        parent_field: Option<PathBuf>,

        /// Resume a previous eval run, skipping fields that already passed
        #[arg(long)]
        resume: Option<String>,

        /// Generate HTML dashboard instead of CLI output
        #[arg(long)]
        html: bool,

        /// Template variable as KEY=VALUE (repeatable)
        #[arg(long = "var", value_name = "KEY=VALUE")]
        var: Vec<String>,

        /// JSON file containing template variables (key→value map)
        #[arg(long = "vars", value_name = "FILE")]
        vars: Option<PathBuf>,
    },
    /// List trajectories and eval runs
    List {
        #[command(subcommand)]
        subcommand: ListSubcommand,
    },
    /// Replay a trajectory step-by-step
    Replay {
        /// Trajectory ID (filename without .json extension)
        trajectory_id: String,

        /// Output format (text or json)
        #[arg(short = 'f', long, default_value = "text")]
        format: String,

        /// Generate HTML viewer instead of CLI output
        #[arg(long)]
        html: bool,
    },
    /// Compare two trajectories
    Diff {
        /// First trajectory ID
        trajectory_a: String,

        /// Second trajectory ID
        trajectory_b: String,

        /// Output format (text or json)
        #[arg(short = 'f', long, default_value = "text")]
        format: String,

        /// Generate HTML comparison view instead of CLI output
        #[arg(long)]
        html: bool,
    },
    /// Generate an adaptation report from existing trajectories
    Report {
        /// Field name to analyze
        field_name: String,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(short = 'f', long)]
        failed: bool,

        /// Limit number of trajectories to analyze
        #[arg(short = 'l', long)]
        limit: Option<usize>,
    },
    /// View evals and trajectories as interactive HTML
    View {
        #[command(subcommand)]
        subcommand: ViewSubcommand,
    },
    /// Print CLI reference documentation as Markdown
    Docs,
}

#[derive(Subcommand)]
enum ListSubcommand {
    /// List trajectories
    Trajectories {
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
    /// List eval runs
    Evals {
        /// Filter by directory (substring match)
        dir: Option<String>,

        /// Limit number of results
        #[arg(short = 'l', long)]
        limit: Option<usize>,
    },
}

#[derive(Subcommand)]
enum ViewSubcommand {
    /// View a single trajectory
    Trajectory {
        /// Trajectory ID (filename without .json extension)
        trajectory_id: String,

        /// Don't automatically open in browser
        #[arg(long)]
        no_open: bool,
    },
    /// View eval results dashboard
    Eval {
        /// Eval run ID or directory path
        id_or_dir: String,

        /// Don't automatically open in browser
        #[arg(long)]
        no_open: bool,
    },
    /// View comparison of two trajectories
    Diff {
        /// First trajectory ID
        trajectory_a: String,

        /// Second trajectory ID
        trajectory_b: String,

        /// Don't automatically open in browser
        #[arg(long)]
        no_open: bool,
    },
    /// View field adaptation report
    Field {
        /// Field name to analyze
        field_name: String,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(short = 'f', long)]
        failed: bool,

        /// Limit number of trajectories to analyze
        #[arg(short = 'l', long)]
        limit: Option<usize>,

        /// Don't automatically open in browser
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
            // If it starts with '{' or '[', treat as inline JSON; otherwise treat as file path
            let trimmed = s.trim();
            if trimmed.starts_with('{') || trimmed.starts_with('[') {
                // Validate it's parseable JSON before handing to runtime
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
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
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
                .add_directive("portlang_runtime=info".parse().unwrap())
                .add_directive("portlang_trajectory=info".parse().unwrap()),
        )
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
            parent_field,
            var,
            vars,
            input,
        } => match build_runtime_context(var, vars, input) {
            Ok(ctx) => commands::run::run_command(field_path, parent_field, ctx).await,
            Err(e) => Err(e),
        },
        Commands::Check {
            field_path,
            parent_field,
            var,
            vars,
        } => match build_runtime_context(var, vars, None) {
            Ok(ctx) => commands::check::check_command(field_path, parent_field, ctx),
            Err(e) => Err(e),
        },
        Commands::Converge {
            field_path,
            runs,
            parent_field,
            var,
            vars,
            input,
        } => match build_runtime_context(var, vars, input) {
            Ok(ctx) => {
                commands::converge::converge_command(field_path, runs, parent_field, ctx).await
            }
            Err(e) => Err(e),
        },
        Commands::List { subcommand } => match subcommand {
            ListSubcommand::Trajectories {
                field_name,
                converged,
                failed,
                limit,
            } => commands::list::list_command(field_name, converged, failed, limit),
            ListSubcommand::Evals { dir, limit } => commands::evals::evals_command(dir, limit),
        },
        Commands::Eval {
            directory,
            parent_field,
            resume,
            html,
            var,
            vars,
        } => {
            if html {
                commands::view::view_eval(directory.to_string_lossy().to_string(), true)
            } else {
                match build_runtime_context(var, vars, None) {
                    Ok(ctx) => {
                        commands::eval::eval_command(directory, parent_field, resume, ctx).await
                    }
                    Err(e) => Err(e),
                }
            }
        }
        Commands::Replay {
            trajectory_id,
            format,
            html,
        } => {
            if html {
                commands::view::view_trajectory(trajectory_id, true)
            } else {
                commands::replay::replay_command(trajectory_id, format)
            }
        }
        Commands::Diff {
            trajectory_a,
            trajectory_b,
            format,
            html,
        } => {
            if html {
                commands::view::view_diff(trajectory_a, trajectory_b, true)
            } else {
                commands::diff::diff_command(trajectory_a, trajectory_b, format)
            }
        }
        Commands::Report {
            field_name,
            converged,
            failed,
            limit,
        } => commands::report::report_command(field_name, converged, failed, limit),
        Commands::Docs => {
            let markdown = clap_markdown::help_markdown::<Cli>();
            std::fs::write("CLI.md", &markdown).expect("failed to write CLI.md");
            println!("CLI.md written.");
            return;
        }
        Commands::View { subcommand } => match subcommand {
            ViewSubcommand::Trajectory {
                trajectory_id,
                no_open,
            } => commands::view::view_trajectory(trajectory_id, !no_open),
            ViewSubcommand::Eval { id_or_dir, no_open } => {
                commands::view::view_eval(id_or_dir, !no_open)
            }
            ViewSubcommand::Diff {
                trajectory_a,
                trajectory_b,
                no_open,
            } => commands::view::view_diff(trajectory_a, trajectory_b, !no_open),
            ViewSubcommand::Field {
                field_name,
                converged,
                failed,
                limit,
                no_open,
            } => commands::view::view_field_report(field_name, converged, failed, limit, !no_open),
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
