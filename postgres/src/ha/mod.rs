//! High Availability (HA) orchestration module for PostgreSQL clusters.
//!
//! This module provides orchestration commands for managing PostgreSQL HA clusters:
//! - **Switchover**: Planned role transfer from primary to replica.
//! - **Failover**: Emergency promotion of a replica when primary is down.
//! - **Clone Node**: Create a new replica from backup/PITR.
//!
//! All commands work offline (no HOLD/C2 dependency) and leverage existing
//! backup/restore/PITR primitives.

pub mod checks;
pub mod clone;
pub mod failover;
pub mod switchover;
pub mod types;

pub use checks::{NodeHealthCheck, NodeHealthStatus, ReplicationStatus};
pub use clone::{CloneNodeOptions, CloneNodeOrchestrator};
pub use failover::{FailoverOptions, FailoverOrchestrator};
pub use switchover::{SwitchoverOptions, SwitchoverOrchestrator};
pub use types::{format_plan, format_result, HaError, HaPlan, HaPlanStep, HaResult, HaStepStatus};
