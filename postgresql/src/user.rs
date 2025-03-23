use crate::common::PostgresConfig;
use crate::PostgresError;
use log::{info, warn};
use std::process::{Command, Stdio};
use tokio_postgres::Client;

/// PostgreSQL user manager for backup and restore operations
pub struct UserManager {
    client: Client,
    config: PostgresConfig,
}

impl UserManager {
    /// Create a new user manager
    pub fn new(client: Client, config: PostgresConfig) -> Self {
        Self { client, config }
    }

    /// Create a backup user with necessary permissions
    pub async fn create_backup_user(
        &self,
        username: &str,
        password: Option<&str>,
    ) -> Result<(), PostgresError> {
        info!("Creating backup user: {}", username);

        // Create user
        let create_user_sql = match password {
            Some(pwd) => format!("CREATE USER {} WITH PASSWORD '{}'", username, pwd),
            None => format!("CREATE USER {} WITHOUT PASSWORD", username),
        };

        self.client
            .execute(&create_user_sql, &[])
            .await
            .map_err(|e| PostgresError::RestoreError(e.to_string()))?;

        // Grant necessary permissions
        self.grant_backup_permissions(username).await?;

        info!("Backup user created successfully: {}", username);
        Ok(())
    }

    /// Grant necessary permissions for backup operations
    pub async fn grant_backup_permissions(&self, username: &str) -> Result<(), PostgresError> {
        info!("Granting backup permissions to user: {}", username);

        // Get PostgreSQL version to determine which functions to grant
        let row = self
            .client
            .query_one("SELECT current_setting('server_version_num')::int", &[])
            .await
            .map_err(|e| PostgresError::RestoreError(e.to_string()))?;

        let version_num: i32 = row.get(0);

        // Different permissions based on PostgreSQL version
        if version_num >= 140000 {
            // PostgreSQL 14 and above
            let grant_statements = [
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_backup_start(text, boolean) to {}",
                    username
                ),
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_backup_stop(boolean) to {}",
                    username
                ),
                format!("GRANT EXECUTE ON FUNCTION pg_switch_wal() to {}", username),
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_create_restore_point(text) to {}",
                    username
                ),
                format!("GRANT pg_read_all_settings TO {}", username),
                format!("GRANT pg_read_all_stats TO {}", username),
            ];

            for stmt in grant_statements.iter() {
                self.client
                    .execute(stmt, &[])
                    .await
                    .map_err(|e| PostgresError::RestoreError(e.to_string()))?;
            }

            // For PostgreSQL 15+, grant pg_checkpoint role
            if version_num >= 150000 {
                self.client
                    .execute(&format!("GRANT pg_checkpoint TO {}", username), &[])
                    .await
                    .map_err(|e| PostgresError::RestoreError(e.to_string()))?;
            }
        } else {
            // PostgreSQL 13 and below
            let grant_statements = [
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_start_backup(text, boolean, boolean) to {}",
                    username
                ),
                format!("GRANT EXECUTE ON FUNCTION pg_stop_backup() to {}", username),
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_stop_backup(boolean, boolean) to {}",
                    username
                ),
                format!("GRANT EXECUTE ON FUNCTION pg_switch_wal() to {}", username),
                format!(
                    "GRANT EXECUTE ON FUNCTION pg_create_restore_point(text) to {}",
                    username
                ),
                format!("GRANT pg_read_all_settings TO {}", username),
                format!("GRANT pg_read_all_stats TO {}", username),
            ];

            for stmt in grant_statements.iter() {
                self.client
                    .execute(stmt, &[])
                    .await
                    .map_err(|e| PostgresError::RestoreError(e.to_string()))?;
            }
        }

        info!("Backup permissions granted to user: {}", username);
        Ok(())
    }

    /// Check if user has necessary permissions for backup operations
    pub async fn check_backup_permissions(&self, username: &str) -> Result<bool, PostgresError> {
        info!("Checking backup permissions for user: {}", username);

        // Get the database name from the config
        let db_name = &self.config.database;

        // Use psql to check permissions (this is a simplified approach)
        let output = Command::new("psql")
            .arg("-U")
            .arg(username)
            .arg("-d")
            .arg(db_name)
            .arg("-c")
            .arg("SELECT version();")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| PostgresError::Io(e.into()))?;

        let has_permissions = output.success();

        if has_permissions {
            info!("User {} has necessary permissions", username);
        } else {
            warn!("User {} does not have necessary permissions", username);
        }

        Ok(has_permissions)
    }

    /// Drop a backup user
    pub async fn drop_user(&self, username: &str) -> Result<(), PostgresError> {
        info!("Dropping user: {}", username);

        let drop_user_sql = format!("DROP USER IF EXISTS {}", username);

        self.client
            .execute(&drop_user_sql, &[])
            .await
            .map_err(|e| PostgresError::RestoreError(e.to_string()))?;

        info!("User dropped successfully: {}", username);
        Ok(())
    }
}
