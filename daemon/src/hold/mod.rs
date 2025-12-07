//! HOLD/C2 Integration Module
//!
//! This module provides optional integration with a HOLD control plane.
//! When enabled, it allows HOLD to:
//! - Observe agent status and metrics
//! - Trigger existing operations remotely (backup, status queries)
//!
//! The integration is designed to be:
//! - Optional: Disabled by default, controlled via config
//! - Non-blocking: HOLD unavailability never blocks local operations
//! - Minimal: Thin layer over existing CLI/internal APIs

mod client;
mod commands;
mod events;
mod integration;

pub use client::HoldClient;
pub use commands::{HoldCommand, HoldCommandHandler};
pub use events::{HoldEvent, HoldEventPublisher};
pub use integration::HoldIntegration;
