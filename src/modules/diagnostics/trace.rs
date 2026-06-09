//! 路由跟踪（traceroute）。
//!
//! Windows 实现：用 `IcmpSendEcho` 配合逐跳递增的 TTL（IP_OPTION_INFORMATION），
//! 中间路由返回 `IP_TTL_EXPIRED_TRANSIT(11013)` 暴露其地址，到达目标时返回成功。
//! 不依赖外部 `tracert` 程序。非 Windows 暂以"不支持"提示占位（后续统一迁移）。

use super::{config_field_item, FocusArea};
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::textinput::{filter_host, TextInput};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState},
};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

const DEFAULT_MAX_HOPS: u32 = 30;

#[derive(Debug, Clone)]
struct Hop {
    ttl: u8,
    addr: Option<Ipv4Addr>,
    rtt_ms: Option<u64>,
    host: Option<String>,
}

#[derive(Debug)]
enum TraceEvent {
    Hop(Hop),
    Done,
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone)]
struct TraceConfig {
    target: TextInput,
    max_hops: TextInput,
    timeout_ms: TextInput,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            target: TextInput::with_text("8.8.8.8"),
            max_hops: TextInput::with_text(&DEFAULT_MAX_HOPS.to_string()),
            timeout_ms: TextInput::with_text("1000"),
        }
    }
}

pub struct TraceTool {
    config: TraceConfig,
    config_state: ListState,
    running: bool,
    error_key: Option<String>,
    done: bool,

    hops: Vec<Hop>,
    result_state: TableState,

    tx: mpsc::Sender<TraceEvent>,
    rx: mpsc::Receiver<TraceEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl TraceTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(64);
        Self {
            config: TraceConfig::default(),
            config_state,
            running: false,
            error_key: None,
            done: false,
            hops: Vec::new(),
            result_state: TableState::default(),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                TraceEvent::Hop(hop) => {
                    self.hops.push(hop);
                    if self.result_state.selected().is_none() {
                        self.result_state.select(Some(0));
                    }
                }
                TraceEvent::Done => {
                    self.running = false;
                    self.done = true;
                }
                TraceEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                }
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>, focus: FocusArea) {
        match focus {
            FocusArea::Main => match action {
                Some(Action::Toggle) => {
                    if self.running {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
                Some(Action::Down) => self.next_result(),
                Some(Action::Up) => self.prev_result(),
                _ => {}
            },
            FocusArea::Config => self.handle_config_key(key, action),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, action: Option<Action>) {
        // 带光标编辑：目标接受主机名字符，跳数/超时仅数字。
        if !self.running {
            if let Some(idx) = self.config_state.selected() {
                let too_long =
                    matches!(key.code, KeyCode::Char(_)) && self.field_mut(idx).len() >= 64;
                if !too_long {
                    let consumed = if idx == 0 {
                        self.field_mut(idx).handle_key(key.code, filter_host)
                    } else {
                        self.field_mut(idx).handle_key(key.code, |c| c.is_ascii_digit())
                    };
                    if consumed {
                        return;
                    }
                }
            }
        }
        match action {
            Some(Action::Down) => self.next_config(),
            Some(Action::Up) => self.prev_config(),
            _ => {}
        }
    }

    fn field_mut(&mut self, idx: usize) -> &mut TextInput {
        match idx {
            0 => &mut self.config.target,
            1 => &mut self.config.max_hops,
            _ => &mut self.config.timeout_ms,
        }
    }

    fn next_config(&mut self) {
        let i = self.config_state.selected().map(|i| (i + 1) % 3).unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| if i == 0 { 2 } else { i - 1 })
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn next_result(&mut self) {
        if self.hops.is_empty() {
            return;
        }
        let i = self
            .result_state
            .selected()
            .map(|i| (i + 1) % self.hops.len())
            .unwrap_or(0);
        self.result_state.select(Some(i));
    }

    fn prev_result(&mut self) {
        if self.hops.is_empty() {
            return;
        }
        let n = self.hops.len();
        let i = self
            .result_state
            .selected()
            .map(|i| if i == 0 { n - 1 } else { i - 1 })
            .unwrap_or(0);
        self.result_state.select(Some(i));
    }

    fn start(&mut self) {
        let max_hops: u32 = self
            .config
            .max_hops
            .value()
            .parse()
            .unwrap_or(DEFAULT_MAX_HOPS)
            .clamp(1, 64);
        let timeout_ms: u32 = self
            .config
            .timeout_ms
            .value()
            .parse()
            .unwrap_or(1000)
            .clamp(100, 10000);
        let target = self.config.target.value().trim().to_string();
        if target.is_empty() {
            self.error_key = Some("diag_trace_err".to_string());
            return;
        }

        self.running = true;
        self.done = false;
        self.error_key = None;
        self.hops.clear();
        self.result_state.select(None);
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();

        tokio::spawn(async move {
            run_trace(target, max_hops, timeout_ms, tx, abort).await;
        });
    }

    fn stop(&mut self) {
        self.running = false;
        *self.abort_flag.lock().unwrap() = true;
    }

    // -------------------------------------------------------------------------
    // 绘图
    // -------------------------------------------------------------------------

    pub fn draw(
        &mut self,
        f: &mut Frame,
        main_area: Rect,
        config_area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        self.draw_main(f, main_area, i18n, is_focused, active_focus);
        self.draw_config(f, config_area, i18n, is_focused, active_focus);
    }

    fn draw_main(
        &mut self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Main {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_main_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        if !is_focused {
            let p = Paragraph::new(i18n.t("diag_msg_focus_hint"))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(p, inner);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(inner);

        let header = Row::new(vec![
            Cell::from(i18n.t("diag_trace_col_hop")).style(Style::default().fg(Color::Gray)),
            Cell::from(i18n.t("diag_trace_col_addr")).style(Style::default().fg(Color::Gray)),
            Cell::from(i18n.t("diag_trace_col_rtt")).style(Style::default().fg(Color::Gray)),
            Cell::from(i18n.t("diag_trace_col_host")).style(Style::default().fg(Color::Gray)),
        ])
        .height(1);

        let star = i18n.t("diag_trace_timeout_star");
        let rows = self.hops.iter().map(|h| {
            let addr = h
                .addr
                .map(|a| a.to_string())
                .unwrap_or_else(|| star.clone());
            let rtt = h
                .rtt_ms
                .map(|r| format!("{} ms", r))
                .unwrap_or_else(|| star.clone());
            let host = h.host.clone().unwrap_or_else(|| "-".to_string());
            Row::new(vec![
                Cell::from(format!("{:>2}", h.ttl)).style(Style::default().fg(theme::COLOR_SECONDARY)),
                Cell::from(addr).style(Style::default().fg(Color::White)),
                Cell::from(rtt).style(Style::default().fg(theme::COLOR_PRIMARY)),
                Cell::from(host).style(Style::default().fg(Color::DarkGray)),
            ])
        });
        let table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(17),
                Constraint::Length(10),
                Constraint::Min(0),
            ],
        )
        .header(header)
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("> ");
        f.render_stateful_widget(table, chunks[0], &mut self.result_state);

        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!("{} | {}", i18n.t("diag_status_running"), i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else if self.done {
            (i18n.t("diag_trace_done"), Style::default().fg(theme::COLOR_SECONDARY))
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(text).style(style), chunks[1]);
    }

    fn draw_config(
        &mut self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Config {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_config_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let is_active = is_focused && active_focus == FocusArea::Config;
        let labels = [
            i18n.t("diag_trace_target"),
            i18n.t("diag_trace_maxhops"),
            i18n.t("diag_trace_timeout"),
        ];
        let inputs = [
            &self.config.target,
            &self.config.max_hops,
            &self.config.timeout_ms,
        ];
        let selected = self.config_state.selected();

        let list_items: Vec<ListItem> = (0..3)
            .map(|i| {
                let is_sel = selected == Some(i);
                let active = is_sel && is_active && !self.running;
                let hint = if active {
                    Some(if i == 0 {
                        i18n.t("diag_hint_input")
                    } else {
                        i18n.t("diag_hint_digits")
                    })
                } else {
                    None
                };
                config_field_item(&labels[i], is_sel, is_active, inputs[i], active, hint)
            })
            .collect();

        f.render_widget(List::new(list_items), inner);
    }
}

// -----------------------------------------------------------------------------
// 平台实现
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
async fn run_trace(
    target: String,
    max_hops: u32,
    timeout_ms: u32,
    tx: mpsc::Sender<TraceEvent>,
    abort: Arc<Mutex<bool>>,
) {
    use std::net::IpAddr;

    // 解析目标为 IPv4
    let dest_v4: Ipv4Addr = match target.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4,
        Ok(IpAddr::V6(_)) => {
            let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
            return;
        }
        Err(_) => match tokio::net::lookup_host((target.as_str(), 0u16)).await {
            Ok(mut it) => loop {
                match it.next() {
                    Some(sa) => {
                        if let IpAddr::V4(v4) = sa.ip() {
                            break v4;
                        }
                    }
                    None => {
                        let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
                        return;
                    }
                }
            },
            Err(_) => {
                let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
                return;
            }
        },
    };

    for ttl in 1..=max_hops as u8 {
        if *abort.lock().unwrap() {
            return;
        }

        let probe = tokio::task::spawn_blocking(move || {
            super::icmp::echo_once(dest_v4, ttl, timeout_ms)
        })
        .await;
        let result = match probe {
            Ok(v) => v,
            Err(_) => return,
        };

        let addr = result.addr;

        // 反向 DNS（best-effort，不阻塞 UI）
        let host = if let Some(a) = addr {
            tokio::task::spawn_blocking(move || {
                dns_lookup::lookup_addr(&std::net::IpAddr::V4(a)).ok()
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };

        let reached = addr == Some(dest_v4) || result.reached();

        let _ = tx
            .send(TraceEvent::Hop(Hop {
                ttl,
                addr,
                rtt_ms: result.rtt_ms,
                host,
            }))
            .await;

        if reached {
            let _ = tx.send(TraceEvent::Done).await;
            return;
        }
    }

    let _ = tx.send(TraceEvent::Done).await;
}

#[cfg(not(target_os = "windows"))]
async fn run_trace(
    _target: String,
    _max_hops: u32,
    _timeout_ms: u32,
    tx: mpsc::Sender<TraceEvent>,
    _abort: Arc<Mutex<bool>>,
) {
    let _ = tx
        .send(TraceEvent::Error("diag_trace_unsupported".into()))
        .await;
}
