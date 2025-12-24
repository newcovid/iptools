use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 支持的语言类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    En,
    Zh,
}

impl Language {
    pub fn as_str(&self) -> &str {
        match self {
            Language::En => "en-US",
            Language::Zh => "zh-CN",
        }
    }

    /// 循环切换下一个语言
    pub fn next(&self) -> Self {
        match self {
            Language::En => Language::Zh,
            Language::Zh => Language::En,
        }
    }
}

pub struct I18n {
    current_lang: Language,
    dictionaries: HashMap<String, HashMap<String, String>>,
}

impl I18n {
    pub fn new(lang: Language) -> Self {
        let mut dictionaries = HashMap::new();

        // 采用嵌入静态资源的方式，确保二进制文件独立运行
        let en_json = include_str!("../../assets/locales/en-US.json");
        let en_map: HashMap<String, String> =
            serde_json::from_str(en_json).expect("Failed to parse en-US.json");
        dictionaries.insert("en-US".to_string(), en_map);

        let zh_json = include_str!("../../assets/locales/zh-CN.json");
        let zh_map: HashMap<String, String> =
            serde_json::from_str(zh_json).expect("Failed to parse zh-CN.json");
        dictionaries.insert("zh-CN".to_string(), zh_map);

        Self {
            current_lang: lang,
            dictionaries,
        }
    }

    pub fn set_lang(&mut self, lang: Language) {
        self.current_lang = lang;
    }

    pub fn get_lang(&self) -> Language {
        self.current_lang
    }

    /// 执行翻译。如果对应语言缺失 Key，则尝试回退到英文，否则返回 MISSING 标记
    pub fn t(&self, key: &str) -> String {
        let lang_key = self.current_lang.as_str();

        // 尝试当前语言
        if let Some(dict) = self.dictionaries.get(lang_key) {
            if let Some(val) = dict.get(key) {
                return val.clone();
            }
        }

        // 回退到英文
        if self.current_lang != Language::En {
            if let Some(dict) = self.dictionaries.get("en-US") {
                if let Some(val) = dict.get(key) {
                    return val.clone();
                }
            }
        }

        format!("MISSING:{}", key)
    }
}
