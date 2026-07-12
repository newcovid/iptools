//! Serializable configuration data shared by every runtime.
//!
//! Paths, platform language detection and file I/O intentionally stay in the
//! native crate. These types preserve the published JSON field names and Serde
//! defaults so existing configuration files remain readable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Language;

pub type PersistedKeymap = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    pub url: String,
    /// `ipsb` | `ipinfo` | `plaintext`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PublicIpConfig {
    pub endpoints: Vec<Endpoint>,
    pub use_system_proxy: bool,
}

impl Default for PublicIpConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![
                Endpoint {
                    url: "https://api.ip.sb/geoip".into(),
                    kind: "ipsb".into(),
                },
                Endpoint {
                    url: "https://ipinfo.io/json".into(),
                    kind: "ipinfo".into(),
                },
            ],
            use_system_proxy: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigData {
    pub language: Language,
    pub theme: crate::ThemeId,
    pub scan_concurrency: usize,
    pub keybindings: PersistedKeymap,
    pub session: SessionState,
    pub public_ip: PublicIpConfig,
}

impl Default for ConfigData {
    fn default() -> Self {
        Self {
            language: Language::En,
            theme: crate::ThemeId::Classic,
            scan_concurrency: 50,
            keybindings: PersistedKeymap::new(),
            session: SessionState::default(),
            public_ip: PublicIpConfig::default(),
        }
    }
}

impl ConfigData {
    /// Apply a persistence-only effect to the shared configuration schema.
    ///
    /// Native and Web stores call this same pure function, then persist the
    /// resulting value using their platform-specific storage backend.
    pub fn apply_persistence_effect(&mut self, effect: &crate::Effect) -> bool {
        match effect {
            crate::Effect::PersistPreferences(preferences) => {
                self.language = preferences.language;
                self.theme = preferences.theme;
                self.scan_concurrency = preferences.scan_concurrency;
            }
            crate::Effect::PersistSession(update) => match update {
                crate::SessionUpdate::Scanner(value) => self.session.scanner = value.clone(),
                crate::SessionUpdate::CidrHistory(value) => {
                    self.session.history.cidrs = value.clone();
                }
                crate::SessionUpdate::Ping(value) => self.session.ping = value.clone(),
                crate::SessionUpdate::Trace(value) => self.session.trace = value.clone(),
                crate::SessionUpdate::PortScan(value) => {
                    self.session.port_scan = value.clone();
                }
                crate::SessionUpdate::LanSpeed(value) => {
                    self.session.lan_speed = value.clone();
                }
                crate::SessionUpdate::LinkQuality(value) => {
                    self.session.link_quality = value.clone();
                }
                crate::SessionUpdate::TargetHistory(value) => {
                    self.session.history.targets = value.clone();
                }
                crate::SessionUpdate::Ui(value) => self.session.ui = value.clone(),
                crate::SessionUpdate::Reset(ui) => {
                    self.session = SessionState {
                        ui: ui.clone(),
                        ..SessionState::default()
                    };
                }
            },
            crate::Effect::PersistAdapterEdit {
                guid,
                params,
                history,
            } => {
                self.session
                    .adapter_edit
                    .adapters
                    .insert(guid.clone(), params.clone());
                self.session.history.adapter = history.clone();
            }
            _ => return false,
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionState {
    pub scanner: ScannerPersist,
    pub ping: PingPersist,
    pub port_scan: PortScanPersist,
    pub trace: TracePersist,
    pub lan_speed: LanSpeedPersist,
    pub link_quality: LinkQualityPersist,
    pub adapter_edit: AdapterEditPersist,
    pub ui: UiPersist,
    pub history: HistoryPersist,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiPersist {
    pub last_tab: u8,
    pub last_diag_tool: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HistoryPersist {
    pub targets: Vec<String>,
    pub cidrs: Vec<String>,
    pub adapter: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScannerPersist {
    pub cidr: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PingPersist {
    pub target: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub packet_size: u64,
}

impl Default for PingPersist {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".into(),
            interval_ms: 1_000,
            timeout_ms: 2_000,
            packet_size: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortScanPersist {
    pub target: String,
    pub start_port: String,
    pub end_port: String,
    pub timeout_ms: String,
}

impl Default for PortScanPersist {
    fn default() -> Self {
        Self {
            target: "127.0.0.1".into(),
            start_port: "1".into(),
            end_port: "1024".into(),
            timeout_ms: "300".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TracePersist {
    pub target: String,
    pub max_hops: String,
    pub timeout_ms: String,
}

impl Default for TracePersist {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".into(),
            max_hops: "30".into(),
            timeout_ms: "1000".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanSpeedPersist {
    pub mode: String,
    pub peer: String,
    pub port: String,
    pub proto: String,
    pub direction: String,
    pub duration: String,
    pub streams: String,
    pub payload: String,
    pub rate: String,
}

impl Default for LanSpeedPersist {
    fn default() -> Self {
        Self {
            mode: "server".into(),
            peer: String::new(),
            port: "50505".into(),
            proto: "tcp".into(),
            direction: "up".into(),
            duration: "10".into(),
            streams: "1".into(),
            payload: "65536".into(),
            rate: "0".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkParams {
    pub target: String,
    pub count: String,
    pub interval_ms: String,
    pub timeout_ms: String,
    pub packet_size: String,
}

impl Default for LinkParams {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".into(),
            count: "20".into(),
            interval_ms: "200".into(),
            timeout_ms: "1000".into(),
            packet_size: "32".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LinkQualityPersist {
    pub adapters: BTreeMap<String, LinkParams>,
    pub selected: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdapterEditParams {
    pub use_dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns1: String,
    pub dns2: String,
}

impl Default for AdapterEditParams {
    fn default() -> Self {
        Self {
            use_dhcp: true,
            ip: String::new(),
            mask: String::new(),
            gateway: String::new(),
            dns1: String::new(),
            dns2: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AdapterEditPersist {
    pub adapters: BTreeMap<String, AdapterEditParams>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_legacy_config_keeps_defaults() {
        let config: ConfigData =
            serde_json::from_str(r#"{"language":"En","scan_concurrency":50}"#).unwrap();
        assert_eq!(config.language, Language::En);
        assert_eq!(config.theme, crate::ThemeId::Classic);
        assert_eq!(config.session.ping, PingPersist::default());
        assert_eq!(config.public_ip, PublicIpConfig::default());
    }

    #[test]
    fn partial_session_fields_keep_other_defaults() {
        let session: SessionState =
            serde_json::from_str(r#"{"ping":{"target":"1.1.1.1"}}"#).unwrap();
        assert_eq!(session.ping.target, "1.1.1.1");
        assert_eq!(session.ping.interval_ms, 1_000);
        assert_eq!(session.port_scan.end_port, "1024");
        assert_eq!(session.lan_speed.port, "50505");
    }

    #[test]
    fn empty_session_uses_all_tool_defaults() {
        let session: SessionState = serde_json::from_str("{}").unwrap();
        assert_eq!(session, SessionState::default());
        assert_eq!(session.ping.target, "8.8.8.8");
        assert_eq!(session.port_scan.end_port, "1024");
        assert_eq!(session.lan_speed.mode, "server");
    }

    #[test]
    fn history_defaults_and_partial_values_are_compatible() {
        let session: SessionState =
            serde_json::from_str(r#"{"history":{"targets":["8.8.8.8","1.1.1.1"]}}"#).unwrap();
        assert_eq!(session.history.targets, ["8.8.8.8", "1.1.1.1"]);
        assert!(session.history.cidrs.is_empty());
        assert!(session.history.adapter.is_empty());
    }

    #[test]
    fn old_lan_speed_fields_keep_new_defaults() {
        let persist: LanSpeedPersist =
            serde_json::from_str(r#"{"mode":"client","peer":"10.0.0.2","port":"5000"}"#).unwrap();
        assert_eq!(persist.mode, "client");
        assert_eq!(persist.proto, "tcp");
        assert_eq!(persist.direction, "up");
        assert_eq!(persist.duration, "10");
    }

    #[test]
    fn per_adapter_link_quality_roundtrips_stably() {
        let mut session = SessionState::default();
        session.link_quality.adapters.insert(
            "{GUID-WIFI}".into(),
            LinkParams {
                target: "192.168.1.1".into(),
                ..LinkParams::default()
            },
        );
        session.link_quality.adapters.insert(
            "{GUID-ETH}".into(),
            LinkParams {
                target: "10.0.0.1".into(),
                ..LinkParams::default()
            },
        );
        session.link_quality.selected = Some("{GUID-WIFI}".into());

        let json = serde_json::to_string(&session).unwrap();
        let restored: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, session);
        assert_eq!(
            restored.link_quality.adapters["{GUID-ETH}"].target,
            "10.0.0.1"
        );
    }

    #[test]
    fn session_roundtrips() {
        let session = SessionState::default();
        let json = serde_json::to_string(&session).unwrap();
        assert_eq!(
            serde_json::from_str::<SessionState>(&json).unwrap(),
            session
        );
    }

    #[test]
    fn theme_is_optional_for_legacy_configs_and_roundtrips_as_a_stable_name() {
        let config: ConfigData = serde_json::from_str(
            r#"{"language":"Zh","theme":"catppuccin-mocha","scan_concurrency":50}"#,
        )
        .unwrap();
        assert_eq!(config.theme, crate::ThemeId::CatppuccinMocha);
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"theme\":\"catppuccin-mocha\""));
    }

    #[test]
    fn persistence_effects_update_the_same_schema_for_every_store() {
        let mut config = ConfigData::default();
        assert!(
            config.apply_persistence_effect(&crate::Effect::PersistPreferences(
                crate::Preferences {
                    language: Language::Zh,
                    theme: crate::ThemeId::Nord,
                    scan_concurrency: 80,
                },
            ))
        );
        assert_eq!(config.language, Language::Zh);
        assert_eq!(config.theme, crate::ThemeId::Nord);
        assert_eq!(config.scan_concurrency, 80);

        assert!(
            config.apply_persistence_effect(&crate::Effect::PersistSession(
                crate::SessionUpdate::TargetHistory(vec!["1.1.1.1".into()]),
            ))
        );
        assert_eq!(config.session.history.targets, ["1.1.1.1"]);

        assert!(
            !config.apply_persistence_effect(&crate::Effect::RefreshTraffic {
                job: crate::JobId {
                    tool: crate::ToolKind::Traffic,
                    generation: 1,
                },
            })
        );
    }
}
