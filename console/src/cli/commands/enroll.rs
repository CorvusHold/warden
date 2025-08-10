// options
// name: string (name display in the console)
// tag: string (tags display in the console)

// command
// enroll [OPTIONS] <ENROLLMENT_TOKEN>

use anyhow::Result;
use clap::Args;
use log::info;

#[derive(Debug, Args)]
pub struct Enroll {
    /// Enrollment token
    enrollment_token: String,

    /// Name to display in the console
    #[clap(long)]
    name: Option<String>,

    /// Tags to display in the console
    #[clap(long)]
    tags: Option<String>,
}

impl Enroll {
    pub async fn run(self) -> Result<()> {
        info!("Enrolling with token: {}", self.enrollment_token);
        if let Some(name) = self.name {
            info!("  Name: {name}");
        }
        if let Some(tags) = self.tags {
            info!("  Tags: {tags}");
        }

        // TODO: Implement the actual enrollment logic here
        // This is where you would call the API to enroll the device
        // For now, we'll just print a message

        info!("Enrollment successful (fake API call)");

        Ok(())
    }
}
