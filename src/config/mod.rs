mod keys;

// Re-export the common library's configuration
pub use common::config::{WardenConfig, C2AuthConfig, FeaturesConfig, load_config, update_config};
pub use keys::{generate_keypair, save_keypair, determine_key_directory};
