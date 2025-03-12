use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(name = "warden", about = "The worker daemon for Corvus", version)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Console commands for interacting with the Hold
    #[clap(subcommand)]
    Console(ConsoleCommands),

    /// Start the warden daemon
    Start,

    /// Stop the warden daemon
    Stop,

    /// Restart the warden daemon
    Restart,
}

#[derive(Subcommand, Debug)]
enum ConsoleCommands {
    /// Enroll a device with the Warden service
    Enroll(console::cli::commands::enroll::Enroll),

    /// Get the status of the Warden service
    Status(console::cli::commands::status::Status),

    /// Toggle the Warden service on or off
    Toggle(console::cli::commands::toggle::Toggle),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Console(console_command) => match console_command {
            ConsoleCommands::Enroll(enroll) => {
                enroll.run().await?;
            }
            ConsoleCommands::Status(status) => {
                status.run().await?;
            }
            ConsoleCommands::Toggle(toggle) => {
                toggle.run().await?;
            }
        },
        Commands::Start => {
            println!("Starting warden daemon...");
            // TODO: Implement daemon start logic
        }
        Commands::Stop => {
            println!("Stopping warden daemon...");
            // TODO: Implement daemon stop logic
        }
        Commands::Restart => {
            println!("Restarting warden daemon...");
            // TODO: Implement daemon restart logic
        }
    }

    Ok(())
}
