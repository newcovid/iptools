use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::utils::i18n::Language;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub language: Language,
    // 新增：扫描并发数
    pub scan_concurrency: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Language::Zh, // 默认中文，会根据系统环境调整
            scan_concurrency: 50,   // 默认 50 并发
        }
    }
}

impl Config {
    pub fn load() -> Self {
        if Path::new("config.json").exists() {
            if let Ok(content) = fs::read_to_string("config.json") {
                if let Ok(cfg) = serde_json::from_str(&content) {
                    return cfg;
                }
            }
        }

        // 如果文件不存在，保存默认配置
        let cfg = Self::default();
        cfg.save();
        cfg
    }

    pub fn save(&self) {
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write("config.json", content);
        }
    }
}
