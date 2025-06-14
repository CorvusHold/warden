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

    /// PostgreSQL backup and restore commands
    #[clap(subcommand)]
    Postgresql(Box<postgres::cli::PostgresqlCommands>),

    /// Commands for interacting with SSH.
    Ssh {
        #[clap(subcommand)]
        command: SshCommands,
    },

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

#[derive(Subcommand, Debug)]
enum SshCommands {
    /// Forwards a remote port to a local port over SSH.
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
            } => {
                eprintln!("[CLI] Starting snapshot-backup command...");
                eprintln!("[CLI] Parameters: host={}, port={}, database={}, user={}, backup_dir={:?}, remote_storage={}", host, port, database, user, backup_dir, remote_storage);
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
                };
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
                )
                .await
                {
                    Ok(_) => {
                        eprintln!(
                            "[CLI] snapshot-backup completed successfully. Backup dir: {:?}",
                            backup_dir
                        );
                    }
                    Err(e) => {
                        eprintln!("[CLI] snapshot-backup failed: {}", e);
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
                };
                let _ = postgres::cli::commands::list_snapshot_contents(
                    host, port, database, user, password, ssl_mode, backup_dir, backup_id, ssh,
                    storage,
                )
                .await;
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
    }

    Ok(())
}
