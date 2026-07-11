//! Native configuration storage.
//!
//! The serializable schema lives in `iptools-core`; this module owns only the
//! filesystem path, system-language bootstrap and atomic persistence.

use std::{
    fs,
    io::Write,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
};

pub use iptools_core::{ConfigData, Endpoint, PublicIpConfig};

use crate::{keymap::KeyMap, utils::i18n::detect_system_language};

const DEFAULT_CONFIG_PATH: &str = "config.json";

#[derive(Debug, Clone)]
pub struct FsConfigStore {
    path: PathBuf,
}

impl FsConfigStore {
    pub fn new(path: Option<&str>) -> Self {
        Self {
            path: PathBuf::from(path.unwrap_or(DEFAULT_CONFIG_PATH)),
        }
    }

    pub fn load(&self) -> Option<ConfigData> {
        let content = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str(&content).ok()
    }

    pub fn save(&self, data: &ConfigData) -> std::io::Result<()> {
        let content = serde_json::to_vec_pretty(data).map_err(std::io::Error::other)?;
        write_atomic(&self.path, &content)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Compatibility facade for the current native `App` during vertical migration.
/// Field access dereferences to the shared `ConfigData`; no serializable state is
/// defined in this crate anymore.
pub struct Config {
    data: ConfigData,
    store: FsConfigStore,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data: ConfigData::default(),
            store: FsConfigStore::new(None),
        }
    }
}

impl Deref for Config {
    type Target = ConfigData;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for Config {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Config {
    pub fn load(path: Option<&str>) -> Self {
        let store = FsConfigStore::new(path);
        if let Some(mut data) = store.load() {
            if data.keybindings.is_empty() {
                data.keybindings = KeyMap::default().to_persisted();
                if let Err(error) = store.save(&data) {
                    tracing::warn!(path = %store.path().display(), %error, "failed to persist default keybindings");
                }
            }
            return Self { data, store };
        }

        let mut data = ConfigData {
            language: detect_system_language(),
            ..ConfigData::default()
        };
        data.keybindings = KeyMap::default().to_persisted();
        if let Err(error) = store.save(&data) {
            tracing::warn!(path = %store.path().display(), %error, "failed to create configuration");
        }
        Self { data, store }
    }

    pub fn keymap(&self) -> KeyMap {
        KeyMap::from_persisted(&self.keybindings)
    }

    pub fn save(&self) {
        if let Err(error) = self.store.save(&self.data) {
            tracing::warn!(path = %self.store.path().display(), %error, "failed to persist configuration");
        }
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut file = atomic_write_file::AtomicWriteFile::options().open(path)?;
    file.write_all(contents)?;
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_without_public_ip_falls_back() {
        let json = r#"{"language":"En","scan_concurrency":50}"#;
        let config: ConfigData = serde_json::from_str(json).unwrap();
        assert_eq!(config.public_ip.endpoints.len(), 2);
        assert!(config.public_ip.use_system_proxy);
    }

    #[test]
    fn example_config_is_valid() {
        serde_json::from_str::<ConfigData>(include_str!("../../../config.example.json"))
            .expect("config.example.json must remain valid");
    }

    #[test]
    fn filesystem_store_roundtrips_shared_data_atomically() {
        let path = std::env::temp_dir().join(format!(
            "iptools-config-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = FsConfigStore { path: path.clone() };
        let expected = ConfigData {
            language: iptools_core::Language::Zh,
            scan_concurrency: 120,
            ..ConfigData::default()
        };

        store.save(&expected).unwrap();
        assert_eq!(store.load(), Some(expected));
        std::fs::remove_file(path).unwrap();
    }
}
