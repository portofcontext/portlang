use anyhow::{Context, Result};
use portlang_trajectory::{format_step, format_summary, FilesystemStore, ReplaySession};
use std::io::{self, Write};

/// Replay a trajectory step-by-step
pub fn replay_command(trajectory_id: String, format: String) -> Result<()> {
    let store = FilesystemStore::new()?;

    // Load trajectory by filename
    let trajectory = store
        .find_by_filename(&trajectory_id)
        .context(format!("Failed to load trajectory: {}", trajectory_id))?;

    match format.as_str() {
        "json" => {
            // Output raw JSON
            let json = serde_json::to_string_pretty(&trajectory)?;
            println!("{}", json);
        }
        "text" | _ => {
            // Interactive replay
            interactive_replay(trajectory)?;
        }
    }

    Ok(())
}

fn interactive_replay(trajectory: portlang_core::Trajectory) -> Result<()> {
    let mut session = ReplaySession::new(trajectory);

    // Print summary
    println!("{}", format_summary(session.trajectory()));
    println!();

    // Interactive loop
    loop {
        // Print current step
        if let Some(step) = session.current() {
            println!("{}", format_step(step));
        } else if session.is_at_end() {
            println!("=== End of Trajectory ===");
            if let Some(outcome) = &session.trajectory().outcome {
                println!("Final outcome: {}", outcome.description());
            }
        }

        println!();

        // Show navigation options
        let mut options = Vec::new();
        if !session.is_at_start() {
            options.push("[p]rev");
        }
        if !session.is_at_end() {
            options.push("[n]ext");
        }
        options.push("[g]oto");
        options.push("[s]ummary");
        options.push("[q]uit");

        println!(
            "Step {}/{} - {}",
            session.current_step_number(),
            session.total_steps(),
            options.join("  ")
        );

        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        match input.as_str() {
            "n" | "next" => {
                if session.next().is_none() && session.is_at_end() {
                    println!("Already at the end.");
                }
            }
            "p" | "prev" => {
                if session.prev().is_none() && session.is_at_start() {
                    println!("Already at the start.");
                }
            }
            "g" | "goto" => {
                print!("Go to step (0-{}): ", session.total_steps());
                io::stdout().flush()?;
                let mut step_input = String::new();
                io::stdin().read_line(&mut step_input)?;
                if let Ok(step_num) = step_input.trim().parse::<usize>() {
                    if session.goto(step_num).is_none() {
                        println!("Invalid step number.");
                    }
                } else {
                    println!("Invalid input.");
                }
            }
            "s" | "summary" => {
                println!("{}", format_summary(session.trajectory()));
            }
            "q" | "quit" => {
                break;
            }
            "" => {
                // Empty input - go to next step
                if session.next().is_none() && session.is_at_end() {
                    println!("Already at the end.");
                }
            }
            _ => {
                println!("Unknown command: {}", input);
            }
        }

        println!();
    }

    Ok(())
}
