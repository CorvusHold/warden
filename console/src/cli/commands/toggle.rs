// commands
// toggle

// Render
// Warden is <enabled/disabled>
// Hold is aligned <yes/no>

use anyhow::Result;
use clap::Args;

#[derive(Debug, Args)]
pub struct Toggle {
    /// Toggle warden on or off
    #[clap(long, default_value = "true")]
    on: bool,
}

impl Toggle {
    pub async fn run(self) -> Result<()> {
        println!("Toggling warden (fake API call)...");
        println!("  On: {}", self.on);

        // TODO: Implement the actual toggle logic here
        // This is where you would call the API to toggle the warden
        // For now, we'll just print a message

        println!(
            "Warden is now {}",
            if self.on { "enabled" } else { "disabled" }
        );
        println!("Hold is aligned: yes");

        Ok(())
    }
}
