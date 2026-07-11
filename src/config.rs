use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::keymap::{KeyMap, PersistedKeymap};
use crate::session::SessionState;
use crate::utils::i18n::Language;

/// 默认配置文件名（相对当前工作目录）。
const DEFAULT_CONFIG_PATH: &str = "config.json";

/// 公网信息抓取配置（顶层设置，非 session）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PublicIpConfig {
    /// 按顺序尝试的端点；首个成功即用。
    pub endpoints: Vec<Endpoint>,
    /// 是否走系统/环境代理（默认 true）。false = 强制直连（power-user，无 UI）。
    pub use_system_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub url: String,
    /// "ipsb" | "ipinfo" | "plaintext"
    pub kind: String,
}

impl Default for PublicIpConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![
                Endpoint {
                    url: "https://api.ip.sb/geoip".to_string(),
                    kind: "ipsb".to_string(),
                },
                Endpoint {
                    url: "https://ipinfo.io/json".to_string(),
                    kind: "ipinfo".to_string(),
                },
            ],
            use_system_proxy: true,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub language: Language,
    /// 扫描并发数
    pub scan_concurrency: usize,

    /// 用户自定义快捷键（动作名 -> 组合键列表）。缺省时回退内置默认绑定。
    #[serde(default)]
    pub keybindings: PersistedKeymap,

    /// 各页面/诊断工具上次使用的输入参数（IP/端口/间隔/超时/载荷/CIDR、链路质量按网卡）。
    /// 重启回灌，避免每次重置。缺省（旧配置无此段）回退到各项默认值。
    #[serde(default)]
    pub session: SessionState,

    /// 公网信息抓取端点与代理策略。缺省（旧配置无此段）回退默认（ip.sb→ipinfo，尊重代理）。
    #[serde(default)]
    pub public_ip: PublicIpConfig,

    /// 配置文件实际路径。不参与序列化，由 `load` 注入，`save` 时写回同一路径。
    #[serde(skip)]
    path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::En, // 仅作结构默认；首次创建时 load() 会用系统语言覆盖
            scan_concurrency: 50,
            keybindings: KeyMap::default().to_persisted(),
            session: SessionState::default(),
            public_ip: PublicIpConfig::default(),
            path: PathBuf::from(DEFAULT_CONFIG_PATH),
        }
    }
}

impl Config {
    /// 从指定路径加载配置；`None` 时使用默认路径 `config.json`。
    /// 文件不存在或解析失败时，生成一份以系统语言为默认值的配置并落盘。
    pub fn load(path: Option<&str>) -> Self {
        let path = PathBuf::from(path.unwrap_or(DEFAULT_CONFIG_PATH));

        if Path::new(&path).exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = serde_json::from_str::<Config>(&content) {
                    cfg.path = path;
                    // 旧配置可能缺少 keybindings 段：补齐默认值并落盘，方便用户发现可改项
                    if cfg.keybindings.is_empty() {
                        cfg.keybindings = KeyMap::default().to_persisted();
                        cfg.save();
                    }
                    return cfg;
                }
            }
        }

        // 文件不存在/损坏：依系统语言生成默认配置并保存
        let cfg = Self {
            language: Language::detect_system(),
            scan_concurrency: 50,
            keybindings: KeyMap::default().to_persisted(),
            session: SessionState::default(),
            public_ip: PublicIpConfig::default(),
            path,
        };
        cfg.save();
        cfg
    }

    /// 由当前持久化绑定构建运行时键位映射表。
    pub fn keymap(&self) -> KeyMap {
        KeyMap::from_persisted(&self.keybindings)
    }

    pub fn save(&self) {
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&self.path, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_ip_defaults() {
        let c = PublicIpConfig::default();
        assert_eq!(c.endpoints.len(), 2);
        assert_eq!(c.endpoints[0].kind, "ipsb");
        assert!(c.endpoints[0].url.starts_with("https://"));
        assert!(c.use_system_proxy);
    }

    #[test]
    fn old_config_without_public_ip_falls_back() {
        // 模拟旧 config.json（无 public_ip 段）：public_ip 回退默认。
        let json = r#"{"language":"En","scan_concurrency":50}"#;
        let c: Config = serde_json::from_str(json).unwrap();
        assert_eq!(c.public_ip.endpoints.len(), 2);
        assert!(c.public_ip.use_system_proxy);
    }

    #[test]
    fn example_config_is_valid() {
        serde_json::from_str::<Config>(include_str!("../config.example.json"))
            .expect("config.example.json must remain a valid Config");
    }
}
