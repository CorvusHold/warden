//! Failure simulators for various system components.
//!
//! These simulators provide controlled ways to inject failures into
//! PostgreSQL, storage, network, and disk operations.

use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use log::{debug, info};
use thiserror::Error;

#[cfg(all(unix, feature = "chaos-testing"))]
use nix::sys::signal::{kill, Signal};
#[cfg(all(unix, feature = "chaos-testing"))]
use nix::unistd::Pid;

/// Errors from chaos simulators.
#[derive(Error, Debug)]
pub enum SimulatorError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Simulation error: {0}")]
    Simulation(String),
}

/// PostgreSQL failure simulator.
#[derive(Debug, Clone)]
pub struct PostgresSimulator {
    /// Data directory for the PostgreSQL instance.
    pub data_dir: Option<PathBuf>,
    /// Host for the PostgreSQL instance.
    pub host: String,
    /// Port for the PostgreSQL instance.
    pub port: u16,
}

impl Default for PostgresSimulator {
    fn default() -> Self {
        Self {
            data_dir: None,
            host: "localhost".to_string(),
            port: 5432,
        }
    }
}

impl PostgresSimulator {
    /// Create a new PostgreSQL simulator.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            data_dir: None,
            host: host.into(),
            port,
        }
    }

    /// Set the data directory.
    pub fn with_data_dir(mut self, data_dir: impl Into<PathBuf>) -> Self {
        self.data_dir = Some(data_dir.into());
        self
    }

    /// Simulate a PostgreSQL crash by sending SIGKILL to the postmaster.
    /// This is a destructive operation and should only be used in test environments.
    pub fn simulate_crash(&self) -> Result<(), SimulatorError> {
        info!(
            "[chaos] Simulating PostgreSQL crash at {}:{}",
            self.host, self.port
        );

        if let Some(ref data_dir) = self.data_dir {
            let pid_file = data_dir.join("postmaster.pid");
            if pid_file.exists() {
                let pid_content = std::fs::read_to_string(&pid_file)?;
                let pid: i32 = pid_content
                    .lines()
                    .next()
                    .and_then(|line| line.parse().ok())
                    .ok_or_else(|| {
                        SimulatorError::Simulation("Could not parse PID from postmaster.pid".into())
                    })?;

                // Send SIGKILL to the postmaster
                #[cfg(all(unix, feature = "chaos-testing"))]
                {
                    kill(Pid::from_raw(pid), Signal::SIGKILL)
                        .map_err(|e| SimulatorError::Simulation(format!("Failed to kill: {}", e)))?;
                }

                #[cfg(not(all(unix, feature = "chaos-testing")))]
                {
                    // Fallback: use kill command
                    let output = Command::new("kill")
                        .args(["-9", &pid.to_string()])
                        .output()?;
                    if !output.status.success() {
                        return Err(SimulatorError::Simulation(
                            format!("Failed to kill process {}", pid),
                        ));
                    }
                }

                info!("[chaos] Sent SIGKILL to PostgreSQL (PID: {})", pid);
                return Ok(());
            }
        }

        // Try using pg_ctl stop -m immediate as fallback
        if let Some(ref data_dir) = self.data_dir {
            let output = Command::new("pg_ctl")
                .args(["stop", "-D"])
                .arg(data_dir)
                .args(["-m", "immediate"])
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(SimulatorError::CommandFailed(format!(
                    "pg_ctl stop failed: {}",
                    stderr
                )));
            }
        }

        Ok(())
    }

    /// Simulate a connection failure by blocking the port (requires root/sudo).
    /// Returns a guard that unblocks the port when dropped.
    pub fn simulate_connection_block(&self) -> Result<ConnectionBlockGuard, SimulatorError> {
        info!(
            "[chaos] Simulating connection block on port {}",
            self.port
        );

        // Use iptables on Linux to block connections
        #[cfg(target_os = "linux")]
        {
            let output = Command::new("sudo")
                .args([
                    "iptables",
                    "-A",
                    "INPUT",
                    "-p",
                    "tcp",
                    "--dport",
                    &self.port.to_string(),
                    "-j",
                    "DROP",
                ])
                .output()?;

            if !output.status.success() {
                warn!(
                    "[chaos] iptables command failed (may need sudo): {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }

        Ok(ConnectionBlockGuard { port: self.port })
    }

    /// Check if PostgreSQL is accepting connections.
    pub fn is_accepting_connections(&self) -> bool {
        let output = Command::new("pg_isready")
            .args(["-h", &self.host, "-p", &self.port.to_string()])
            .output();

        match output {
            Ok(o) => o.status.success(),
            Err(_) => false,
        }
    }

    /// Wait for PostgreSQL to become unavailable.
    pub fn wait_for_unavailable(&self, timeout: Duration) -> Result<(), SimulatorError> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if !self.is_accepting_connections() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(SimulatorError::Timeout(
            "PostgreSQL did not become unavailable".into(),
        ))
    }

    /// Wait for PostgreSQL to become available.
    pub fn wait_for_available(&self, timeout: Duration) -> Result<(), SimulatorError> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            if self.is_accepting_connections() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(SimulatorError::Timeout(
            "PostgreSQL did not become available".into(),
        ))
    }
}

/// Guard that unblocks a port when dropped.
pub struct ConnectionBlockGuard {
    #[allow(dead_code)] // Used in Drop impl on Linux
    port: u16,
}

impl Drop for ConnectionBlockGuard {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("sudo")
                .args([
                    "iptables",
                    "-D",
                    "INPUT",
                    "-p",
                    "tcp",
                    "--dport",
                    &self.port.to_string(),
                    "-j",
                    "DROP",
                ])
                .output();
        }
    }
}

/// Storage (S3/MinIO) failure simulator.
#[derive(Debug, Clone)]
pub struct StorageSimulator {
    /// Endpoint URL for the storage service.
    pub endpoint: String,
    /// Bucket name.
    pub bucket: String,
    /// Simulated latency to add to requests.
    pub latency: Option<Duration>,
    /// Whether to simulate unavailability.
    pub unavailable: bool,
    /// Failure rate (0.0 - 1.0).
    pub failure_rate: f64,
}

impl Default for StorageSimulator {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9000".to_string(),
            bucket: "testbucket".to_string(),
            latency: None,
            unavailable: false,
            failure_rate: 0.0,
        }
    }
}

impl StorageSimulator {
    /// Create a new storage simulator.
    pub fn new(endpoint: impl Into<String>, bucket: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            ..Default::default()
        }
    }

    /// Set simulated latency.
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = Some(latency);
        self
    }

    /// Set unavailable state.
    pub fn set_unavailable(&mut self, unavailable: bool) {
        self.unavailable = unavailable;
    }

    /// Set failure rate.
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Check if a simulated failure should occur.
    pub fn should_fail(&self) -> bool {
        if self.unavailable {
            return true;
        }
        if self.failure_rate > 0.0 {
            let random: f64 = rand::random();
            return random < self.failure_rate;
        }
        false
    }

    /// Get the simulated latency.
    pub fn get_latency(&self) -> Option<Duration> {
        self.latency
    }

    /// Simulate a storage operation with potential failures.
    pub async fn simulate_operation<F, T, E>(&self, operation: F) -> Result<T, E>
    where
        F: std::future::Future<Output = Result<T, E>>,
        E: From<SimulatorError>,
    {
        // Add latency if configured
        if let Some(latency) = self.latency {
            debug!("[chaos] Adding {}ms latency to storage operation", latency.as_millis());
            tokio::time::sleep(latency).await;
        }

        // Check for simulated failure
        if self.should_fail() {
            return Err(SimulatorError::Simulation("Simulated storage failure".into()).into());
        }

        operation.await
    }
}

/// Network failure simulator.
#[derive(Debug, Clone)]
pub struct NetworkSimulator {
    /// Target host.
    pub host: String,
    /// Target port.
    pub port: u16,
    /// Simulated latency.
    pub latency: Option<Duration>,
    /// Packet loss rate (0.0 - 1.0).
    pub packet_loss: f64,
}

impl Default for NetworkSimulator {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            latency: None,
            packet_loss: 0.0,
        }
    }
}

impl NetworkSimulator {
    /// Create a new network simulator.
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            ..Default::default()
        }
    }

    /// Set simulated latency.
    pub fn with_latency(mut self, latency: Duration) -> Self {
        self.latency = Some(latency);
        self
    }

    /// Set packet loss rate.
    pub fn with_packet_loss(mut self, rate: f64) -> Self {
        self.packet_loss = rate.clamp(0.0, 1.0);
        self
    }

    /// Apply network conditions using tc (traffic control) on Linux.
    /// Requires root/sudo privileges.
    #[cfg(target_os = "linux")]
    pub fn apply_conditions(&self, interface: &str) -> Result<NetworkConditionGuard, SimulatorError> {
        info!(
            "[chaos] Applying network conditions: latency={:?}, packet_loss={}%",
            self.latency,
            self.packet_loss * 100.0
        );

        // Add qdisc
        let mut args = vec![
            "tc".to_string(),
            "qdisc".to_string(),
            "add".to_string(),
            "dev".to_string(),
            interface.to_string(),
            "root".to_string(),
            "netem".to_string(),
        ];

        if let Some(latency) = self.latency {
            args.push("delay".to_string());
            args.push(format!("{}ms", latency.as_millis()));
        }

        if self.packet_loss > 0.0 {
            args.push("loss".to_string());
            args.push(format!("{}%", self.packet_loss * 100.0));
        }

        let output = Command::new("sudo").args(&args).output()?;

        if !output.status.success() {
            warn!(
                "[chaos] tc command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(NetworkConditionGuard {
            interface: interface.to_string(),
        })
    }

    /// Check if the target is reachable.
    pub fn is_reachable(&self) -> bool {
        use std::net::TcpStream;
        TcpStream::connect(format!("{}:{}", self.host, self.port)).is_ok()
    }
}

/// Guard that removes network conditions when dropped.
#[cfg(target_os = "linux")]
pub struct NetworkConditionGuard {
    interface: String,
}

#[cfg(target_os = "linux")]
impl Drop for NetworkConditionGuard {
    fn drop(&mut self) {
        let _ = Command::new("sudo")
            .args(["tc", "qdisc", "del", "dev", &self.interface, "root"])
            .output();
    }
}

/// Disk failure simulator.
#[derive(Debug, Clone)]
pub struct DiskSimulator {
    /// Target directory for disk operations.
    pub target_dir: PathBuf,
    /// Simulated available space in bytes.
    pub available_space: Option<u64>,
    /// Whether to simulate permission errors.
    pub permission_denied: bool,
    /// Whether to simulate read-only filesystem.
    pub read_only: bool,
}

impl DiskSimulator {
    /// Create a new disk simulator.
    pub fn new(target_dir: impl Into<PathBuf>) -> Self {
        Self {
            target_dir: target_dir.into(),
            available_space: None,
            permission_denied: false,
            read_only: false,
        }
    }

    /// Set simulated available space.
    pub fn with_available_space(mut self, bytes: u64) -> Self {
        self.available_space = Some(bytes);
        self
    }

    /// Set permission denied simulation.
    pub fn with_permission_denied(mut self, denied: bool) -> Self {
        self.permission_denied = denied;
        self
    }

    /// Set read-only simulation.
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Check if a write operation should fail due to simulated conditions.
    pub fn should_fail_write(&self, bytes_to_write: u64) -> Option<io::Error> {
        if self.permission_denied {
            return Some(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Simulated permission denied",
            ));
        }

        if self.read_only {
            return Some(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Simulated read-only filesystem",
            ));
        }

        if let Some(available) = self.available_space {
            if bytes_to_write > available {
                return Some(io::Error::other(
                    "Simulated disk full (no space left on device)",
                ));
            }
        }

        None
    }

    /// Create a directory that simulates disk full conditions.
    /// Uses a small tmpfs mount on Linux.
    #[cfg(target_os = "linux")]
    pub fn create_small_tmpfs(&self, size_mb: u64) -> Result<TmpfsGuard, SimulatorError> {
        let mount_point = &self.target_dir;
        std::fs::create_dir_all(mount_point)?;

        let output = Command::new("sudo")
            .args([
                "mount",
                "-t",
                "tmpfs",
                "-o",
                &format!("size={}m", size_mb),
                "tmpfs",
                mount_point.to_str().unwrap(),
            ])
            .output()?;

        if !output.status.success() {
            return Err(SimulatorError::CommandFailed(format!(
                "Failed to mount tmpfs: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(TmpfsGuard {
            mount_point: mount_point.clone(),
        })
    }

    /// Get actual available space on the filesystem.
    pub fn get_actual_available_space(&self) -> Result<u64, SimulatorError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = std::fs::metadata(&self.target_dir)?;
            // This is a simplified check - in reality you'd use statvfs
            Ok(metadata.size())
        }

        #[cfg(not(unix))]
        {
            Ok(u64::MAX)
        }
    }

    /// Create a file that fills up the disk to a certain percentage.
    pub fn fill_disk_to_percentage(&self, percentage: f64) -> Result<PathBuf, SimulatorError> {
        let fill_file = self.target_dir.join(".chaos_fill_file");
        
        // Get available space
        let available = self.get_actual_available_space()?;
        let bytes_to_write = (available as f64 * percentage / 100.0) as u64;

        info!(
            "[chaos] Filling disk to {}% ({} bytes)",
            percentage, bytes_to_write
        );

        // Write zeros to fill the disk
        let mut file = std::fs::File::create(&fill_file)?;
        let chunk_size = 1024 * 1024; // 1MB chunks
        let zeros = vec![0u8; chunk_size];
        let mut written = 0u64;

        while written < bytes_to_write {
            let to_write = std::cmp::min(chunk_size as u64, bytes_to_write - written) as usize;
            std::io::Write::write_all(&mut file, &zeros[..to_write])?;
            written += to_write as u64;
        }

        Ok(fill_file)
    }
}

/// Guard that unmounts a tmpfs when dropped.
#[cfg(target_os = "linux")]
pub struct TmpfsGuard {
    mount_point: PathBuf,
}

#[cfg(target_os = "linux")]
impl Drop for TmpfsGuard {
    fn drop(&mut self) {
        let _ = Command::new("sudo")
            .args(["umount", self.mount_point.to_str().unwrap()])
            .output();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_postgres_simulator_default() {
        let sim = PostgresSimulator::default();
        assert_eq!(sim.host, "localhost");
        assert_eq!(sim.port, 5432);
    }

    #[test]
    fn test_storage_simulator_failure_rate() {
        let sim = StorageSimulator::default().with_failure_rate(1.0);
        assert!(sim.should_fail());

        let sim = StorageSimulator::default().with_failure_rate(0.0);
        assert!(!sim.should_fail());
    }

    #[test]
    fn test_disk_simulator_should_fail_write() {
        let sim = DiskSimulator::new("/tmp/test")
            .with_available_space(1000);

        // Should succeed for small write
        assert!(sim.should_fail_write(500).is_none());

        // Should fail for large write
        assert!(sim.should_fail_write(2000).is_some());
    }

    #[test]
    fn test_disk_simulator_permission_denied() {
        let sim = DiskSimulator::new("/tmp/test")
            .with_permission_denied(true);

        let err = sim.should_fail_write(100).unwrap();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }
}
