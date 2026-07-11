use std::collections::HashMap;

pub use iptools_core::Language;

/// Infer the first-run default without leaking platform/environment access into core.
pub fn detect_system_language() -> Language {
    let tag = system_locale_tag().to_lowercase();
    if tag.starts_with("zh") {
        Language::Zh
    } else {
        Language::En
    }
}

/// 返回形如 "zh-CN" / "en-US" 的系统区域标签（失败时返回空串）。
#[cfg(target_os = "windows")]
fn system_locale_tag() -> String {
    use windows::Win32::Globalization::GetUserDefaultLocaleName;
    // LOCALE_NAME_MAX_LENGTH == 85
    let mut buf = [0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(&mut buf) };
    if len <= 0 {
        return String::new();
    }
    // 返回值含结尾的 NUL，截掉它
    let end = (len as usize).saturating_sub(1);
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(not(target_os = "windows"))]
fn system_locale_tag() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
}

pub struct I18n {
    current_lang: Language,
    dictionaries: HashMap<String, HashMap<String, String>>,
}

impl I18n {
    pub fn new(lang: Language) -> Self {
        let mut dictionaries = HashMap::new();

        // 采用嵌入静态资源的方式，确保二进制文件独立运行
        let en_json = include_str!("../../../../assets/locales/en-US.json");
        let en_map: HashMap<String, String> =
            serde_json::from_str(en_json).expect("Failed to parse en-US.json");
        dictionaries.insert("en-US".to_string(), en_map);

        let zh_json = include_str!("../../../../assets/locales/zh-CN.json");
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
        if let Some(dict) = self.dictionaries.get(lang_key)
            && let Some(val) = dict.get(key)
        {
            return val.clone();
        }

        // 回退到英文
        if self.current_lang != Language::En
            && let Some(dict) = self.dictionaries.get("en-US")
            && let Some(val) = dict.get(key)
        {
            return val.clone();
        }

        format!("MISSING:{}", key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> HashMap<String, String> {
        serde_json::from_str(json).expect("locale JSON must parse")
    }

    /// 锁定不变量：两份语言包的 key 集合必须完全一致。
    /// 任何新增 UI 文案若只加了一种语言，此测试会立即失败，
    /// 从根上杜绝 `MISSING:<key>` 与硬编码漏翻。
    #[test]
    fn locale_keys_are_in_sync() {
        let en = parse(include_str!("../../../../assets/locales/en-US.json"));
        let zh = parse(include_str!("../../../../assets/locales/zh-CN.json"));

        let only_en: Vec<&String> = en.keys().filter(|k| !zh.contains_key(*k)).collect();
        let only_zh: Vec<&String> = zh.keys().filter(|k| !en.contains_key(*k)).collect();

        assert!(
            only_en.is_empty() && only_zh.is_empty(),
            "locale key mismatch\n  only in en-US: {:?}\n  only in zh-CN: {:?}",
            only_en,
            only_zh
        );
    }

    #[test]
    fn falls_back_to_english_then_marks_missing() {
        let i18n = I18n::new(Language::Zh);
        assert_eq!(i18n.t("nonexistent_key_xyz"), "MISSING:nonexistent_key_xyz");
        assert!(!i18n.t("tab_dashboard").starts_with("MISSING"));
    }
}
