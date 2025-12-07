//! Plugin registry for managing data source plugins.
//!
//! The registry is the central point for discovering and accessing
//! data source plugins at runtime.

use std::collections::HashMap;
use std::sync::Arc;

use super::error::{PluginError, PluginResult};
use super::traits::DataSource;
use super::types::PluginInfo;

/// Registry for managing data source plugins.
///
/// The registry maintains a collection of registered plugins and provides
/// methods for discovering, registering, and accessing them.
///
/// # Thread Safety
///
/// The registry uses `Arc<dyn DataSource>` for thread-safe sharing of plugins.
/// The registry itself should be wrapped in appropriate synchronization
/// primitives if accessed from multiple threads.
///
/// # Example
///
/// ```rust,ignore
/// use common::datasource::{PluginRegistry, DataSource};
///
/// // Create a new registry
/// let mut registry = PluginRegistry::new();
///
/// // Register a plugin
/// registry.register(Arc::new(PostgresDataSource::new()))?;
///
/// // List available plugins
/// for info in registry.list() {
///     println!("{}: {}", info.name, info.description);
/// }
///
/// // Get a specific plugin
/// if let Some(pg) = registry.get("postgresql") {
///     let status = pg.status(&config).await?;
/// }
/// ```
#[derive(Default)]
pub struct PluginRegistry {
    plugins: HashMap<String, Arc<dyn DataSource>>,
}

impl PluginRegistry {
    /// Create a new empty registry.
    ///
    /// Use `PluginRegistry::with_defaults()` to create a registry
    /// pre-populated with compile-time enabled plugins.
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Create a registry with default plugins based on compile-time features.
    ///
    /// This method registers all plugins that are enabled via Cargo features.
    /// Currently supported:
    /// - `postgresql` (default)
    ///
    /// Future plugins will be added as features are implemented.
    pub fn with_defaults() -> Self {
        // Plugins are registered by the main binary or daemon
        // based on compile-time features. This method provides
        // a hook for that registration.
        //
        // Example (in main.rs):
        // ```
        // let mut registry = PluginRegistry::with_defaults();
        // #[cfg(feature = "postgresql")]
        // registry.register(Arc::new(postgres::PostgresDataSource::new()))?;
        // ```

        Self::new()
    }

    /// Register a new plugin.
    ///
    /// # Arguments
    ///
    /// * `plugin` - The plugin to register
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Plugin registered successfully
    /// * `Err(PluginError::AlreadyRegistered)` - A plugin with this name exists
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let plugin = Arc::new(MyDataSource::new());
    /// registry.register(plugin)?;
    /// ```
    pub fn register(&mut self, plugin: Arc<dyn DataSource>) -> PluginResult<()> {
        let name = plugin.name().to_string();

        if self.plugins.contains_key(&name) {
            return Err(PluginError::already_registered(&name));
        }

        log::info!("Registering plugin: {} v{}", name, plugin.version());
        self.plugins.insert(name, plugin);
        Ok(())
    }

    /// Unregister a plugin by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to unregister
    ///
    /// # Returns
    ///
    /// * `Ok(Arc<dyn DataSource>)` - The removed plugin
    /// * `Err(PluginError::NotFound)` - No plugin with this name
    pub fn unregister(&mut self, name: &str) -> PluginResult<Arc<dyn DataSource>> {
        self.plugins
            .remove(name)
            .ok_or_else(|| PluginError::not_found(name))
    }

    /// Get a plugin by name.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to retrieve
    ///
    /// # Returns
    ///
    /// * `Some(Arc<dyn DataSource>)` - The plugin if found
    /// * `None` - No plugin with this name
    pub fn get(&self, name: &str) -> Option<Arc<dyn DataSource>> {
        self.plugins.get(name).cloned()
    }

    /// Check if a plugin is registered.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin to check
    pub fn contains(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// List all registered plugins.
    ///
    /// Returns information about each registered plugin including
    /// name, version, description, and capabilities.
    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins
            .values()
            .map(|plugin| PluginInfo {
                name: plugin.name().to_string(),
                version: plugin.version().to_string(),
                description: plugin.description().to_string(),
                capabilities: plugin.capabilities(),
            })
            .collect()
    }

    /// Get detailed information about a specific plugin.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the plugin
    ///
    /// # Returns
    ///
    /// * `Some(PluginInfo)` - Plugin information if found
    /// * `None` - No plugin with this name
    pub fn info(&self, name: &str) -> Option<PluginInfo> {
        self.plugins.get(name).map(|plugin| PluginInfo {
            name: plugin.name().to_string(),
            version: plugin.version().to_string(),
            description: plugin.description().to_string(),
            capabilities: plugin.capabilities(),
        })
    }

    /// Get the number of registered plugins.
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Get an iterator over plugin names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(|s| s.as_str())
    }

    /// Get an iterator over all plugins.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arc<dyn DataSource>)> {
        self.plugins.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl std::fmt::Debug for PluginRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRegistry")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// Global plugin registry singleton.
///
/// This provides a convenient way to access the plugin registry
/// from anywhere in the application. The registry is initialized
/// lazily on first access.
///
/// # Thread Safety
///
/// The global registry uses `std::sync::OnceLock` for thread-safe
/// lazy initialization. Access to the registry after initialization
/// is lock-free.
///
/// # Example
///
/// ```rust,ignore
/// use common::datasource::global_registry;
///
/// // Initialize the global registry (typically done at startup)
/// global_registry::init(|registry| {
///     registry.register(Arc::new(PostgresDataSource::new()))?;
///     Ok(())
/// })?;
///
/// // Access the registry from anywhere
/// let registry = global_registry::get();
/// for plugin in registry.list() {
///     println!("{}", plugin.name);
/// }
/// ```
pub mod global_registry {
    use super::*;
    use std::sync::OnceLock;

    static REGISTRY: OnceLock<PluginRegistry> = OnceLock::new();

    /// Initialize the global registry.
    ///
    /// This should be called once at application startup. The initializer
    /// function receives a mutable reference to the registry for plugin
    /// registration.
    ///
    /// # Panics
    ///
    /// Panics if called more than once.
    pub fn init<F>(initializer: F) -> PluginResult<()>
    where
        F: FnOnce(&mut PluginRegistry) -> PluginResult<()>,
    {
        let mut registry = PluginRegistry::new();
        initializer(&mut registry)?;

        REGISTRY
            .set(registry)
            .map_err(|_| PluginError::InitializationFailed("Registry already initialized".into()))
    }

    /// Get a reference to the global registry.
    ///
    /// # Panics
    ///
    /// Panics if the registry has not been initialized.
    pub fn get() -> &'static PluginRegistry {
        REGISTRY
            .get()
            .expect("Plugin registry not initialized. Call global_registry::init() first.")
    }

    /// Try to get a reference to the global registry.
    ///
    /// Returns `None` if the registry has not been initialized.
    pub fn try_get() -> Option<&'static PluginRegistry> {
        REGISTRY.get()
    }

    /// Check if the global registry has been initialized.
    pub fn is_initialized() -> bool {
        REGISTRY.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasource::*;

    /// Mock data source for testing
    struct MockDataSource {
        name: String,
        version: String,
    }

    impl MockDataSource {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                version: "1.0.0".to_string(),
            }
        }
    }

    #[async_trait::async_trait]
    impl DataSource for MockDataSource {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        fn description(&self) -> &str {
            "Mock data source for testing"
        }

        async fn discover(&self, _config: &DiscoverConfig) -> Result<DiscoverResult, DataSourceError> {
            Ok(DiscoverResult {
                connected: true,
                server_version: Some("1.0.0".to_string()),
                metadata: HashMap::new(),
                databases: vec!["test".to_string()],
                latency_ms: Some(10),
            })
        }

        async fn backup(&self, _config: &BackupConfig) -> Result<BackupResult, DataSourceError> {
            unimplemented!()
        }

        async fn list_backups(&self, _filter: &BackupFilter) -> Result<Vec<BackupMetadata>, DataSourceError> {
            Ok(vec![])
        }

        async fn restore(&self, _config: &RestoreConfig) -> Result<RestoreResult, DataSourceError> {
            unimplemented!()
        }

        fn supports_pitr(&self) -> bool {
            false
        }

        async fn pitr_restore(&self, _config: &PitrConfig) -> Result<RestoreResult, DataSourceError> {
            Err(DataSourceError::PitrNotSupported)
        }

        async fn status(&self, _config: &StatusConfig) -> Result<DataSourceStatus, DataSourceError> {
            Ok(DataSourceStatus::connected("1.0.0"))
        }

        fn capabilities(&self) -> DataSourceCapabilities {
            DataSourceCapabilities::default()
        }
    }

    #[test]
    fn test_registry_register() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockDataSource::new("test"));

        assert!(registry.register(plugin).is_ok());
        assert!(registry.contains("test"));
    }

    #[test]
    fn test_registry_duplicate_registration() {
        let mut registry = PluginRegistry::new();
        let plugin1 = Arc::new(MockDataSource::new("test"));
        let plugin2 = Arc::new(MockDataSource::new("test"));

        assert!(registry.register(plugin1).is_ok());
        assert!(matches!(
            registry.register(plugin2),
            Err(PluginError::AlreadyRegistered(_))
        ));
    }

    #[test]
    fn test_registry_get() {
        let mut registry = PluginRegistry::new();
        let plugin = Arc::new(MockDataSource::new("test"));
        registry.register(plugin).unwrap();

        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_registry_list() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(MockDataSource::new("plugin1"))).unwrap();
        registry.register(Arc::new(MockDataSource::new("plugin2"))).unwrap();

        let list = registry.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_registry_unregister() {
        let mut registry = PluginRegistry::new();
        registry.register(Arc::new(MockDataSource::new("test"))).unwrap();

        assert!(registry.unregister("test").is_ok());
        assert!(!registry.contains("test"));
        assert!(matches!(
            registry.unregister("test"),
            Err(PluginError::NotFound(_))
        ));
    }
}
