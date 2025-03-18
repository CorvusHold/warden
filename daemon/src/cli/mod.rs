pub mod run;
pub mod start;
pub mod status;
pub mod stop;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "warden", about = "The worker daemon for Corvus", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the warden daemon
    Start,

    /// Stop the warden daemon
    Stop,

    /// Restart the warden daemon
    Restart,

    /// Run the warden daemon in the foreground
    Run,

    /// Get the status of the warden daemon
    Status,
}
