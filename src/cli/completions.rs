//! Shell completion generation for Warden CLI
//!
//! Generates completion scripts for bash, zsh, fish, and other shells.

use clap::Command;
use clap_complete::{generate, Shell};
use std::io;

/// Generate shell completions and write to stdout
pub fn generate_completions(shell: Shell, cmd: &mut Command) {
    generate(shell, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

/// Get installation instructions for the given shell
pub fn get_installation_instructions(shell: Shell) -> String {
    match shell {
        Shell::Bash => r#"
# Bash Completion Installation

## Option 1: System-wide (requires root)
warden completions bash | sudo tee /etc/bash_completion.d/warden > /dev/null

## Option 2: User-local
mkdir -p ~/.local/share/bash-completion/completions
warden completions bash > ~/.local/share/bash-completion/completions/warden

## Option 3: Add to .bashrc
echo 'eval "$(warden completions bash)"' >> ~/.bashrc
source ~/.bashrc
"#
        .to_string(),

        Shell::Zsh => r#"
# Zsh Completion Installation

## Option 1: Using Oh My Zsh
warden completions zsh > ~/.oh-my-zsh/completions/_warden

## Option 2: Manual installation
# First, ensure your completions directory is in fpath
# Add to ~/.zshrc:
#   fpath=(~/.zsh/completions $fpath)
#   autoload -Uz compinit && compinit

mkdir -p ~/.zsh/completions
warden completions zsh > ~/.zsh/completions/_warden

## Option 3: System-wide (requires root)
warden completions zsh | sudo tee /usr/local/share/zsh/site-functions/_warden > /dev/null
"#
        .to_string(),

        Shell::Fish => r#"
# Fish Completion Installation

## User-local (recommended)
warden completions fish > ~/.config/fish/completions/warden.fish

## System-wide (requires root)
warden completions fish | sudo tee /usr/share/fish/vendor_completions.d/warden.fish > /dev/null
"#
        .to_string(),

        Shell::Elvish => r#"
# Elvish Completion Installation

# Add to ~/.elvish/rc.elv:
eval (warden completions elvish | slurp)
"#
        .to_string(),

        Shell::PowerShell => r#"
# PowerShell Completion Installation

# Add to your PowerShell profile ($PROFILE):
warden completions powershell | Out-String | Invoke-Expression

# Or save to a file and source it:
warden completions powershell > warden.ps1
. ./warden.ps1
"#
        .to_string(),

        _ => "No installation instructions available for this shell.".to_string(),
    }
}
