use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::keymap::{KeyMap, PersistedKeymap};
use crate::utils::i18n::Language;

/// 默认配置文件名（相对当前工作目录）。
const DEFAULT_CONFIG_PATH: &str = "config.json";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub language: Language,
    /// 扫描并发数
    pub scan_concurrency: usize,

    /// 用户自定义快捷键（动作名 -> 组合键列表）。缺省时回退内置默认绑定。
    #[serde(default)]
    pub keybindings: PersistedKeymap,

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
