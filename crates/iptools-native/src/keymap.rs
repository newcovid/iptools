//! 可由用户自定义的快捷键映射。
//!
//! 设计：将物理按键与"语义动作"(`Action`) 解耦。各模块只关心动作，
//! 不再硬编码 `KeyCode`。用户可在 `config.json` 的 `keybindings` 段覆盖默认绑定，
//! 形如：
//! ```json
//! "keybindings": {
//!   "quit":  ["Ctrl+c", "Ctrl+q"],
//!   "down":  ["Down", "j"],
//!   "next_tab": ["Tab"]
//! }
//! ```
//! 未覆盖的动作沿用内置默认值；无法解析的组合键被忽略（不会让程序崩溃）。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};

/// 持久化形式：动作名 -> 组合键字符串列表。直接写入 / 读出 config.json。
pub type PersistedKeymap = BTreeMap<String, Vec<String>>;

/// 全部语义动作。新增动作时记得同步 `name`/`from_name`/`ALL`/`default_combos`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    // 全局
    Quit,
    ToggleLanguage,
    NextTab,
    PrevTab,
    // 通用导航 / 控制
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Refresh,
    History,
    Edit,
    Toggle,
    Help,
}

impl Action {
    /// 稳定的持久化名称（也用于 i18n 描述键的后缀）。
    pub fn name(self) -> &'static str {
        match self {
            Action::Quit => "quit",
            Action::ToggleLanguage => "toggle_language",
            Action::NextTab => "next_tab",
            Action::PrevTab => "prev_tab",
            Action::Up => "up",
            Action::Down => "down",
            Action::Left => "left",
            Action::Right => "right",
            Action::Confirm => "confirm",
            Action::Back => "back",
            Action::Refresh => "refresh",
            Action::History => "history",
            Action::Edit => "edit",
            Action::Toggle => "toggle",
            Action::Help => "help",
        }
    }

    pub fn from_name(s: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.name() == s)
    }

    /// 解析优先级顺序（全局动作在前）。`action_for` 按此顺序匹配。
    pub const ALL: [Action; 15] = [
        Action::Quit,
        Action::ToggleLanguage,
        Action::NextTab,
        Action::PrevTab,
        Action::Up,
        Action::Down,
        Action::Left,
        Action::Right,
        Action::Confirm,
        Action::Back,
        Action::Refresh,
        Action::History,
        Action::Edit,
        Action::Toggle,
        Action::Help,
    ];

    fn default_combos(self) -> Vec<KeyCombo> {
        use KeyCode::*;
        let c = |code, mods| KeyCombo { code, mods };
        let plain = |code| KeyCombo {
            code,
            mods: KeyModifiers::NONE,
        };
        match self {
            Action::Quit => vec![
                c(Char('c'), KeyModifiers::CONTROL),
                c(Char('q'), KeyModifiers::CONTROL),
            ],
            Action::ToggleLanguage => vec![c(Char('l'), KeyModifiers::CONTROL)],
            Action::NextTab => vec![plain(Tab)],
            // Shift+Tab 在不同终端可能上报为：BackTab(无修饰)、BackTab+Shift、
            // 或 Tab+Shift。三种都绑上，避免「上一标签页」在 Windows 终端失效。
            Action::PrevTab => vec![
                plain(BackTab),
                c(BackTab, KeyModifiers::SHIFT),
                c(Tab, KeyModifiers::SHIFT),
            ],
            Action::Up => vec![plain(Up), plain(Char('w'))],
            Action::Down => vec![plain(Down), plain(Char('s'))],
            Action::Left => vec![plain(Left), plain(Char('a'))],
            Action::Right => vec![plain(Right), plain(Char('d'))],
            Action::Confirm => vec![plain(Enter)],
            Action::Back => vec![plain(Esc)],
            Action::Refresh => vec![plain(Char('r'))],
            Action::History => vec![c(Char('r'), KeyModifiers::CONTROL)],
            Action::Edit => vec![plain(Char('e'))],
            Action::Toggle => vec![plain(Char(' '))],
            Action::Help => vec![plain(F(1))],
        }
    }
}

/// 单个组合键 = 主键 + 修饰键集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyCombo {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

/// 仅保留 Ctrl/Alt/Shift，忽略终端可能附带的其它位（如 SUPER/KEYPAD）。
fn relevant_mods(m: KeyModifiers) -> KeyModifiers {
    m & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT)
}

/// 把 Shift+Tab 的多种编码统一成一个规范形式：`BackTab`（去掉 Shift）。
///
/// Shift+Tab 在不同终端 / 持久化往返后可能呈现为三种形态——`BackTab`(无修饰)、
/// `BackTab`+Shift、或 `Tab`+Shift。尤其 `to_label()` 把 `BackTab` 写成 "Shift+Tab"，
/// 重新解析时会变回 `Tab`+Shift，于是存盘再读后 `BackTab` 绑定被悄悄丢失，导致
/// 「上一标签页」在 Windows（crossterm 上报 `BackTab`）失效。规范化后三种形态等价，
/// 无论怎样存取都能匹配。
fn normalize(code: KeyCode, mods: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let mods = relevant_mods(mods);
    match code {
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::BackTab, mods & !KeyModifiers::SHIFT)
        }
        KeyCode::BackTab => (KeyCode::BackTab, mods & !KeyModifiers::SHIFT),
        _ => (code, mods),
    }
}

impl KeyCombo {
    pub fn matches(&self, ev: &KeyEvent) -> bool {
        normalize(self.code, self.mods) == normalize(ev.code, ev.modifiers)
    }

    /// 解析 "Ctrl+c" / "Shift+Tab" / "Up" / "Space" / "j" 等。大小写不敏感的修饰键。
    pub fn parse(s: &str) -> Option<KeyCombo> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let parts: Vec<&str> = s.split('+').map(|p| p.trim()).collect();
        let (key_part, mod_parts) = parts.split_last()?;

        let mut mods = KeyModifiers::NONE;
        for m in mod_parts {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
                "alt" => mods |= KeyModifiers::ALT,
                "shift" => mods |= KeyModifiers::SHIFT,
                _ => return None,
            }
        }

        let code = parse_keycode(key_part)?;
        Some(KeyCombo { code, mods })
    }

    pub fn to_label(self) -> String {
        self.to_label_i18n("en-US")
    }

    /// 返回本地化的组合键标签。zh-CN 下 Enter→回车。
    pub fn to_label_i18n(self, locale: &str) -> String {
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push_str("Ctrl+");
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push_str("Alt+");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            out.push_str("Shift+");
        }
        out.push_str(&keycode_label_i18n(self.code, locale));
        out
    }
}

fn parse_keycode(s: &str) -> Option<KeyCode> {
    use KeyCode::*;
    let lower = s.to_ascii_lowercase();
    let code = match lower.as_str() {
        "enter" | "return" => Enter,
        "esc" | "escape" => Esc,
        "tab" => Tab,
        "backtab" | "shift+tab" => BackTab,
        "space" => Char(' '),
        "up" => Up,
        "down" => Down,
        "left" => Left,
        "right" => Right,
        "backspace" => Backspace,
        "delete" | "del" => Delete,
        "home" => Home,
        "end" => End,
        "pageup" | "pgup" => PageUp,
        "pagedown" | "pgdn" => PageDown,
        "insert" | "ins" => Insert,
        _ => {
            // 功能键 F1..F12
            if let Some(rest) = lower.strip_prefix('f')
                && let Ok(n) = rest.parse::<u8>()
                && (1..=12).contains(&n)
            {
                return Some(F(n));
            }
            // 单字符按键
            let mut chars = s.chars();
            let first = chars.next()?;
            if chars.next().is_none() {
                Char(first.to_ascii_lowercase())
            } else {
                return None;
            }
        }
    };
    Some(code)
}

/// 返回键名的本地化标签。zh-CN 下 Enter→回车，其余键名保持英文。
fn keycode_label_i18n(code: KeyCode, locale: &str) -> String {
    use KeyCode::*;
    match code {
        Enter => {
            if locale == "zh-CN" {
                "回车".into()
            } else {
                "Enter".into()
            }
        }
        Esc => "Esc".into(),
        Tab => "Tab".into(),
        BackTab => "Shift+Tab".into(),
        Char(' ') => "Space".into(),
        Up => "Up".into(),
        Down => "Down".into(),
        Left => "Left".into(),
        Right => "Right".into(),
        Backspace => "Backspace".into(),
        Delete => "Delete".into(),
        Home => "Home".into(),
        End => "End".into(),
        PageUp => "PageUp".into(),
        PageDown => "PageDown".into(),
        Insert => "Insert".into(),
        F(n) => format!("F{}", n),
        Char(c) => c.to_string(),
        other => format!("{:?}", other),
    }
}

/// 运行时键位映射表。
pub struct KeyMap {
    map: HashMap<Action, Vec<KeyCombo>>,
}

impl Default for KeyMap {
    fn default() -> Self {
        let map = Action::ALL
            .iter()
            .map(|a| (*a, a.default_combos()))
            .collect();
        Self { map }
    }
}

impl KeyMap {
    /// 以默认绑定为基底，叠加用户在 config 中覆盖的部分。
    pub fn from_persisted(over: &PersistedKeymap) -> Self {
        let mut km = KeyMap::default();
        for (name, combos) in over {
            if let Some(action) = Action::from_name(name) {
                let parsed: Vec<KeyCombo> =
                    combos.iter().filter_map(|s| KeyCombo::parse(s)).collect();
                // 仅当用户给出至少一个可解析组合时才覆盖，避免误把动作清空
                if !parsed.is_empty() {
                    km.map.insert(action, parsed);
                }
            }
        }
        km
    }

    /// 导出为持久化形式（含全部动作，便于用户在 config 中发现与编辑）。
    pub fn to_persisted(&self) -> PersistedKeymap {
        Action::ALL
            .iter()
            .map(|a| {
                let labels = self
                    .map
                    .get(a)
                    .map(|v| v.iter().map(|c| c.to_label()).collect())
                    .unwrap_or_default();
                (a.name().to_string(), labels)
            })
            .collect()
    }

    /// 将按键事件解析为语义动作（按 `Action::ALL` 顺序取首个匹配）。
    pub fn action_for(&self, ev: KeyEvent) -> Option<Action> {
        for action in Action::ALL {
            if let Some(combos) = self.map.get(&action)
                && combos.iter().any(|c| c.matches(&ev))
            {
                return Some(action);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn parse_and_label_roundtrip() {
        for s in ["Ctrl+c", "Tab", "Up", "Space", "j", "F5", "Shift+Tab"] {
            let combo = KeyCombo::parse(s).expect("should parse");
            // 标签可能规范化大小写，但再次解析应得到等价组合
            let relabeled = combo.to_label();
            let reparsed = KeyCombo::parse(&relabeled).expect("relabel should parse");
            assert_eq!(combo, reparsed, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn defaults_resolve_expected_actions() {
        let km = KeyMap::default();
        assert_eq!(
            km.action_for(ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        assert_eq!(
            km.action_for(ev(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(Action::Down)
        );
        assert_eq!(
            km.action_for(ev(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::NextTab)
        );
        // Ctrl+l 与裸 l 互不混淆
        assert_eq!(
            km.action_for(ev(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            Some(Action::ToggleLanguage)
        );
        assert_eq!(
            km.action_for(ev(KeyCode::Char('d'), KeyModifiers::NONE)),
            Some(Action::Right)
        );
    }

    #[test]
    fn user_override_replaces_default() {
        let mut over = PersistedKeymap::new();
        over.insert("quit".into(), vec!["Ctrl+x".into()]);
        let km = KeyMap::from_persisted(&over);
        assert_eq!(
            km.action_for(ev(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
        // 旧的 Ctrl+c 被覆盖后不再触发退出
        assert_eq!(
            km.action_for(ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn invalid_override_is_ignored_not_panicked() {
        let mut over = PersistedKeymap::new();
        over.insert("quit".into(), vec!["NotAKey++".into()]);
        let km = KeyMap::from_persisted(&over);
        // 无效覆盖被忽略，默认 Ctrl+c 仍生效
        assert_eq!(
            km.action_for(ev(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn shift_tab_resolves_prev_tab_in_all_encodings() {
        let km = KeyMap::default();
        // 终端可能以三种形态上报 Shift+Tab，均应解析为 PrevTab。
        for (code, mods) in [
            (KeyCode::BackTab, KeyModifiers::NONE),
            (KeyCode::BackTab, KeyModifiers::SHIFT),
            (KeyCode::Tab, KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                km.action_for(ev(code, mods)),
                Some(Action::PrevTab),
                "Shift+Tab 编码 {:?}+{:?} 应解析为 PrevTab",
                code,
                mods
            );
        }
        // 裸 Tab 仍是 NextTab，不被规范化波及。
        assert_eq!(
            km.action_for(ev(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::NextTab)
        );
    }

    #[test]
    fn prev_tab_survives_persist_roundtrip() {
        // 回归：存盘再读后 BackTab 绑定一度被 to_label/parse 悄悄丢成 Tab+Shift，
        // 使 Windows 上「上一标签页」失效。经规范化后任何编码都应仍命中 PrevTab。
        let persisted = KeyMap::default().to_persisted();
        let km = KeyMap::from_persisted(&persisted);
        for (code, mods) in [
            (KeyCode::BackTab, KeyModifiers::NONE),
            (KeyCode::BackTab, KeyModifiers::SHIFT),
            (KeyCode::Tab, KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                km.action_for(ev(code, mods)),
                Some(Action::PrevTab),
                "持久化往返后 {:?}+{:?} 仍应为 PrevTab",
                code,
                mods
            );
        }
    }

    #[test]
    fn persisted_contains_all_actions() {
        let p = KeyMap::default().to_persisted();
        assert_eq!(p.len(), Action::ALL.len());
        assert!(p.contains_key("toggle_language"));
    }

    #[test]
    fn ctrl_r_resolves_history() {
        let km = KeyMap::default();
        assert_eq!(
            km.action_for(ev(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            Some(Action::History)
        );
        // 裸 r 仍是 Refresh，不被 Ctrl+r 波及。
        assert_eq!(
            km.action_for(ev(KeyCode::Char('r'), KeyModifiers::NONE)),
            Some(Action::Refresh)
        );
    }
}
