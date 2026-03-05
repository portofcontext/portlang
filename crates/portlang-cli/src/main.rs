mod commands;
mod output;

use clap::{Parser, Subcommand};
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
    },
    /// Check a field for errors
    Check {
        /// Path to the field TOML file
        field_path: PathBuf,
    },
    /// Run a field N times and measure convergence reliability
    Converge {
        /// Path to the field TOML file
        field_path: PathBuf,

        /// Number of runs to execute
        #[arg(short = 'n', long, default_value = "10")]
        runs: usize,
    },
    /// Run all fields in a directory and report aggregate accuracy
    Eval {
        /// Directory containing field.toml files (searched recursively)
        directory: PathBuf,
    },
    /// List trajectories
    List {
        /// Field name to filter by (optional)
        field_name: Option<String>,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(long)]
        failed: bool,

        /// Limit number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Replay a trajectory step-by-step
    Replay {
        /// Trajectory ID (filename without .json extension)
        trajectory_id: String,

        /// Output format (text or json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Compare two trajectories
    Diff {
        /// First trajectory ID
        trajectory_a: String,

        /// Second trajectory ID
        trajectory_b: String,

        /// Output format (text or json)
        #[arg(long, default_value = "text")]
        format: String,
    },
    /// Generate an adaptation report from existing trajectories
    Report {
        /// Field name to analyze
        field_name: String,

        /// Show only converged trajectories
        #[arg(long)]
        converged: bool,

        /// Show only failed trajectories
        #[arg(long)]
        failed: bool,

        /// Limit number of trajectories to analyze
        #[arg(long)]
        limit: Option<usize>,
    },
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init { install, start } => {
            if install {
                commands::init::init_install_command().await
            } else if start {
                commands::init::init_start_command()
            } else {
                commands::init::init_command()
            }
        }
        Commands::Run { field_path } => commands::run::run_command(field_path).await,
        Commands::Check { field_path } => commands::check::check_command(field_path),
        Commands::Converge { field_path, runs } => {
            commands::converge::converge_command(field_path, runs).await
        }
        Commands::Eval { directory } => commands::eval::eval_command(directory).await,
        Commands::List {
            field_name,
            converged,
            failed,
            limit,
        } => commands::list::list_command(field_name, converged, failed, limit),
        Commands::Replay {
            trajectory_id,
            format,
        } => commands::replay::replay_command(trajectory_id, format),
        Commands::Diff {
            trajectory_a,
            trajectory_b,
            format,
        } => commands::diff::diff_command(trajectory_a, trajectory_b, format),
        Commands::Report {
            field_name,
            converged,
            failed,
            limit,
        } => commands::report::report_command(field_name, converged, failed, limit),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
