use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[clap(
    name = "warden-console",
    about = "A console for interacting with the Warden service"
)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
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
    let cli = Cli::parse();

    match cli.command {
        Commands::Enroll(enroll) => enroll.run().await?,
        Commands::Status(status) => status.run().await?,
        Commands::Toggle(toggle) => toggle.run().await?,
        Commands::Config(config) => config.run().await?,
    }
    Ok(())
}
