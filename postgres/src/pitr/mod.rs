//! Point-in-Time Recovery (PITR) module for PostgreSQL.
//!
//! This module provides functionality for recovering a PostgreSQL database
//! to a specific point in time using base backups and WAL archives.
//!
//! # Overview
//!
//! PITR works by:
//! 1. Restoring a base backup (full or snapshot backup)
//! 2. Replaying WAL segments up to the target time
//! 3. Configuring PostgreSQL recovery parameters
//!
//! # Usage
//!
//! ```ignore
//! use postgres::pitr::{PitrPlanner, RecoveryPlan};
//!
//! // Create a planner
//! let planner = PitrPlanner::new(storage, backup_dir);
//!
//! // Compute a recovery plan
//! let plan = planner.plan_recovery(target_time).await?;
//!
//! // Execute the recovery
//! let executor = PitrExecutor::new(plan, target_dir);
//! executor.execute().await?;
//! ```

pub mod executor;
pub mod planner;
pub mod types;
pub mod wal;

pub use executor::PitrExecutor;
pub use planner::PitrPlanner;
pub use types::*;
pub use wal::*;
