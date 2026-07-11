//! 带光标的单行文本输入，供各编辑框（适配器 IP、扫描 CIDR、Ping/端口目标等）复用。
//!
//! 此前各模块都只支持「从末尾退格」，想改中间的字符得几乎全删重打。
//! 本组件维护一个光标位置，支持：插入 / 退格 / 删除、左右移动、Home/End，
//! 以及「点击定位光标」（鼠标支持）。渲染时把光标处反显成一个高亮块。
//!
//! 约定：内容均为 ASCII（数字、`.`、`/`、`:`、主机名字符等单宽字符），
//! 因此字符索引 == 终端列偏移，点击列即字符列。

use crossterm::event::KeyCode;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

#[derive(Debug, Clone, Default)]
pub struct TextInput {
    chars: Vec<char>,
    /// 光标位于第 `cursor` 个字符之前；取值范围 0..=len。
    cursor: usize,
}

impl TextInput {
    pub fn new() -> Self {
        Self {
            chars: Vec::new(),
            cursor: 0,
        }
    }

    /// 以初始内容构造，光标置于末尾。
    pub fn with_text(s: &str) -> Self {
        let chars: Vec<char> = s.chars().collect();
        let cursor = chars.len();
        Self { chars, cursor }
    }

    pub fn value(&self) -> String {
        self.chars.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, c: char) {
        self.chars.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.chars.len() {
            self.chars.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    /// 鼠标点击：`rel_col` 为点击列相对文本起始列的偏移，定位光标到该字符前。
    pub fn set_cursor_col(&mut self, rel_col: usize) {
        self.cursor = rel_col.min(self.chars.len());
    }

    /// 处理一个与文本编辑相关的按键；`filter` 决定哪些可打印字符被接受。
    /// 返回 `true` 表示该按键已被消费（调用方据此决定是否继续走语义动作）。
    pub fn handle_key(&mut self, code: KeyCode, filter: impl Fn(char) -> bool) -> bool {
        match code {
            KeyCode::Char(c) if filter(c) => {
                self.insert(c);
                true
            }
            KeyCode::Backspace => {
                self.backspace();
                true
            }
            KeyCode::Delete => {
                self.delete();
                true
            }
            KeyCode::Left => {
                self.left();
                true
            }
            KeyCode::Right => {
                self.right();
                true
            }
            KeyCode::Home => {
                self.home();
                true
            }
            KeyCode::End => {
                self.end();
                true
            }
            _ => false,
        }
    }

    /// 渲染为若干 Span。`active` 为真时在光标处反显一个高亮块，
    /// 末尾光标用一个高亮空格表示；非活跃时仅以 `base` 样式平铺。
    pub fn render_spans(&self, active: bool, base: Style) -> Vec<Span<'static>> {
        if !active {
            return vec![Span::styled(self.value(), base)];
        }
        let cursor_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let mut spans = Vec::new();
        if self.cursor > 0 {
            let pre: String = self.chars[..self.cursor].iter().collect();
            spans.push(Span::styled(pre, base));
        }
        if self.cursor < self.chars.len() {
            spans.push(Span::styled(
                self.chars[self.cursor].to_string(),
                cursor_style,
            ));
            if self.cursor + 1 < self.chars.len() {
                let post: String = self.chars[self.cursor + 1..].iter().collect();
                spans.push(Span::styled(post, base));
            }
        } else {
            spans.push(Span::styled(" ".to_string(), cursor_style));
        }
        spans
    }

    /// 渲染并在行尾追加灰字「幽灵补全」。
    /// 仅当 `active` 且光标在末尾且 `ghost` 非空时追加；否则退化为 `render_spans`。
    /// `ghost` 为「已输入串之后的剩余部分」（调用方用 History::suggest 去前缀算出）。
    pub fn render_spans_with_ghost(
        &self,
        active: bool,
        base: Style,
        ghost: Option<&str>,
    ) -> Vec<Span<'static>> {
        if let Some(g) = ghost {
            if active && self.cursor == self.chars.len() && !g.is_empty() {
                let typed: String = self.chars.iter().collect();
                let ghost_style = Style::default().fg(Color::DarkGray);
                return vec![
                    Span::styled(typed, base),
                    Span::styled(g.to_string(), ghost_style),
                ];
            }
        }
        self.render_spans(active, base)
    }
}

/// 常用字符过滤器：IP / 掩码 / 网关 / DNS（数字与点）。
pub fn filter_ipv4(c: char) -> bool {
    c.is_ascii_digit() || c == '.'
}

/// CIDR（数字、点、斜杠）。
pub fn filter_cidr(c: char) -> bool {
    c.is_ascii_digit() || c == '.' || c == '/'
}

/// 主机名 / IP（可见 ASCII，且非空格）。
pub fn filter_host(c: char) -> bool {
    c.is_ascii() && !c.is_control() && c != ' '
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode;

    #[test]
    fn insert_and_move() {
        let mut t = TextInput::with_text("192.168.1.1");
        // 光标在末尾
        assert_eq!(t.cursor(), 11);
        // Home 后在开头
        t.home();
        assert_eq!(t.cursor(), 0);
        // 右移 4 到 '1' 后（"192." 之后），删除一位再输入
        for _ in 0..4 {
            t.right();
        }
        assert_eq!(t.cursor(), 4);
    }

    #[test]
    fn edit_middle_without_clearing() {
        // 把 192.168.192.99 的第二段 168 改成 192：把光标移到 '1' 后逐位改
        let mut t = TextInput::with_text("192.168.192.99");
        t.home();
        for _ in 0..4 {
            t.right();
        } // 光标在 "192." 后，正对 '1'(168 的首位)
        t.delete();
        t.delete();
        t.delete(); // 删掉 168
        for c in "192".chars() {
            t.insert(c);
        }
        assert_eq!(t.value(), "192.192.192.99");
    }

    #[test]
    fn filter_rejects_disallowed() {
        let mut t = TextInput::new();
        assert!(t.handle_key(KeyCode::Char('1'), filter_ipv4));
        assert!(!t.handle_key(KeyCode::Char('x'), filter_ipv4));
        assert!(t.handle_key(KeyCode::Char('/'), filter_cidr));
        assert_eq!(t.value(), "1/");
    }

    #[test]
    fn backspace_delete_bounds() {
        let mut t = TextInput::new();
        // 空串退格/删除不 panic
        t.backspace();
        t.delete();
        t.left();
        t.right();
        assert_eq!(t.value(), "");
    }

    #[test]
    fn click_positions_cursor() {
        let mut t = TextInput::with_text("8.8.8.8");
        t.set_cursor_col(3);
        assert_eq!(t.cursor(), 3);
        // 越界点击夹到末尾
        t.set_cursor_col(999);
        assert_eq!(t.cursor(), 7);
    }

    #[test]
    fn ghost_appended_only_at_end_active() {
        let t = TextInput::with_text("192");
        let base = Style::default();
        // 光标在末尾 + active + 有 ghost → 末尾追加灰字 span，内容为剩余串。
        let spans = t.render_spans_with_ghost(true, base, Some(".168.1.1"));
        let last = spans.last().unwrap();
        assert_eq!(last.content, ".168.1.1");
        // 无 ghost → 等价 render_spans。
        let spans2 = t.render_spans_with_ghost(true, base, None);
        let spans_plain = t.render_spans(true, base);
        assert_eq!(spans2.len(), spans_plain.len());
    }
}
