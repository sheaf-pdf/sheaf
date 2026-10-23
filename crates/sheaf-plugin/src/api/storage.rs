//! The `storage` interface: the plugin's own store, its settings, and its
//! preferences as the call started with them.

use super::api;
use crate::runtime::HostState;

impl api::storage::Host for HostState {
    fn get(&mut self, key: String) -> Option<String> {
        self.storage.get(&key)
    }

    fn set(&mut self, key: String, value: String) -> Result<(), String> {
        self.storage.set(key, value)
    }

    fn delete(&mut self, key: String) {
        if let Err(e) = self.storage.delete(&key) {
            log::warn!(target: "plugin", "[{}] {e}", self.plugin);
        }
    }

    fn keys(&mut self) -> Vec<String> {
        self.storage.keys()
    }

    fn settings(&mut self) -> String {
        self.storage.settings()
    }

    fn preferences(&mut self) -> String {
        self.call
            .as_ref()
            .map(|c| c.preferences.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "{}".into())
    }
}
