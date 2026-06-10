//! 目标/CIDR 输入框的「最近使用」交互与渲染——所有工具复用。
//!
//! 键位（零冲突）：
//! - 编辑态：`→` 在行尾且有补全 → 采纳整条；否则照常移光标。`Ctrl+R`(Action::History) 开下拉。
//! - 下拉态：`↑/↓` 选、`Enter` 填入、`Esc`/其它键关。期间不触发字段切换。

use crate::history::History;
use crate::keymap::Action;
use crate::utils::i18n::I18n;
use crate::utils::textinput::TextInput;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
};

/// 单个输入框的 MRU 下拉 UI 状态。
#[derive(Debug, Clone, Default)]
pub struct MruState {
    pub open: bool,
    pub sel: usize,
}

/// 处理一次按键的 MRU 部分。返回 `true` 表示已消费（调用方应 `return`，不再走原文本编辑）。
///
/// `running` 为真（任务进行中）时不接管编辑，但仍允许下拉已开时的导航/关闭。
pub fn handle_mru_key(
    input: &mut TextInput,
    mru: &mut MruState,
    hist: &History,
    key: KeyEvent,
    action: Option<Action>,
    running: bool,
) -> bool {
    let suggest = hist.suggest(&input.value());
    let entries: Vec<String> = hist.entries().to_vec();
    handle_mru_key_data(input, mru, &entries, suggest, key, action, running)
}

/// 处理一次按键的 MRU 部分（接受预取的数据，避免同时借用多个字段）。
///
/// `entries` 为历史条目列表，`suggest` 为当前输入的补全建议。
pub fn handle_mru_key_data(
    input: &mut TextInput,
    mru: &mut MruState,
    entries: &[String],
    suggest: Option<String>,
    key: KeyEvent,
    action: Option<Action>,
    running: bool,
) -> bool {
    if mru.open {
        let len = entries.len();
        match action {
            Some(Action::Up) => {
                mru.sel = mru.sel.saturating_sub(1);
            }
            Some(Action::Down) => {
                if len > 0 {
                    mru.sel = (mru.sel + 1).min(len - 1);
                }
            }
            Some(Action::Confirm) | Some(Action::Toggle) => {
                if let Some(v) = entries.get(mru.sel) {
                    *input = TextInput::with_text(v);
                }
                mru.open = false;
            }
            _ => {
                // Back / 其它任意键 → 关闭，不改值。
                mru.open = false;
            }
        }
        return true;
    }

    if running {
        return false;
    }

    // 打开历史下拉。
    if action == Some(Action::History) {
        mru.open = true;
        mru.sel = 0;
        return true;
    }

    // 行尾 `→` 采纳灰字补全。
    if key.code == KeyCode::Right && input.cursor() == input.len() {
        if let Some(s) = suggest {
            *input = TextInput::with_text(&s);
            return true;
        }
    }

    false
}

/// 计算该输入框当前应渲染的 spans（含行尾灰字补全）。
pub fn mru_ghost_spans(
    input: &TextInput,
    hist: &History,
    active: bool,
    base: Style,
) -> Vec<Span<'static>> {
    let typed = input.value();
    let ghost = if active && input.cursor() == input.len() && !typed.is_empty() {
        hist.suggest(&typed)
            .and_then(|s| s.strip_prefix(&typed).map(|r| r.to_string()))
            .filter(|r| !r.is_empty())
    } else {
        None
    };
    input.render_spans_with_ghost(active, base, ghost.as_deref())
}

/// 在 `anchor` 区域顶部画一个历史下拉浮层（覆盖式）。entries 为空时画一行「暂无历史」。
pub fn draw_mru_popup(
    f: &mut Frame,
    anchor: Rect,
    entries: &[String],
    sel: usize,
    i18n: &I18n,
) {
    let max_rows = entries.len().max(1).min(8) as u16;
    let height = max_rows + 2; // 含上下边框
    let width = anchor.width.min(48).max(20);
    let area = Rect::new(
        anchor.x,
        anchor.y,
        width.min(anchor.width),
        height.min(anchor.height.max(3)),
    );

    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(i18n.t("mru_popup_title"))
        .border_style(Style::default().fg(Color::Yellow));

    if entries.is_empty() {
        let inner = block.inner(area);
        f.render_widget(block, area);
        let empty = List::new(vec![ListItem::new(i18n.t("mru_popup_empty"))])
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, inner);
        return;
    }

    let items: Vec<ListItem> = entries
        .iter()
        .map(|e| ListItem::new(e.clone()))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = ListState::default();
    state.select(Some(sel.min(entries.len() - 1)));
    f.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn ev(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn right_at_end_accepts_suggestion() {
        let mut hist = History::new(5);
        hist.record("192.168.1.1");
        let mut input = TextInput::with_text("192");
        let mut mru = MruState::default();
        // 光标在末尾，→ 采纳整条。
        let consumed = handle_mru_key(
            &mut input,
            &mut mru,
            &hist,
            ev(KeyCode::Right),
            Some(Action::Right),
            false,
        );
        assert!(consumed);
        assert_eq!(input.value(), "192.168.1.1");
    }

    #[test]
    fn ctrl_r_opens_and_enter_fills() {
        let mut hist = History::new(5);
        hist.record("a");
        hist.record("b"); // entries: ["b","a"]
        let mut input = TextInput::new();
        let mut mru = MruState::default();

        // History 动作打开下拉。
        assert!(handle_mru_key(&mut input, &mut mru, &hist, ev(KeyCode::Null), Some(Action::History), false));
        assert!(mru.open);

        // Down 选到第 2 项 "a"。
        handle_mru_key(&mut input, &mut mru, &hist, ev(KeyCode::Down), Some(Action::Down), false);
        assert_eq!(mru.sel, 1);

        // Enter 填入并关闭。
        handle_mru_key(&mut input, &mut mru, &hist, ev(KeyCode::Enter), Some(Action::Confirm), false);
        assert!(!mru.open);
        assert_eq!(input.value(), "a");
    }

    #[test]
    fn esc_closes_without_change() {
        let mut hist = History::new(5);
        hist.record("x");
        let mut input = TextInput::with_text("keep");
        let mut mru = MruState { open: true, sel: 0 };
        handle_mru_key(&mut input, &mut mru, &hist, ev(KeyCode::Esc), Some(Action::Back), false);
        assert!(!mru.open);
        assert_eq!(input.value(), "keep");
    }
}
