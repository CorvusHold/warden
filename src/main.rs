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

    /// Run the warden daemon in the foreground
    Run,
}

#[derive(Subcommand, Debug)]
enum ConsoleCommands {
    /// Enroll a device with the Warden service
    Enroll(console::cli::commands::enroll::Enroll),

    /// Get the status of the Warden service
    Status(console::cli::commands::status::Status),

    /// Toggle the Warden service on or off
    Toggle(console::cli::commands::toggle::Toggle),

    /// Manage the Warden configuration
    Config(console::cli::commands::config::Config),
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .format_timestamp(None)
        .format_level(true)
        .format_module_path(false)
        .format_indent(Some(4))
        .filter_level(log::LevelFilter::Info)
        .try_init()?;

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
            ConsoleCommands::Config(config) => {
                config.run().await?;
            }
        },
        Commands::Run => {
            log::info!("Running warden daemon in the foreground...");
            daemon::cli::run::execute().await?
        }
        Commands::Start => {
            log::info!("Starting daemonization process...");
            daemon::cli::start::execute().await?
        }
        Commands::Stop => {
            log::info!("Stopping warden daemon...");
            daemon::cli::stop::execute().await?
        }
        Commands::Restart => {
            log::info!("Restarting warden daemon...");
            // First stop the daemon
            daemon::cli::stop::execute().await?;
            // Then start it again
            daemon::cli::start::execute().await?
        }
    }

    Ok(())
}
