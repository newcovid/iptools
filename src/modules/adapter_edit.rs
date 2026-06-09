//! 适配器 IP 配置编辑表单（静态 IP / DHCP）。
//!
//! 安全设计：编辑态独立于只读视图；应用前需通过 [`validate`] 校验，
//! 再经一次显式确认浮层方可写入；实际写入在后台线程进行并把结果回传，
//! 不阻塞 UI。真正改写系统的逻辑收敛在 [`crate::utils::ipconfig`]。

use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::ipconfig;
use crate::utils::net::InterfaceInfo;
use crate::utils::textinput::{filter_ipv4, TextInput};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};
use std::net::Ipv4Addr;
use std::str::FromStr;
use tokio::sync::mpsc;

const FIELD_COUNT: usize = 6;

/// on_key 的处理结果，告知调用方（AdapterModule）下一步。
pub enum EditOutcome {
    /// 留在编辑态
    Stay,
    /// 取消编辑，返回只读视图
    Cancel,
    /// 应用成功，返回只读视图并刷新网卡列表
    Done,
}

pub struct EditForm {
    guid: String,
    adapter_name: String,
    use_dhcp: bool,
    ip: TextInput,
    mask: TextInput,
    gateway: TextInput,
    dns1: TextInput,
    dns2: TextInput,

    field: usize,
    confirming: bool,
    applying: bool,
    error_key: Option<String>,
    /// 应用结果：Ok 或 Err(人类可读信息)
    result: Option<Result<(), String>>,

    tx: mpsc::Sender<Result<(), String>>,
    rx: mpsc::Receiver<Result<(), String>>,
}

impl EditForm {
    pub fn from_interface(iface: &InterfaceInfo) -> Self {
        let (tx, rx) = mpsc::channel(4);
        let ip = iface.ipv4.first().cloned().unwrap_or_default();
        let mask = iface
            .cidr
            .as_ref()
            .and_then(|c| cidr_to_mask(c))
            .unwrap_or_default();
        Self {
            guid: iface.guid.clone(),
            adapter_name: iface.name.clone(),
            use_dhcp: iface.dhcp_enabled,
            ip: TextInput::with_text(&ip),
            mask: TextInput::with_text(&mask),
            gateway: TextInput::new(),
            dns1: TextInput::new(),
            dns2: TextInput::new(),
            field: 0,
            confirming: false,
            applying: false,
            error_key: None,
            result: None,
            tx,
            rx,
        }
    }

    pub fn update(&mut self) {
        if let Ok(res) = self.rx.try_recv() {
            self.applying = false;
            self.result = Some(res);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>) -> EditOutcome {
        // 写入进行中：忽略一切按键
        if self.applying {
            return EditOutcome::Stay;
        }

        // 结果展示态：成功→任意键退出；失败→任意键回到表单重试
        if let Some(res) = &self.result {
            let ok = res.is_ok();
            self.result = None;
            return if ok { EditOutcome::Done } else { EditOutcome::Stay };
        }

        // 确认浮层态
        if self.confirming {
            match action {
                Some(Action::Confirm) => self.apply(),
                Some(Action::Back) => self.confirming = false,
                _ => {}
            }
            return EditOutcome::Stay;
        }

        // 表单编辑态
        match action {
            Some(Action::Back) => return EditOutcome::Cancel,
            Some(Action::Up) => {
                self.field = if self.field == 0 {
                    FIELD_COUNT - 1
                } else {
                    self.field - 1
                };
                return EditOutcome::Stay;
            }
            Some(Action::Down) => {
                self.field = (self.field + 1) % FIELD_COUNT;
                return EditOutcome::Stay;
            }
            _ => {}
        }

        if self.field == 0 {
            // 模式字段：左右切换 DHCP / 静态
            if matches!(action, Some(Action::Left) | Some(Action::Right)) {
                self.use_dhcp = !self.use_dhcp;
            }
        } else if !self.use_dhcp {
            // 地址字段文本编辑（仅静态模式）：带光标，支持中间插入/删除、
            // 左右移动、Home/End。最长 15 字符（IPv4 文本上限）。
            let at_cap =
                self.field_mut().len() >= 15 && matches!(key.code, KeyCode::Char(_));
            if !at_cap && self.field_mut().handle_key(key.code, filter_ipv4) {
                return EditOutcome::Stay;
            }
        }

        // 确认键：先校验，通过则进入确认浮层
        if action == Some(Action::Confirm) {
            match self.validate() {
                Ok(()) => {
                    self.error_key = None;
                    self.confirming = true;
                }
                Err(k) => self.error_key = Some(k.to_string()),
            }
        }

        EditOutcome::Stay
    }

    /// 当前选中字段对应的文本输入（仅对 1..=5 有效；字段 0 是模式开关）。
    fn field_mut(&mut self) -> &mut TextInput {
        self.input_mut(self.field)
    }

    fn input_mut(&mut self, idx: usize) -> &mut TextInput {
        match idx {
            1 => &mut self.ip,
            2 => &mut self.mask,
            3 => &mut self.gateway,
            4 => &mut self.dns1,
            _ => &mut self.dns2,
        }
    }

    fn input(&self, idx: usize) -> &TextInput {
        match idx {
            1 => &self.ip,
            2 => &self.mask,
            3 => &self.gateway,
            4 => &self.dns1,
            _ => &self.dns2,
        }
    }

    /// 返回校验错误对应的 i18n 键。
    fn validate(&self) -> Result<(), &'static str> {
        if self.use_dhcp {
            return Ok(());
        }
        if !is_ipv4(&self.ip.value()) {
            return Err("adapter_err_ip");
        }
        if !is_valid_mask(&self.mask.value()) {
            return Err("adapter_err_mask");
        }
        if !self.gateway.is_empty() && !is_ipv4(&self.gateway.value()) {
            return Err("adapter_err_gw");
        }
        if (!self.dns1.is_empty() && !is_ipv4(&self.dns1.value()))
            || (!self.dns2.is_empty() && !is_ipv4(&self.dns2.value()))
        {
            return Err("adapter_err_dns");
        }
        Ok(())
    }

    fn apply(&mut self) {
        self.confirming = false;
        self.applying = true;
        self.result = None;

        let tx = self.tx.clone();
        let guid = self.guid.clone();
        let use_dhcp = self.use_dhcp;
        let ip = self.ip.value();
        let mask = self.mask.value();
        let gateway = if self.gateway.is_empty() {
            None
        } else {
            Some(self.gateway.value())
        };
        let mut dns = Vec::new();
        if !self.dns1.is_empty() {
            dns.push(self.dns1.value());
        }
        if !self.dns2.is_empty() {
            dns.push(self.dns2.value());
        }

        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || {
                if use_dhcp {
                    ipconfig::apply_dhcp(&guid)
                } else {
                    ipconfig::apply_static(&guid, &ip, &mask, gateway.as_deref(), &dns)
                }
            })
            .await
            .unwrap_or_else(|_| Err("internal task error".to_string()));
            let _ = tx.send(res).await;
        });
    }

    /// 字段列表所占的矩形（供鼠标命中测试）。**必须与 `draw` 内的布局一致**。
    pub fn field_list_rect(area: Rect) -> Rect {
        let inner = Block::default().borders(Borders::ALL).inner(area);
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .split(inner)[0]
    }

    /// 鼠标点击编辑表单：选中点击到的字段；若为可编辑文本字段，定位光标到点击列。
    /// `area` 为整个编辑区域（与 draw 收到的一致）。
    pub fn click(&mut self, x: u16, y: u16, area: Rect) {
        if self.confirming || self.applying || self.result.is_some() {
            return;
        }
        let fields = Self::field_list_rect(area);
        let inside = x >= fields.x
            && x < fields.x + fields.width
            && y >= fields.y
            && y < fields.y + fields.height;
        if !inside {
            return;
        }
        let row = (y - fields.y) as usize;
        if row >= FIELD_COUNT {
            return;
        }
        self.field = row;
        // 文本字段（1..=5）且静态模式：把光标定位到点击列。
        // 取值文本起点 = 字段区左缘 + 2(标记) + 14(标签 {:<14})。
        if row >= 1 && !self.use_dhcp {
            let val_x = fields.x + 16;
            let col = x.saturating_sub(val_x) as usize;
            self.input_mut(row).set_cursor_col(col);
        }
    }

    // -------------------------------------------------------------------------
    // 绘图
    // -------------------------------------------------------------------------

    pub fn draw(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::COLOR_SECONDARY))
            .title(format!(
                " {} — {} ",
                i18n.t("adapter_edit_title"),
                self.adapter_name
            ));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(8),    // 字段表
                Constraint::Length(2), // 错误/状态
                Constraint::Length(2), // 操作提示
            ])
            .split(inner);

        // 字段列表
        let dimmed = Style::default().fg(Color::DarkGray);
        let normal = Style::default().fg(Color::White);
        let mode_val = if self.use_dhcp {
            i18n.t("adapter_type_dhcp")
        } else {
            i18n.t("adapter_type_static")
        };
        let labels = [
            i18n.t("adapter_edit_mode"),
            i18n.t("adapter_field_ip"),
            i18n.t("adapter_field_mask"),
            i18n.t("adapter_field_gw"),
            i18n.t("adapter_field_dns1"),
            i18n.t("adapter_field_dns2"),
        ];

        let mut items: Vec<ListItem> = Vec::with_capacity(FIELD_COUNT);
        for i in 0..FIELD_COUNT {
            let selected = i == self.field;
            // 模式字段恒可编辑；地址字段仅静态模式可编辑。
            let enabled = i == 0 || !self.use_dhcp;
            let marker = if selected { "> " } else { "  " };
            let label_span = Span::styled(
                format!("{}{:<14}", marker, labels[i]),
                if selected {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::Gray)
                },
            );
            let val_base = if !enabled {
                dimmed
            } else if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                normal
            };

            let mut spans = vec![label_span];
            if i == 0 {
                spans.push(Span::styled(mode_val.clone(), val_base));
            } else {
                // 选中且可编辑时显示光标块，便于在中间插入/删除。
                let active = selected && enabled;
                let input = self.input(i);
                if input.is_empty() && !active {
                    spans.push(Span::styled("-".to_string(), dimmed));
                } else {
                    spans.extend(input.render_spans(active, val_base));
                }
            }
            items.push(ListItem::new(Line::from(spans)));
        }
        f.render_widget(List::new(items), chunks[0]);

        // 错误 / 应用状态行
        let mut status_lines: Vec<Line> = Vec::new();
        if self.applying {
            status_lines.push(Line::styled(
                i18n.t("adapter_applying"),
                Style::default().fg(Color::Yellow),
            ));
        } else if let Some(res) = &self.result {
            match res {
                Ok(()) => status_lines.push(Line::styled(
                    i18n.t("adapter_apply_ok"),
                    Style::default().fg(theme::COLOR_UP),
                )),
                Err(msg) => status_lines.push(Line::styled(
                    format!("{}: {}", i18n.t("adapter_apply_fail"), msg),
                    Style::default().fg(theme::COLOR_ERROR),
                )),
            }
            status_lines.push(Line::styled(
                i18n.t("adapter_result_hint"),
                Style::default().fg(Color::Gray),
            ));
        } else if let Some(key) = &self.error_key {
            status_lines.push(Line::styled(
                i18n.t(key),
                Style::default().fg(theme::COLOR_ERROR),
            ));
        }
        f.render_widget(
            Paragraph::new(status_lines).wrap(Wrap { trim: true }),
            chunks[1],
        );

        // 操作提示
        let hint = Paragraph::new(i18n.t("adapter_edit_hint"))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true });
        f.render_widget(hint, chunks[2]);

        // 确认浮层
        if self.confirming {
            self.draw_confirm(f, area, i18n);
        }
    }

    fn draw_confirm(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let popup = centered_rect(area, 70, 40);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::COLOR_ERROR))
            .title(format!(" {} ", i18n.t("adapter_confirm_title")));
        let inner = block.inner(popup);
        f.render_widget(block, popup);

        let para = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                i18n.t("adapter_confirm_msg"),
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                i18n.t("adapter_confirm_hint"),
                Style::default().fg(Color::Yellow),
            )),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(para, inner);
    }
}

// -----------------------------------------------------------------------------
// 校验与转换辅助
// -----------------------------------------------------------------------------

fn is_ipv4(s: &str) -> bool {
    Ipv4Addr::from_str(s).is_ok()
}

/// 子网掩码必须是合法 IPv4 且为连续前导 1 的位模式（如 255.255.255.0）。
/// 同时排除全 0（/0）与全 1（/32）：/32 不是可用于接口的子网掩码，
/// EnableStatic 会以错误码 66 拒绝它。
fn is_valid_mask(s: &str) -> bool {
    match Ipv4Addr::from_str(s) {
        Ok(addr) => {
            let bits = u32::from(addr);
            // 取反后应为 2^k-1（低位全 1），等价于 bits 是连续高位 1；0 与全 1 掩码非法
            let inv = !bits;
            bits != 0 && bits != u32::MAX && inv & inv.wrapping_add(1) == 0
        }
        Err(_) => false,
    }
}

/// "192.168.1.100/24" -> "255.255.255.0"
fn cidr_to_mask(cidr: &str) -> Option<String> {
    let prefix: u32 = cidr.split('/').nth(1)?.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    let bits: u32 = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some(Ipv4Addr::from(bits).to_string())
}

fn centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ipv4() {
        assert!(is_ipv4("192.168.1.1"));
        assert!(!is_ipv4("192.168.1"));
        assert!(!is_ipv4("999.1.1.1"));
        assert!(!is_ipv4(""));
    }

    #[test]
    fn validates_subnet_mask() {
        assert!(is_valid_mask("255.255.255.0"));
        assert!(is_valid_mask("255.255.0.0"));
        assert!(is_valid_mask("255.255.255.252")); // /30 仍可用
        assert!(!is_valid_mask("255.255.255.255")); // /32 不可用于接口
        assert!(!is_valid_mask("255.0.255.0")); // 非连续
        assert!(!is_valid_mask("0.0.0.0"));
        assert!(!is_valid_mask("abc"));
    }

    #[test]
    fn cidr_to_mask_converts() {
        assert_eq!(cidr_to_mask("192.168.1.100/24").as_deref(), Some("255.255.255.0"));
        assert_eq!(cidr_to_mask("10.0.0.1/8").as_deref(), Some("255.0.0.0"));
        assert_eq!(cidr_to_mask("1.2.3.4/16").as_deref(), Some("255.255.0.0"));
        assert_eq!(cidr_to_mask("no-slash"), None);
    }
}
