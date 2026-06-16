//! 内网测速（iperf 风格的 TCP 吞吐测试）。
//!
//! 两台均运行本工具的机器：一端设为「服务端(接收)」先启动监听，
//! 另一端设为「客户端(发送)」填入对端 IP 后启动，即可测得链路吞吐。
//! 纯 tokio TCP，跨平台。（对端自动发现留作后续增强，当前手动填 IP。）

mod proto;

use super::{config_field_item, FocusArea};
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::session::LanSpeedPersist;
use crate::ui::mru::MruState;
use crate::ui::theme;
use crate::utils::format::{format_bytes, format_speed_dual};
use crate::utils::i18n::I18n;
use crate::utils::net;
use crate::utils::textinput::{filter_host, TextInput};
use crossterm::event::{KeyCode, KeyEvent};
use proto::{
    avg_bytes_per_sec, run_client, run_server, Direction, Flow, LanEvent, Proto, TestSpec,
    TestSummary, DEFAULT_PORT,
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Server,
    Client,
}

#[derive(Debug, Clone)]
struct LanConfig {
    peer: TextInput,
    port: TextInput,
    duration: TextInput, // 秒
    streams: TextInput,
    payload: TextInput, // 字节（TCP 发送缓冲）
    rate: TextInput,    // 目标速率 Mbps（0 = 不限速）
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            peer: TextInput::new(),
            port: TextInput::with_text(&DEFAULT_PORT.to_string()),
            duration: TextInput::with_text("10"),
            streams: TextInput::with_text("1"),
            payload: TextInput::with_text("65536"),
            rate: TextInput::with_text("0"),
        }
    }
}

/// 配置面板可见字段（顺序即显示顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Mode,
    Port,
    Direction,
    Peer,
    Duration,
    Streams,
    Payload,
}

pub struct LanSpeedTool {
    mode: Mode,
    direction: Direction,
    proto: Proto,
    config: LanConfig,
    config_state: ListState,
    running: bool,
    error_key: Option<String>,
    status_key: Option<String>,

    // 双路实时统计（双向时 Tx/Rx 各一份）
    tx_bps: u64,
    rx_bps: u64,
    tx_total: u64,
    rx_total: u64,
    peak_tx: u64,
    peak_rx: u64,
    tx_history: VecDeque<u64>,
    rx_history: VecDeque<u64>,
    summary: Option<TestSummary>,
    elapsed_ms: u64,

    local_ip: String,

    tx: mpsc::Sender<LanEvent>,
    rx: mpsc::Receiver<LanEvent>,
    abort_flag: Arc<Mutex<bool>>,

    history: Rc<RefCell<HistoryStore>>,
    mru: MruState,
}

impl LanSpeedTool {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(64);
        Self {
            mode: Mode::Server,
            direction: Direction::Up,
            proto: Proto::Tcp,
            config: LanConfig::default(),
            config_state,
            running: false,
            error_key: None,
            status_key: None,
            tx_bps: 0,
            rx_bps: 0,
            tx_total: 0,
            rx_total: 0,
            peak_tx: 0,
            peak_rx: 0,
            tx_history: VecDeque::with_capacity(100),
            rx_history: VecDeque::with_capacity(100),
            summary: None,
            elapsed_ms: 0,
            local_ip: local_ipv4().unwrap_or_else(|| "-".to_string()),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
            history,
            mru: MruState::default(),
        }
    }

    /// 当前可见字段序列：服务端只需端口；客户端展开全部参数。
    fn active_fields(&self) -> Vec<Field> {
        let mut v = vec![Field::Mode, Field::Port];
        if self.mode == Mode::Client {
            v.push(Field::Direction);
            v.push(Field::Peer);
            v.push(Field::Duration);
            v.push(Field::Streams);
            v.push(Field::Payload);
        }
        v
    }

    fn field_input_mut(&mut self, field: Field) -> Option<&mut TextInput> {
        match field {
            Field::Peer => Some(&mut self.config.peer),
            Field::Port => Some(&mut self.config.port),
            Field::Duration => Some(&mut self.config.duration),
            Field::Streams => Some(&mut self.config.streams),
            Field::Payload => Some(&mut self.config.payload),
            Field::Mode | Field::Direction => None,
        }
    }

    /// 导出可持久化参数。
    pub fn export_persist(&self) -> LanSpeedPersist {
        LanSpeedPersist {
            mode: match self.mode {
                Mode::Server => "server".to_string(),
                Mode::Client => "client".to_string(),
            },
            peer: self.config.peer.value(),
            port: self.config.port.value(),
            proto: match self.proto {
                Proto::Tcp => "tcp".to_string(),
                Proto::Udp => "udp".to_string(),
            },
            direction: match self.direction {
                Direction::Up => "up".to_string(),
                Direction::Down => "down".to_string(),
                Direction::Bidir => "bidir".to_string(),
            },
            duration: self.config.duration.value(),
            streams: self.config.streams.value(),
            payload: self.config.payload.value(),
            rate: self.config.rate.value(),
        }
    }

    /// 回灌持久化参数。
    pub fn apply_persist(&mut self, p: &LanSpeedPersist) {
        self.mode = if p.mode == "client" { Mode::Client } else { Mode::Server };
        self.proto = if p.proto == "udp" { Proto::Udp } else { Proto::Tcp };
        self.direction = match p.direction.as_str() {
            "down" => Direction::Down,
            "bidir" => Direction::Bidir,
            _ => Direction::Up,
        };
        self.config.peer = TextInput::with_text(&p.peer);
        self.config.port = TextInput::with_text(&p.port);
        self.config.duration = TextInput::with_text(&p.duration);
        self.config.streams = TextInput::with_text(&p.streams);
        self.config.payload = TextInput::with_text(&p.payload);
        self.config.rate = TextInput::with_text(&p.rate);
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LanEvent::Status(key) => self.status_key = Some(key),
                LanEvent::Progress {
                    flow,
                    total_bytes,
                    elapsed_ms,
                    inst_bps,
                } => {
                    self.elapsed_ms = elapsed_ms;
                    match flow {
                        Flow::Tx => {
                            self.tx_bps = inst_bps;
                            self.tx_total = total_bytes;
                            self.peak_tx = self.peak_tx.max(inst_bps);
                            push_cap(&mut self.tx_history, inst_bps, 100);
                        }
                        Flow::Rx => {
                            self.rx_bps = inst_bps;
                            self.rx_total = total_bytes;
                            self.peak_rx = self.peak_rx.max(inst_bps);
                            push_cap(&mut self.rx_history, inst_bps, 100);
                        }
                    }
                }
                LanEvent::Summary(s) => {
                    self.elapsed_ms = s.elapsed_ms;
                    self.summary = Some(s);
                }
                LanEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                    self.tx_bps = 0;
                    self.rx_bps = 0;
                }
            }
        }
        if self.summary.is_some() && self.status_key.as_deref() == Some("diag_lan_done") {
            self.running = false;
            self.tx_bps = 0;
            self.rx_bps = 0;
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>, focus: FocusArea) {
        match focus {
            FocusArea::Main => {
                if action == Some(Action::Toggle) {
                    if self.running {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
            }
            FocusArea::Config => self.handle_config_key(key, action),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, action: Option<Action>) {
        if self.running {
            return;
        }
        let fields = self.active_fields();
        let idx = self.config_state.selected().unwrap_or(0).min(fields.len() - 1);
        let field = fields[idx];

        match field {
            Field::Mode => {
                if matches!(action, Some(Action::Left) | Some(Action::Right)) {
                    self.mode = match self.mode {
                        Mode::Server => Mode::Client,
                        Mode::Client => Mode::Server,
                    };
                    let n = self.active_fields().len();
                    if idx >= n {
                        self.config_state.select(Some(n - 1));
                    }
                    return;
                }
            }
            Field::Direction => {
                if matches!(action, Some(Action::Left) | Some(Action::Right)) {
                    self.direction = match (self.direction, action) {
                        (Direction::Up, Some(Action::Right)) => Direction::Down,
                        (Direction::Down, Some(Action::Right)) => Direction::Bidir,
                        (Direction::Bidir, Some(Action::Right)) => Direction::Up,
                        (Direction::Up, _) => Direction::Bidir,
                        (Direction::Down, _) => Direction::Up,
                        (Direction::Bidir, _) => Direction::Down,
                    };
                    return;
                }
            }
            Field::Peer => {
                if crate::ui::mru::handle_mru_key(
                    &mut self.config.peer,
                    &mut self.mru,
                    &self.history.borrow().targets,
                    key,
                    action,
                    self.running,
                ) {
                    return;
                }
                let too_long =
                    matches!(key.code, KeyCode::Char(_)) && self.config.peer.len() >= 64;
                if !too_long && self.config.peer.handle_key(key.code, filter_host) {
                    return;
                }
            }
            _ => {
                if let Some(input) = self.field_input_mut(field) {
                    let too_long = matches!(key.code, KeyCode::Char(_)) && input.len() >= 16;
                    if !too_long && input.handle_key(key.code, |c| c.is_ascii_digit()) {
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

    fn next_config(&mut self) {
        let n = self.active_fields().len();
        let i = self.config_state.selected().map(|i| (i + 1) % n).unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let n = self.active_fields().len();
        let i = self
            .config_state
            .selected()
            .map(|i| if i == 0 { n - 1 } else { i - 1 })
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn reset_stats(&mut self) {
        self.tx_bps = 0;
        self.rx_bps = 0;
        self.tx_total = 0;
        self.rx_total = 0;
        self.peak_tx = 0;
        self.peak_rx = 0;
        self.tx_history.clear();
        self.rx_history.clear();
        self.summary = None;
        self.elapsed_ms = 0;
    }

    fn start(&mut self) {
        let port: u16 = self.config.port.value().parse().unwrap_or(DEFAULT_PORT);
        if port == 0 {
            self.error_key = Some("diag_lan_err".to_string());
            return;
        }

        self.running = true;
        self.error_key = None;
        self.status_key = None;
        self.reset_stats();
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();

        match self.mode {
            Mode::Server => {
                tokio::spawn(async move { run_server(port, tx, abort).await });
            }
            Mode::Client => {
                let peer = self.config.peer.value().trim().to_string();
                if peer.is_empty() {
                    self.error_key = Some("diag_lan_err".to_string());
                    self.running = false;
                    return;
                }
                self.history.borrow_mut().targets.record(&peer);

                let duration_ms = self
                    .config
                    .duration
                    .value()
                    .parse::<u64>()
                    .unwrap_or(10)
                    .clamp(1, 600)
                    * 1000;
                let streams = self
                    .config
                    .streams
                    .value()
                    .parse::<u16>()
                    .unwrap_or(1)
                    .clamp(1, 32);
                let payload_size = self
                    .config
                    .payload
                    .value()
                    .parse::<u32>()
                    .unwrap_or(65536)
                    .clamp(1024, 1_048_576);

                let spec = TestSpec {
                    proto: Proto::Tcp,
                    direction: self.direction,
                    duration_ms,
                    streams,
                    rate_mbps: 0,
                    payload_size,
                };
                tokio::spawn(async move { run_client(peer, port, spec, tx, abort).await });
            }
        }
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

        // MRU 历史下拉：仅在配置栏聚焦时有效；失焦则关闭，避免下拉悬留。
        if is_focused && active_focus == FocusArea::Config {
            if self.mru.open {
                let entries: Vec<String> = self.history.borrow().targets.entries().to_vec();
                crate::ui::mru::draw_mru_popup(f, config_area, &entries, self.mru.sel, i18n);
            }
        } else {
            self.mru.open = false;
        }
    }

    fn draw_main(
        &self,
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

        let bidir = self.mode == Mode::Client && self.direction == Direction::Bidir;
        let chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(if bidir { 2 } else { 1 }),
                Constraint::Min(3),
                Constraint::Length(5),
                Constraint::Length(1),
            ])
            .split(inner);

        let port_str = self.config.port.value();
        let endpoint = match self.mode {
            Mode::Server => format!("{}: {}:{}", i18n.t("diag_lan_localip"), self.local_ip, port_str),
            Mode::Client => {
                let peer = self.config.peer.value();
                let peer_disp = if peer.is_empty() { "-".to_string() } else { peer };
                format!("{}: {}:{}", i18n.t("diag_lan_peer"), peer_disp, port_str)
            }
        };
        f.render_widget(
            Paragraph::new(endpoint).style(Style::default().fg(theme::COLOR_SECONDARY)),
            chunks[0],
        );

        let has_tx = self.tx_total > 0 || self.tx_bps > 0;
        let has_rx = self.rx_total > 0 || self.rx_bps > 0;
        let mut tput_lines = Vec::new();
        if bidir || has_tx {
            tput_lines.push(Line::from(vec![
                Span::styled(format!("{} ", i18n.t("diag_lan_tx")), Style::default().fg(Color::Gray)),
                Span::styled(format_speed_dual(self.tx_bps), Style::default().fg(theme::COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            ]));
        }
        if bidir || (!has_tx && has_rx) || (self.mode == Mode::Server) {
            tput_lines.push(Line::from(vec![
                Span::styled(format!("{} ", i18n.t("diag_lan_rx")), Style::default().fg(Color::Gray)),
                Span::styled(format_speed_dual(self.rx_bps), Style::default().fg(theme::COLOR_PRIMARY).add_modifier(Modifier::BOLD)),
            ]));
        }
        if tput_lines.is_empty() {
            tput_lines.push(Line::from(Span::styled(
                format_speed_dual(0),
                Style::default().fg(theme::COLOR_PRIMARY),
            )));
        }
        f.render_widget(Paragraph::new(tput_lines), chunks[1]);

        let data: Vec<u64> = if has_tx {
            self.tx_history.iter().cloned().collect()
        } else {
            self.rx_history.iter().cloned().collect()
        };
        let sparkline = Sparkline::default()
            .block(Block::default().borders(Borders::TOP).title(i18n.t("diag_lan_history")))
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(sparkline, chunks[2]);

        self.draw_summary(f, chunks[3], i18n);

        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            let st = self.status_key.as_ref().map(|k| i18n.t(k)).unwrap_or_else(|| i18n.t("diag_status_running"));
            (format!("{} | {}", st, i18n.t("diag_msg_stop")), Style::default().fg(Color::Green))
        } else if self.summary.is_some() {
            (i18n.t("diag_lan_done"), Style::default().fg(theme::COLOR_SECONDARY))
        } else {
            (format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")), Style::default().fg(Color::Red))
        };
        f.render_widget(Paragraph::new(text).style(style), chunks[4]);
    }

    fn draw_summary(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let s = match &self.summary {
            Some(s) => s,
            None => {
                f.render_widget(
                    Paragraph::new(i18n.t("diag_lan_summary_wait"))
                        .style(Style::default().fg(Color::DarkGray)),
                    area,
                );
                return;
            }
        };
        let gray = Style::default().fg(Color::Gray);
        let white = Style::default().fg(Color::White);
        let mut lines = Vec::new();
        let mk = |label: String, bytes: u64, peak: u64| -> Line {
            Line::from(vec![
                Span::styled(label, gray),
                Span::styled(format!("{} ", format_speed_dual(avg_bytes_per_sec(bytes, s.elapsed_ms))), white),
                Span::styled(format!("{}: ", i18n.t("diag_lan_peak")), gray),
                Span::styled(format!("{} ", format_speed_dual(peak)), white),
                Span::styled(format!("{}: ", i18n.t("diag_lan_total")), gray),
                Span::styled(format_bytes(bytes), white),
            ])
        };
        if s.tx_bytes > 0 {
            lines.push(mk(format!("{} ", i18n.t("diag_lan_tx_avg")), s.tx_bytes, self.peak_tx));
        }
        if s.rx_bytes > 0 {
            lines.push(mk(format!("{} ", i18n.t("diag_lan_rx_avg")), s.rx_bytes, self.peak_rx));
        }
        lines.push(Line::from(vec![
            Span::styled(format!("{}: ", i18n.t("diag_lan_elapsed")), gray),
            Span::styled(format!("{:.1}s", s.elapsed_ms as f64 / 1000.0), white),
        ]));
        if let Some(u) = &s.udp {
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", i18n.t("diag_lan_loss")), gray),
                Span::styled(
                    format!("{:.2}%  ", u.loss_pct()),
                    Style::default().fg(if u.loss_pct() > 1.0 { Color::Red } else { Color::Green }),
                ),
                Span::styled(format!("{}: ", i18n.t("diag_lan_ooo")), gray),
                Span::styled(format!("{}  ", u.out_of_order), white),
                Span::styled(format!("{}: ", i18n.t("diag_lan_jitter")), gray),
                Span::styled(format!("{:.2} ms", u.jitter_ms), white),
            ]));
        }
        f.render_widget(Paragraph::new(lines), area);
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
        let fields = self.active_fields();
        let sel = self.config_state.selected().unwrap_or(0).min(fields.len() - 1);

        let layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(2)])
            .split(inner);

        let mut items: Vec<ListItem> = Vec::with_capacity(fields.len());
        for (i, field) in fields.iter().enumerate() {
            let is_sel = i == sel;
            let active = is_sel && is_active && !self.running;
            match field {
                Field::Mode => {
                    let val = match self.mode {
                        Mode::Server => i18n.t("diag_lan_server"),
                        Mode::Client => i18n.t("diag_lan_client"),
                    };
                    items.push(self.toggle_item(&i18n.t("diag_lan_mode"), &val, is_sel, is_active, i18n));
                }
                Field::Direction => {
                    let val = match self.direction {
                        Direction::Up => i18n.t("diag_lan_dir_up"),
                        Direction::Down => i18n.t("diag_lan_dir_down"),
                        Direction::Bidir => i18n.t("diag_lan_dir_bidir"),
                    };
                    items.push(self.toggle_item(&i18n.t("diag_lan_direction"), &val, is_sel, is_active, i18n));
                }
                Field::Peer => {
                    let val_base = if is_sel && is_active {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let marker = if is_sel { ">> " } else { "   " };
                    let mut spans = vec![Span::styled(marker.to_string(), val_base)];
                    spans.extend(crate::ui::mru::mru_ghost_spans(
                        &self.config.peer,
                        &self.history.borrow().targets,
                        active,
                        val_base,
                    ));
                    if active {
                        spans.push(Span::styled(
                            format!("  ({})", i18n.t("diag_hint_input")),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                    let label_line = Line::from(Span::styled(
                        format!("{}:", i18n.t("diag_lan_peer")),
                        Style::default().fg(if is_sel { Color::Yellow } else { Color::Gray }),
                    ));
                    items.push(ListItem::new(vec![label_line, Line::from(spans)]));
                }
                _ => {
                    let (label, input) = match field {
                        Field::Port => (i18n.t("diag_lan_port"), &self.config.port),
                        Field::Duration => (i18n.t("diag_lan_duration"), &self.config.duration),
                        Field::Streams => (i18n.t("diag_lan_streams"), &self.config.streams),
                        Field::Payload => (i18n.t("diag_lan_payload"), &self.config.payload),
                        _ => unreachable!(),
                    };
                    let hint = if active { Some(i18n.t("diag_hint_digits")) } else { None };
                    items.push(config_field_item(&label, is_sel, is_active, input, active, hint));
                }
            }
        }
        f.render_widget(List::new(items), layout[0]);

        let hint = Paragraph::new(i18n.t("diag_lan_hint"))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(hint, layout[1]);
    }

    /// 预设切换字段（Mode/Direction）的两行渲染（标签 + 值 + ←→ 提示）。
    fn toggle_item(
        &self,
        label: &str,
        value: &str,
        is_sel: bool,
        is_active: bool,
        i18n: &I18n,
    ) -> ListItem<'static> {
        let base = if is_sel && is_active {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let marker = if is_sel { ">> " } else { "   " };
        let mut spans = vec![
            Span::styled(marker.to_string(), base),
            Span::styled(value.to_string(), base),
        ];
        if is_sel && is_active {
            spans.push(Span::styled(
                format!("  ({})", i18n.t("diag_hint_switch")),
                Style::default().fg(Color::DarkGray),
            ));
        }
        ListItem::new(vec![
            Line::from(Span::styled(
                format!("{}:", label),
                Style::default().fg(if is_sel { Color::Yellow } else { Color::Gray }),
            )),
            Line::from(spans),
        ])
    }
}

/// 把值推入定长 ring buffer。
fn push_cap(buf: &mut std::collections::VecDeque<u64>, v: u64, cap: usize) {
    if buf.len() >= cap {
        buf.pop_front();
    }
    buf.push_back(v);
}

/// 取一个活跃物理接口的 IPv4，用于服务端显示监听地址。
fn local_ipv4() -> Option<String> {
    let interfaces = net::get_interfaces();
    interfaces
        .iter()
        .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
        .and_then(|i| i.ipv4.first().cloned())
}
