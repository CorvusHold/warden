//! Data Source Plugin Architecture
//!
//! This module defines the core traits and types for Warden's plugin system.
//! All data source plugins (PostgreSQL, MySQL, MongoDB, etc.) implement the
//! `DataSource` trait to provide a consistent interface for backup, restore,
//! and management operations.
//!
//! # Architecture
//!
//! The plugin architecture consists of:
//! - `DataSource` trait: Core interface all plugins must implement
//! - `PluginRegistry`: Central registry for managing plugins
//! - Configuration types: Standardized configs for operations
//! - Error types: Unified error handling across plugins
//!
//! # Example
//!
//! ```rust,ignore
//! use common::datasource::{DataSource, PluginRegistry};
//!
//! // Get the plugin registry
//! let registry = PluginRegistry::new();
//!
//! // List available plugins
//! for plugin in registry.list() {
//!     println!("{}: {}", plugin.name, plugin.description);
//! }
//!
//! // Get a specific plugin
//! if let Some(pg) = registry.get("postgresql") {
//!     let status = pg.status(&config).await?;
//!     println!("PostgreSQL status: {:?}", status);
//! }
//! ```

mod config;
mod error;
mod registry;
mod traits;
mod types;

pub use config::*;
pub use error::*;
pub use registry::*;
pub use traits::*;
pub use types::*;
