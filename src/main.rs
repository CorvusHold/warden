use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

mod cli;

/// Warden - PostgreSQL backup, restore, and HA orchestration tool
///
/// Warden provides comprehensive data protection for PostgreSQL databases:
/// - Snapshot and incremental backups
/// - Point-in-Time Recovery (PITR)
/// - S3-compatible remote storage
/// - High Availability cluster management
/// - Automated backup scheduling and retention
///
/// For detailed documentation on specific topics, use: warden docs <topic>
/// Available topics: backup, pitr, ha, config, storage, retention
#[derive(Parser, Debug)]
#[clap(
    name = "warden",
    version,
    author = "Corvus",
    about = "PostgreSQL backup, restore, and HA orchestration",
    long_about = "Warden - PostgreSQL backup, restore, and HA orchestration tool\n\n\
        Warden provides comprehensive data protection for PostgreSQL databases:\n\
        • Snapshot and incremental backups\n\
        • Point-in-Time Recovery (PITR)\n\
        • S3-compatible remote storage\n\
        • High Availability cluster management\n\
        • Automated backup scheduling and retention\n\n\
        For detailed documentation on specific topics, use: warden docs <topic>\n\
        Available topics: backup, pitr, ha, config, storage, retention",
    after_help = "EXAMPLES:\n    \
        # Create a snapshot backup\n    \
        warden postgresql snapshot-backup --database mydb --user postgres\n\n    \
        # List available backups\n    \
        warden postgresql backups list --storage-bucket my-backups\n\n    \
        # Restore to a point in time\n    \
        warden postgresql pitr-restore --target-time 2025-01-15T10:30:00Z --target-dir ./recovered\n\n    \
        # Get extended documentation on a topic\n    \
        warden docs backup\n\n\
        For more information, visit: https://github.com/CorvusHold/warden"
)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Console commands for device enrollment and configuration
    ///
    /// Manage device enrollment with Corvus Hold, view status,
    /// and configure local settings.
    #[clap(subcommand)]
    Console(ConsoleCommands),

    /// PostgreSQL backup, restore, and HA commands
    ///
    /// Comprehensive PostgreSQL data protection including:
    /// - Snapshot and incremental backups
    /// - Point-in-Time Recovery (PITR)
    /// - Retention policy management
    /// - High Availability orchestration
    /// - Cluster configuration management
    #[clap(subcommand, visible_alias = "pg")]
    Postgresql(Box<postgres::cli::PostgresqlCommands>),

    /// SSH tunneling and port forwarding
    ///
    /// Establish SSH tunnels for secure database connections.
    Ssh {
        #[clap(subcommand)]
        command: SshCommands,
    },

    /// Start the warden daemon in the background
    ///
    /// Starts the daemon process which handles scheduled backups,
    /// retention policies, and C2 communication.
    Start,

    /// Stop the warden daemon
    ///
    /// Gracefully stops the running daemon process.
    Stop,

    /// Restart the warden daemon
    ///
    /// Stops and then starts the daemon process.
    Restart,

    /// Run the warden daemon in the foreground
    ///
    /// Runs the daemon in the current terminal session.
    /// Useful for debugging or running in containers.
    Run,

    /// Generate shell completion scripts
    ///
    /// Outputs completion scripts for various shells.
    /// See 'warden completions --help' for installation instructions.
    ///
    /// Examples:
    ///   warden completions bash > /etc/bash_completion.d/warden
    ///   warden completions zsh > ~/.zsh/completions/_warden
    ///   warden completions fish > ~/.config/fish/completions/warden.fish
    #[clap(verbatim_doc_comment)]
    Completions {
        /// Shell to generate completions for
        #[clap(value_enum)]
        shell: ShellType,

        /// Show installation instructions instead of completions
        #[clap(long, short = 'i')]
        install: bool,
    },

    /// Show extended documentation for a topic
    ///
    /// Provides detailed documentation for complex features.
    ///
    /// Available topics:
    ///   backup    - Backup concepts, types, and workflows
    ///   pitr      - Point-in-Time Recovery concepts and examples
    ///   ha        - High Availability orchestration guide
    ///   config    - Configuration file reference
    ///   storage   - S3-compatible storage setup
    ///   retention - Retention policies and purge operations
    ///
    /// Examples:
    ///   warden docs backup
    ///   warden docs pitr
    #[clap(verbatim_doc_comment, name = "docs", visible_alias = "doc")]
    Docs {
        /// Topic to get help for (backup, pitr, ha, config, storage, retention)
        topic: Option<String>,
    },

    /// Manage data source plugins
    ///
    /// List available plugins and show detailed information about
    /// specific data source plugins.
    ///
    /// Examples:
    ///   warden plugins list
    ///   warden plugins info postgresql
    #[clap(subcommand, verbatim_doc_comment)]
    Plugins(PluginsCommands),
}

#[derive(Subcommand, Debug)]
enum PluginsCommands {
    /// List all available data source plugins
    ///
    /// Shows a table of registered plugins with their capabilities.
    List,

    /// Show detailed information about a plugin
    ///
    /// Displays comprehensive information including capabilities,
    /// supported backup types, and custom features.
    Info {
        /// Name of the plugin to show info for (e.g., postgresql)
        name: String,
    },
}

/// Shell types for completion generation
#[derive(Debug, Clone, Copy, ValueEnum)]
enum ShellType {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// Elvish shell
    Elvish,
    /// PowerShell
    #[clap(name = "powershell")]
    PowerShell,
}

impl From<ShellType> for Shell {
    fn from(shell: ShellType) -> Self {
        match shell {
            ShellType::Bash => Shell::Bash,
            ShellType::Zsh => Shell::Zsh,
            ShellType::Fish => Shell::Fish,
            ShellType::Elvish => Shell::Elvish,
            ShellType::PowerShell => Shell::PowerShell,
        }
    }
}

#[derive(Subcommand, Debug)]
enum ConsoleCommands {
    /// Enroll this device with Corvus Hold
    ///
    /// Registers this device with the Corvus Hold control plane,
    /// enabling remote management and monitoring.
    Enroll(console::cli::commands::enroll::Enroll),

    /// Show the current status of the Warden service
    ///
    /// Displays connection status, enabled features, and
    /// recent activity.
    Status(console::cli::commands::status::Status),

    /// Enable or disable the Warden service
    ///
    /// Toggle the agent on or off without uninstalling.
    Toggle(console::cli::commands::toggle::Toggle),

    /// View or modify Warden configuration
    ///
    /// Get or set configuration values like C2 server URL,
    /// authentication credentials, and feature flags.
    ///
    /// Examples:
    ///   warden console config get
    ///   warden console config get --format json
    ///   warden console config set c2_server "https://hold.corvus.io"
    #[clap(verbatim_doc_comment)]
    Config(console::cli::commands::config::Config),

    /// Manage notification channels
    ///
    /// Configure and test notification channels for alerting
    /// operators about backup failures, HA events, and other
    /// important operations.
    ///
    /// Examples:
    ///   warden console notifications list
    ///   warden console notifications test --channel ops-webhook
    ///   warden console notifications validate
    #[clap(verbatim_doc_comment)]
    Notifications(console::cli::commands::notifications::Notifications),
}

#[derive(Subcommand, Debug)]
enum SshCommands {
    /// Forward a remote port to a local port over SSH
    ///
    /// Creates an SSH tunnel for secure database connections.
    /// Useful for connecting to databases behind firewalls.
    ///
    /// Examples:
    ///   # Forward remote PostgreSQL to local port
    ///   warden ssh forward --ssh-host bastion.example.com --ssh-user ubuntu \
    ///     --remote-host db.internal --remote-port 5432 --remote-key-path ~/.ssh/id_rsa
    #[clap(verbatim_doc_comment)]
    Forward {
        #[clap(flatten)]
        cmd: ssh::cli::forward::ForwardCommand,
    },
}

use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    // --- Sentry initialization ---
    let sentry_dsn = env::var("SENTRY_DSN").ok();
    let _sentry_guard = if let Some(dsn) = sentry_dsn {
        let env = env::var("SENTRY_ENVIRONMENT").unwrap_or_else(|_| "development".into());
        let release = env!("CARGO_PKG_VERSION");
        let guard = sentry::init(sentry::ClientOptions {
            dsn: Some(dsn.parse().expect("Invalid SENTRY_DSN")),
            environment: Some(env.into()),
            release: Some(release.into()),
            attach_stacktrace: true,
            ..Default::default()
        });
        // Integrate sentry-log for breadcrumbs
        let logger =
            sentry_log::SentryLogger::with_dest(env_logger::Builder::from_default_env().build());
        log::set_boxed_logger(Box::new(logger)).expect("Failed to set logger");
        log::set_max_level(log::LevelFilter::Info);
        Some(guard)
    } else {
        None
    };

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
            ConsoleCommands::Notifications(notifications) => {
                notifications.run().await?;
            }
        },
        Commands::Postgresql(postgres_command) => match *postgres_command {
            postgres::cli::PostgresqlCommands::FullBackup {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                backup_dir,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::full_backup(
                    host, port, database, user, password, ssl_mode, backup_dir, ssh, storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::IncrementalBackup {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::incremental_backup(
                    host, port, database, user, password, ssl_mode, backup_dir, ssh, storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::SnapshotBackup {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                labels,
                tenant,
                cluster,
                protection_group,
            } => {
                log::info!("[CLI] Starting snapshot-backup command...");
                log::info!("[CLI] Parameters: host={host}, port={port}, database={database}, user={user}, backup_dir={backup_dir:?}, remote_storage={remote_storage}");
                if !labels.is_empty() {
                    log::info!("[CLI] Labels: {:?}", labels);
                }
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions {
                        tenant,
                        cluster,
                        protection_group,
                        include_legacy: false,
                    },
                };
                let labels_map: std::collections::HashMap<String, String> = labels.into_iter().collect();
                match postgres::cli::commands::snapshot_backup(
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir.clone(),
                    ssh,
                    storage,
                    labels_map,
                )
                .await
                {
                    Ok(result) => {
                        // Print structured output for scripting
                        println!("backup_id={}", result.backup_id);
                        println!("local_path={}", result.local_path.display());
                        if let Some(remote_path) = &result.remote_path {
                            println!("remote_path={}", remote_path);
                        }
                        log::info!(
                            "[CLI] snapshot-backup completed successfully. Backup ID: {}",
                            result.backup_id
                        );
                    }
                    Err(e) => {
                        log::error!("[CLI] snapshot-backup failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ListBackups {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::list_backups(
                    host, port, database, user, password, ssl_mode, backup_dir, ssh, storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::RestoreFull {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                backup_id,
                target_dir,
                container_id,
                container_type,
                auto_restart,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                yes,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::restore_full(
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir,
                    backup_id,
                    target_dir,
                    container_id,
                    container_type,
                    auto_restart,
                    ssh,
                    storage,
                    yes,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::RestoreIncremental {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                full_backup_id,
                target_dir,
                container_id,
                container_type,
                auto_restart,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::restore_incremental(
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir,
                    full_backup_id,
                    target_dir,
                    container_id,
                    container_type,
                    auto_restart,
                    ssh,
                    storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::RestorePointInTime {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                full_backup_id,
                target_dir,
                target_time,
                container_id,
                container_type,
                auto_restart,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::restore_point_in_time(
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir,
                    full_backup_id,
                    target_dir,
                    target_time,
                    container_id,
                    container_type,
                    auto_restart,
                    ssh,
                    storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::RestoreSnapshot {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                backup_id,
                target_dir,
                container_id,
                container_type,
                auto_restart,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::restore_snapshot(
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir,
                    backup_id,
                    target_dir,
                    container_id,
                    container_type,
                    auto_restart,
                    ssh,
                    storage,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::ListSnapshotContents {
                host,
                port,
                database,
                user,
                password,
                ssl_mode,
                backup_dir,
                backup_id,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                let _ = postgres::cli::commands::list_snapshot_contents(
                    host, port, database, user, password, ssl_mode, backup_dir, backup_id, ssh,
                    storage,
                )
                .await;
            }
            postgres::cli::PostgresqlCommands::InspectBackup {
                backup_id,
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::inspect_backup(storage, backup_id).await?;
            }
            postgres::cli::PostgresqlCommands::DownloadBackup {
                backup_id,
                target_dir,
                verify_checksums,
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::download_backup(
                    storage,
                    backup_id,
                    target_dir,
                    verify_checksums,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::InitRetentionPolicy {
                policy_file,
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::init_retention_policy(storage, policy_file).await?;
            }
            postgres::cli::PostgresqlCommands::ShowRetentionPolicy {
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::show_retention_policy(storage).await?;
            }
            postgres::cli::PostgresqlCommands::PurgePlan {
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::purge_plan(storage, format).await?;
            }
            postgres::cli::PostgresqlCommands::Purge {
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                apply,
                yes,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::purge(storage, apply, yes).await?;
            }
            postgres::cli::PostgresqlCommands::ReconstructMetadata {
                backup_dir: _,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                server_version,
                dry_run,
                skip_checksums,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                postgres::cli::commands::reconstruct_metadata(
                    storage,
                    server_version,
                    dry_run,
                    skip_checksums,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::PitrPlan {
                target_time,
                backup_dir,
                wal_archive_dir,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                wal_prefix,
                format,
            } => {
                let storage_opts = postgres::cli::commands::PitrStorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    wal_prefix,
                };
                postgres::cli::commands::pitr_plan(
                    target_time,
                    backup_dir,
                    wal_archive_dir,
                    storage_opts,
                    format,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::PitrRestore {
                target_time,
                target_dir,
                backup_dir,
                wal_archive_dir,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                wal_prefix,
                auto_start,
                pg_bin_dir,
                yes,
                interactive,
            } => {
                // Handle interactive mode
                let (final_target_time, final_target_dir, final_auto_start, final_yes) = if interactive {
                    match cli::interactive::pitr_restore_wizard() {
                        Ok(config) => {
                            if !config.confirmed {
                                log::info!("PITR restore cancelled by user");
                                return Ok(());
                            }
                            (
                                config.target_time,
                                std::path::PathBuf::from(config.target_dir),
                                config.auto_start,
                                true, // User confirmed in wizard
                            )
                        }
                        Err(e) => {
                            log::error!("Interactive mode failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    (
                        target_time.expect("target_time required when not in interactive mode"),
                        target_dir.expect("target_dir required when not in interactive mode"),
                        auto_start,
                        yes,
                    )
                };

                let storage_opts = postgres::cli::commands::PitrStorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    wal_prefix,
                };
                postgres::cli::commands::pitr_restore(
                    final_target_time,
                    final_target_dir,
                    backup_dir,
                    wal_archive_dir,
                    storage_opts,
                    final_auto_start,
                    pg_bin_dir,
                    final_yes,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::PitrList {
                backup_dir,
                wal_archive_dir,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                wal_prefix,
                format,
            } => {
                let storage_opts = postgres::cli::commands::PitrStorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    wal_prefix,
                };
                postgres::cli::commands::pitr_list(
                    backup_dir,
                    wal_archive_dir,
                    storage_opts,
                    format,
                )
                .await?;
            }
            postgres::cli::PostgresqlCommands::FullRestore {
                backup_id,
                host,
                port,
                database,
                target_database,
                user,
                password,
                ssl_mode,
                backup_dir,
                yes,
                dry_run,
                format,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
            } => {
                log::info!("[CLI] Starting full-restore command...");
                log::info!(
                    "[CLI] Parameters: backup_id={}, host={}, port={}, database={}, dry_run={}",
                    backup_id, host, port, database, dry_run
                );
                let ssh = postgres::cli::commands::SshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: storage_provider,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                match postgres::cli::commands::execute_full_restore(
                    backup_id,
                    host,
                    port,
                    database,
                    user,
                    password,
                    ssl_mode,
                    backup_dir,
                    target_database,
                    yes,
                    dry_run,
                    format,
                    ssh,
                    storage,
                )
                .await
                {
                    Ok(_) => {
                        log::info!("[CLI] full-restore completed successfully");
                    }
                    Err(e) => {
                        log::error!("[CLI] full-restore failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::RetentionPlan {
                policy_file,
                backup_dir,
                wal_archive_dir,
                include_local,
                include_remote,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                let opts = postgres::cli::commands::RetentionOptions {
                    policy_file,
                    backup_dir,
                    wal_archive_dir,
                    include_local,
                    include_remote,
                    format: format.clone(),
                };
                match postgres::cli::commands::retention_plan(storage, opts).await {
                    Ok(result) => {
                        let output = postgres::cli::commands::format_retention_plan(&result, &format);
                        println!("{}", output);
                    }
                    Err(e) => {
                        log::error!("[CLI] retention-plan failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::RetentionApply {
                policy_file,
                backup_dir,
                wal_archive_dir,
                include_local,
                include_remote,
                remote_storage,
                storage_provider,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                apply,
                yes,
                format,
            } => {
                let storage = postgres::cli::commands::StorageOptions {
                    remote_storage,
                    provider_type: Some(storage_provider),
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                let opts = postgres::cli::commands::RetentionOptions {
                    policy_file,
                    backup_dir,
                    wal_archive_dir,
                    include_local,
                    include_remote,
                    format: format.clone(),
                };
                match postgres::cli::commands::retention_apply(storage, opts, !apply, yes).await {
                    Ok(_) => {
                        log::info!("[CLI] retention-apply completed successfully");
                    }
                    Err(e) => {
                        log::error!("[CLI] retention-apply failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::RetentionInit {
                output,
                preset,
                format,
            } => {
                match postgres::cli::commands::retention_init(&output, &preset, &format) {
                    Ok(_) => {
                        log::info!("[CLI] retention-init completed successfully");
                    }
                    Err(e) => {
                        log::error!("[CLI] retention-init failed: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::Backups(backups_cmd) => {
                match backups_cmd {
                    postgres::cli::BackupsCommands::List {
                        storage_bucket,
                        storage_prefix,
                        storage_region,
                        storage_endpoint,
                        storage_access_key,
                        storage_secret_key,
                        backup_type,
                        database,
                        status,
                        after,
                        before,
                        labels,
                        limit,
                        format,
                    } => {
                        let storage_opts = postgres::cli::commands::backups::BackupStorageOptions {
                            provider_type: Some("s3".to_string()),
                            bucket: storage_bucket,
                            prefix: storage_prefix,
                            region: storage_region,
                            endpoint: storage_endpoint,
                            access_key: storage_access_key,
                            secret_key: storage_secret_key,
                        };
                        let list_opts = postgres::cli::commands::backups::BackupListOptions {
                            backup_type,
                            database,
                            status,
                            after,
                            before,
                            labels,
                            limit,
                            format: format.clone(),
                        };
                        match postgres::cli::commands::backups::list_backups(&storage_opts, &list_opts).await {
                            Ok(result) => {
                                let output = if format == "json" {
                                    postgres::cli::commands::backups::format_list_json(&result)
                                        .unwrap_or_else(|e| format!("Error: {}", e))
                                } else {
                                    postgres::cli::commands::backups::format_list_table(&result)
                                };
                                println!("{}", output);
                            }
                            Err(e) => {
                                log::error!("[CLI] backups list failed: {}", e);
                                eprintln!("Error: {}", e);
                                std::process::exit(4); // Remote service error
                            }
                        }
                    }
                    postgres::cli::BackupsCommands::Show {
                        backup_id,
                        storage_bucket,
                        storage_prefix,
                        storage_region,
                        storage_endpoint,
                        storage_access_key,
                        storage_secret_key,
                        format,
                    } => {
                        let storage_opts = postgres::cli::commands::backups::BackupStorageOptions {
                            provider_type: Some("s3".to_string()),
                            bucket: storage_bucket,
                            prefix: storage_prefix,
                            region: storage_region,
                            endpoint: storage_endpoint,
                            access_key: storage_access_key,
                            secret_key: storage_secret_key,
                        };
                        match postgres::cli::commands::backups::show_backup(&storage_opts, &backup_id).await {
                            Ok(result) => {
                                let output = if format == "json" {
                                    postgres::cli::commands::backups::format_show_json(&result)
                                        .unwrap_or_else(|e| format!("Error: {}", e))
                                } else {
                                    postgres::cli::commands::backups::format_show_table(&result)
                                };
                                println!("{}", output);
                            }
                            Err(e) => {
                                log::error!("[CLI] backups show failed: {}", e);
                                eprintln!("Error: {}", e);
                                // Check if it's a not found error
                                if e.to_string().contains("not found") {
                                    std::process::exit(4); // Remote service error - backup not found
                                } else {
                                    std::process::exit(4); // Remote service error
                                }
                            }
                        }
                    }
                    postgres::cli::BackupsCommands::Download {
                        backup_id,
                        output,
                        verify_checksums,
                        storage_bucket,
                        storage_prefix,
                        storage_region,
                        storage_endpoint,
                        storage_access_key,
                        storage_secret_key,
                        format,
                    } => {
                        let storage_opts = postgres::cli::commands::backups::BackupStorageOptions {
                            provider_type: Some("s3".to_string()),
                            bucket: storage_bucket,
                            prefix: storage_prefix,
                            region: storage_region,
                            endpoint: storage_endpoint,
                            access_key: storage_access_key,
                            secret_key: storage_secret_key,
                        };
                        match postgres::cli::commands::backups::download_backup(
                            &storage_opts,
                            &backup_id,
                            &output,
                            verify_checksums,
                        ).await {
                            Ok(result) => {
                                let output_str = if format == "json" {
                                    postgres::cli::commands::backups::format_download_json(&result)
                                        .unwrap_or_else(|e| format!("Error: {}", e))
                                } else {
                                    postgres::cli::commands::backups::format_download_table(&result)
                                };
                                println!("{}", output_str);
                                
                                // Exit with error if checksum verification failed
                                if let Some(ref checksum_result) = result.checksum_verified {
                                    if !checksum_result.all_matched {
                                        std::process::exit(5); // Internal error - checksum mismatch
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!("[CLI] backups download failed: {}", e);
                                eprintln!("Error: {}", e);
                                if e.to_string().contains("not found") {
                                    std::process::exit(4); // Remote service error - backup not found
                                } else if e.to_string().contains("checksum") {
                                    std::process::exit(5); // Internal error - checksum mismatch
                                } else {
                                    std::process::exit(4); // Remote service error
                                }
                            }
                        }
                    }
                    // HA commands are defined in BackupsCommands but should be in PostgresqlCommands
                    // These are placeholders until the CLI structure is fixed
                    postgres::cli::BackupsCommands::HaSwitchover { .. } => {
                        log::error!("[CLI] ha-switchover is not yet implemented in this context");
                        eprintln!("Error: ha-switchover command is not available through 'backups' subcommand");
                        std::process::exit(1);
                    }
                    postgres::cli::BackupsCommands::HaFailover { .. } => {
                        log::error!("[CLI] ha-failover is not yet implemented in this context");
                        eprintln!("Error: ha-failover command is not available through 'backups' subcommand");
                        std::process::exit(1);
                    }
                    postgres::cli::BackupsCommands::HaCloneNode { .. } => {
                        log::error!("[CLI] ha-clone-node is not yet implemented in this context");
                        eprintln!("Error: ha-clone-node command is not available through 'backups' subcommand");
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ClusterValidate { config, interactive } => {
                // Handle interactive mode
                let config_path = if interactive {
                    match cli::interactive::cluster_validate_wizard() {
                        Ok(wizard_config) => wizard_config.config_path.map(std::path::PathBuf::from),
                        Err(e) => {
                            log::error!("Interactive mode failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                } else {
                    config
                };

                let config_path_ref = config_path.as_deref();
                match postgres::cli::commands::cluster_validate(config_path_ref) {
                    Ok(result) => {
                        let output = postgres::cli::commands::format_validation_result(
                            &result,
                            postgres::cli::commands::OutputFormat::Table,
                        );
                        println!("{}", output);
                        if !result.valid {
                            if interactive {
                                cli::interactive::warn("Configuration has errors. Review the issues above and fix your cluster.yaml file.");
                            }
                            std::process::exit(2); // Configuration error
                        } else if interactive {
                            cli::interactive::success("Configuration is valid!");
                        }
                    }
                    Err(e) => {
                        log::error!("[CLI] cluster-validate failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(5); // Internal error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ClusterShow { config, format } => {
                let config_path = config.as_deref();
                let output_format = postgres::cli::commands::OutputFormat::from_str(&format);
                match postgres::cli::commands::cluster_show(config_path) {
                    Ok(overview) => {
                        let output = postgres::cli::commands::format_cluster_overview(&overview, output_format);
                        println!("{}", output);
                    }
                    Err(e) => {
                        log::error!("[CLI] cluster-show failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ClusterNodes { config, cluster, role, format } => {
                let config_path = config.as_deref();
                let output_format = postgres::cli::commands::OutputFormat::from_str(&format);
                match postgres::cli::commands::cluster_nodes(config_path, cluster.as_deref(), role.as_deref()) {
                    Ok(list) => {
                        let output = postgres::cli::commands::format_node_list(&list, output_format);
                        println!("{}", output);
                    }
                    Err(e) => {
                        log::error!("[CLI] cluster-nodes failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ClusterProtectionGroups { config, cluster, format } => {
                let config_path = config.as_deref();
                let output_format = postgres::cli::commands::OutputFormat::from_str(&format);
                match postgres::cli::commands::cluster_protection_groups(config_path, cluster.as_deref()) {
                    Ok(list) => {
                        let output = postgres::cli::commands::format_protection_group_list(&list, output_format);
                        println!("{}", output);
                    }
                    Err(e) => {
                        log::error!("[CLI] cluster-protection-groups failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ScheduleList { format, enabled_only, schedule_type } => {
                match postgres::cli::commands::schedule_list(format, enabled_only, schedule_type).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] schedule-list failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ScheduleNextRuns { count, format, enabled_only } => {
                match postgres::cli::commands::schedule_next_runs(count, format, enabled_only).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] schedule-next-runs failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ScheduleValidate => {
                match postgres::cli::commands::schedule_validate().await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] schedule-validate failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(2); // Configuration error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ScheduleRun { id, dry_run } => {
                match postgres::cli::commands::schedule_run(id, dry_run).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] schedule-run failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(3); // Environment error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::Status {
                backup_dir,
                wal_archive_dir,
                retention_policy,
                database,
                host,
                remote_storage,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
                backup_warning_age_hours,
                backup_critical_age_hours,
                tenant,
                cluster,
                protection_group,
                include_legacy,
            } => {
                let storage_opts = postgres::cli::commands::StatusStorageOptions {
                    remote_storage,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions {
                        tenant,
                        cluster,
                        protection_group,
                        include_legacy,
                    },
                };
                match postgres::cli::commands::execute_status(
                    backup_dir,
                    wal_archive_dir,
                    retention_policy,
                    database,
                    host,
                    storage_opts,
                    format,
                    backup_warning_age_hours,
                    backup_critical_age_hours,
                ).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] status failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(3); // Environment error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::BackupStatus {
                backup_dir,
                database,
                remote_storage,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
            } => {
                let storage_opts = postgres::cli::commands::StatusStorageOptions {
                    remote_storage,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                match postgres::cli::commands::execute_backup_status(
                    backup_dir,
                    database,
                    storage_opts,
                    format,
                ).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] backup-status failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(3); // Environment error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::PitrStatus {
                backup_dir,
                wal_archive_dir,
                database,
                remote_storage,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
            } => {
                let storage_opts = postgres::cli::commands::StatusStorageOptions {
                    remote_storage,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                match postgres::cli::commands::execute_pitr_status(
                    backup_dir,
                    wal_archive_dir,
                    database,
                    storage_opts,
                    format,
                ).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] pitr-status failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(3); // Environment error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::Metrics {
                backup_dir,
                wal_archive_dir,
                database,
                host,
                remote_storage,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                output,
                format,
            } => {
                let storage_opts = postgres::cli::commands::StatusStorageOptions {
                    remote_storage,
                    bucket: storage_bucket,
                    prefix: storage_prefix,
                    region: storage_region,
                    endpoint: storage_endpoint,
                    access_key: storage_access_key,
                    secret_key: storage_secret_key,
                    multi_tenant: postgres::cli::commands::MultiTenantOptions::default(),
                };
                match postgres::cli::commands::execute_metrics(
                    backup_dir,
                    wal_archive_dir,
                    database,
                    host,
                    storage_opts,
                    output,
                    format,
                ).await {
                    Ok(()) => {}
                    Err(e) => {
                        log::error!("[CLI] metrics failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(3); // Environment error
                    }
                }
            }
            postgres::cli::PostgresqlCommands::Discover {
                host,
                port,
                user,
                password,
                ssl_mode,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                format,
            } => {
                log::info!("[CLI] Starting discover command for {}:{}", host, port);
                let ssh = postgres::cli::commands::MigrationSshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                match postgres::cli::commands::discover(
                    host,
                    port,
                    user,
                    password,
                    ssl_mode,
                    ssh,
                ).await {
                    Ok(result) => {
                        println!("{}", postgres::cli::commands::format_discovery_result(&result, &format));
                    }
                    Err(e) => {
                        log::error!("[CLI] discover failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::GenerateConfig {
                host,
                port,
                user,
                password,
                ssl_mode,
                cluster_name,
                tenant,
                output,
                interactive: _interactive,
                ssh_host,
                ssh_user,
                ssh_port,
                ssh_password,
                ssh_key_path,
                ssh_local_port,
                ssh_remote_port,
                format,
            } => {
                log::info!("[CLI] Starting generate-config command for {}:{}", host, port);
                let ssh = postgres::cli::commands::MigrationSshOptions {
                    host: ssh_host,
                    user: ssh_user,
                    port: ssh_port,
                    password: ssh_password,
                    key_path: ssh_key_path,
                    local_port: ssh_local_port,
                    remote_port: ssh_remote_port,
                };
                let options = postgres::cli::commands::GenerateConfigOptions {
                    host,
                    port,
                    user,
                    password,
                    ssl_mode,
                    cluster_name,
                    tenant,
                    output_path: output,
                    interactive: false, // TODO: implement interactive mode
                    ssh,
                };
                match postgres::cli::commands::generate_config(options).await {
                    Ok(result) => {
                        println!("{}", postgres::cli::commands::format_generated_config(&result, &format));
                    }
                    Err(e) => {
                        log::error!("[CLI] generate-config failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            postgres::cli::PostgresqlCommands::ImportBackup {
                source,
                backup_type,
                database,
                tenant,
                cluster,
                storage_profile: _storage_profile,
                backup_dir,
                remote_storage,
                storage_bucket,
                storage_prefix,
                storage_region,
                storage_endpoint,
                storage_access_key,
                storage_secret_key,
                format,
            } => {
                log::info!("[CLI] Starting import-backup command for {}", source);
                let backup_type: postgres::cli::commands::ImportBackupType = backup_type
                    .parse()
                    .unwrap_or_else(|e| {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    });
                let storage = if remote_storage {
                    Some(postgres::cli::commands::MigrationStorageOptions {
                        enabled: true,
                        provider: Some("s3".to_string()),
                        bucket: storage_bucket,
                        prefix: storage_prefix,
                        region: storage_region,
                        endpoint: storage_endpoint,
                        access_key: storage_access_key,
                        secret_key: storage_secret_key,
                    })
                } else {
                    None
                };
                let options = postgres::cli::commands::ImportBackupOptions {
                    source,
                    backup_type,
                    database,
                    tenant,
                    cluster,
                    storage_profile: None,
                    backup_dir,
                    storage,
                };
                match postgres::cli::commands::import_backup(options).await {
                    Ok(result) => {
                        println!("{}", postgres::cli::commands::format_import_result(&result, &format));
                    }
                    Err(e) => {
                        log::error!("[CLI] import-backup failed: {}", e);
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Ssh {
            command: SshCommands::Forward { cmd },
        } => {
            ssh::cli::forward::forward(cmd).await?;
        }
        Commands::Run => {
            log::info!("Running warden daemon in the foreground...");
            daemon::cli::run::execute().await?;
        }
        Commands::Start => {
            log::info!("Starting daemonization process...");
            daemon::cli::start::execute().await?;
        }
        Commands::Stop => {
            log::info!("Stopping warden daemon...");
            daemon::cli::stop::execute().await?;
        }
        Commands::Restart => {
            log::info!("Restarting warden daemon...");
            // First stop the daemon
            daemon::cli::stop::execute().await?;
            // Then start it again
            daemon::cli::start::execute().await?;
        }
        Commands::Completions { shell, install } => {
            let shell: Shell = shell.into();
            if install {
                println!("{}", cli::completions::get_installation_instructions(shell));
            } else {
                let mut cmd = Cli::command();
                cli::completions::generate_completions(shell, &mut cmd);
            }
        }
        Commands::Docs { topic } => {
            match topic {
                Some(t) => {
                    if let Some(help) = cli::help_topics::get_topic_help(&t) {
                        println!("{}", help);
                    } else {
                        eprintln!("Unknown topic: {}", t);
                        eprintln!();
                        println!("{}", cli::help_topics::list_topics());
                        std::process::exit(1);
                    }
                }
                None => {
                    println!("{}", cli::help_topics::list_topics());
                }
            }
        }
        Commands::Plugins(plugins_cmd) => {
            let registry = cli::plugins::init_registry();
            match plugins_cmd {
                PluginsCommands::List => {
                    cli::plugins::list_plugins(&registry);
                }
                PluginsCommands::Info { name } => {
                    cli::plugins::show_plugin_info(&registry, &name);
                }
            }
        }
    }

    Ok(())
}
