//! 内网测速（iperf 风格的 TCP 吞吐测试）。
//!
//! 两台均运行本工具的机器：一端设为「服务端(接收)」先启动监听，
//! 另一端设为「客户端(发送)」填入对端 IP 后启动，即可测得链路吞吐。
//! 纯 tokio TCP，跨平台。（对端自动发现留作后续增强，当前手动填 IP。）

use super::FocusArea;
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::format::{format_bytes, format_speed_dual};
use crate::utils::i18n::I18n;
use crate::utils::net;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

const DEFAULT_PORT: u16 = 50505;
const CLIENT_DURATION_MS: u64 = 10_000;
const BUF_SIZE: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Server,
    Client,
}

#[derive(Debug)]
enum LanEvent {
    /// 状态变化（i18n 键）
    Status(String),
    Progress {
        total_bytes: u64,
        elapsed_ms: u64,
        inst_bps: u64,
    },
    Done,
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone)]
struct LanConfig {
    peer: String,
    port: String,
}

impl Default for LanConfig {
    fn default() -> Self {
        Self {
            peer: String::new(),
            port: DEFAULT_PORT.to_string(),
        }
    }
}

pub struct LanSpeedTool {
    mode: Mode,
    config: LanConfig,
    config_state: ListState,
    running: bool,
    error_key: Option<String>,
    status_key: Option<String>,

    total_bytes: u64,
    elapsed_ms: u64,
    current_bps: u64,
    peak_bps: u64,
    history: VecDeque<u64>,

    local_ip: String,

    tx: mpsc::Sender<LanEvent>,
    rx: mpsc::Receiver<LanEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl LanSpeedTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(64);
        Self {
            mode: Mode::Server,
            config: LanConfig::default(),
            config_state,
            running: false,
            error_key: None,
            status_key: None,
            total_bytes: 0,
            elapsed_ms: 0,
            current_bps: 0,
            peak_bps: 0,
            history: VecDeque::with_capacity(100),
            local_ip: local_ipv4().unwrap_or_else(|| "-".to_string()),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LanEvent::Status(key) => self.status_key = Some(key),
                LanEvent::Progress {
                    total_bytes,
                    elapsed_ms,
                    inst_bps,
                } => {
                    self.total_bytes = total_bytes;
                    self.elapsed_ms = elapsed_ms;
                    self.current_bps = inst_bps;
                    self.peak_bps = self.peak_bps.max(inst_bps);
                    if self.history.len() >= 100 {
                        self.history.pop_front();
                    }
                    self.history.push_back(inst_bps);
                }
                LanEvent::Done => {
                    self.running = false;
                    self.current_bps = 0;
                }
                LanEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                    self.current_bps = 0;
                }
            }
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
        let idx = self.config_state.selected().unwrap_or(0);

        // idx 0 = 模式（Left/Right 切换）；idx 1 = 对端 IP（文本）；idx 2 = 端口（数字）
        if idx == 0 {
            if matches!(action, Some(Action::Left) | Some(Action::Right)) {
                self.mode = match self.mode {
                    Mode::Server => Mode::Client,
                    Mode::Client => Mode::Server,
                };
                return;
            }
        } else {
            match key.code {
                KeyCode::Backspace => {
                    self.field_mut(idx).pop();
                    return;
                }
                KeyCode::Char(c) => {
                    let accept = if idx == 1 {
                        c.is_ascii() && !c.is_control() && c != ' '
                    } else {
                        c.is_ascii_digit()
                    };
                    if accept {
                        let f = self.field_mut(idx);
                        if f.len() < 64 {
                            f.push(c);
                        }
                        return;
                    }
                }
                _ => {}
            }
        }

        match action {
            Some(Action::Down) => self.next_config(),
            Some(Action::Up) => self.prev_config(),
            _ => {}
        }
    }

    fn field_mut(&mut self, idx: usize) -> &mut String {
        match idx {
            1 => &mut self.config.peer,
            _ => &mut self.config.port,
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

    fn reset_stats(&mut self) {
        self.total_bytes = 0;
        self.elapsed_ms = 0;
        self.current_bps = 0;
        self.peak_bps = 0;
        self.history.clear();
    }

    fn start(&mut self) {
        let port: u16 = self.config.port.parse().unwrap_or(DEFAULT_PORT);
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
        let mode = self.mode;
        let peer = self.config.peer.trim().to_string();

        tokio::spawn(async move {
            match mode {
                Mode::Server => run_server(port, tx, abort).await,
                Mode::Client => run_client(peer, port, tx, abort).await,
            }
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

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // local ip (server) / peer (client)
                Constraint::Length(2), // throughput
                Constraint::Length(1), // total / elapsed
                Constraint::Min(3),    // sparkline
                Constraint::Length(1), // status
            ])
            .split(inner);

        // 端点信息
        let endpoint = match self.mode {
            Mode::Server => format!(
                "{}: {}:{}",
                i18n.t("diag_lan_localip"),
                self.local_ip,
                self.config.port
            ),
            Mode::Client => format!(
                "{}: {}:{}",
                i18n.t("diag_lan_peer"),
                if self.config.peer.is_empty() {
                    "-"
                } else {
                    &self.config.peer
                },
                self.config.port
            ),
        };
        f.render_widget(
            Paragraph::new(endpoint).style(Style::default().fg(theme::COLOR_SECONDARY)),
            chunks[0],
        );

        // 吞吐量
        let tput = Line::from(vec![
            Span::styled(
                format!("{}  ", i18n.t("diag_lan_throughput")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format_speed_dual(self.current_bps),
                Style::default()
                    .fg(theme::COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(tput), chunks[1]);

        // 传输量 / 用时
        let line = Line::from(vec![
            Span::styled(
                format!("{}: ", i18n.t("diag_lan_total")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:<16}", format_bytes(self.total_bytes)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{}: ", i18n.t("diag_lan_elapsed")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:.1}s", self.elapsed_ms as f64 / 1000.0),
                Style::default().fg(Color::White),
            ),
        ]);
        f.render_widget(Paragraph::new(line), chunks[2]);

        // 吞吐曲线
        let data: Vec<u64> = self.history.iter().cloned().collect();
        let sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(i18n.t("diag_lan_history")),
            )
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(sparkline, chunks[3]);

        // 状态
        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            let st = self
                .status_key
                .as_ref()
                .map(|k| i18n.t(k))
                .unwrap_or_else(|| i18n.t("diag_status_running"));
            (
                format!("{} | {}", st, i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else if self.total_bytes > 0 {
            (i18n.t("diag_lan_done"), Style::default().fg(theme::COLOR_SECONDARY))
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(text).style(style), chunks[4]);
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

        let mode_str = match self.mode {
            Mode::Server => i18n.t("diag_lan_server"),
            Mode::Client => i18n.t("diag_lan_client"),
        };
        let items = [
            (i18n.t("diag_lan_mode"), mode_str),
            (i18n.t("diag_lan_peer"), self.config.peer.clone()),
            (i18n.t("diag_lan_port"), self.config.port.clone()),
        ];
        let list_items: Vec<ListItem> = items
            .iter()
            .map(|(k, v)| ListItem::new(format!("{}:\n  > {}", k, v)))
            .collect();

        let is_active = is_focused && active_focus == FocusArea::Config;
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(2)])
            .split(inner);

        let list = List::new(list_items)
            .highlight_style(if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol(">> ");
        f.render_stateful_widget(list, layout[0], &mut self.config_state);

        let hint = Paragraph::new(i18n.t("diag_lan_hint"))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(ratatui::widgets::Wrap { trim: true });
        f.render_widget(hint, layout[1]);
    }
}

/// 服务端：监听端口，接受一个连接并统计接收吞吐。
async fn run_server(port: u16, tx: mpsc::Sender<LanEvent>, abort: Arc<Mutex<bool>>) {
    use tokio::net::TcpListener;

    let listener = match TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(_) => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_listening".into()))
        .await;

    // 等待连接（可中断）
    let mut stream = loop {
        if *abort.lock().unwrap() {
            return;
        }
        match tokio::time::timeout(Duration::from_millis(500), listener.accept()).await {
            Ok(Ok((s, _))) => break s,
            Ok(Err(_)) => {
                let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
                return;
            }
            Err(_) => continue, // 超时，轮询 abort
        }
    };

    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connected".into()))
        .await;

    let mut buf = vec![0u8; BUF_SIZE];
    let start = Instant::now();
    let mut last = start;
    let mut last_bytes = 0u64;
    let mut total = 0u64;

    loop {
        if *abort.lock().unwrap() {
            break;
        }
        match tokio::time::timeout(Duration::from_millis(500), stream.read(&mut buf)).await {
            Ok(Ok(0)) => break, // 对端关闭
            Ok(Ok(n)) => {
                total += n as u64;
                emit_progress(&tx, start, &mut last, &mut last_bytes, total).await;
            }
            Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }

    let _ = tx.send(LanEvent::Done).await;
}

/// 客户端：连接对端并持续发送，统计发送吞吐，约 10 秒后结束。
async fn run_client(peer: String, port: u16, tx: mpsc::Sender<LanEvent>, abort: Arc<Mutex<bool>>) {
    use tokio::net::TcpStream;

    if peer.is_empty() {
        let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
        return;
    }

    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connecting".into()))
        .await;

    let mut stream = match TcpStream::connect((peer.as_str(), port)).await {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connected".into()))
        .await;

    let buf = vec![0u8; BUF_SIZE];
    let start = Instant::now();
    let mut last = start;
    let mut last_bytes = 0u64;
    let mut total = 0u64;

    loop {
        if *abort.lock().unwrap() {
            break;
        }
        match stream.write_all(&buf).await {
            Ok(_) => {
                total += buf.len() as u64;
                emit_progress(&tx, start, &mut last, &mut last_bytes, total).await;
                if start.elapsed().as_millis() as u64 >= CLIENT_DURATION_MS {
                    break;
                }
            }
            Err(_) => break,
        }
    }

    let _ = stream.shutdown().await;
    let _ = tx.send(LanEvent::Done).await;
}

/// 每累计 ≥250ms 发一次吞吐采样。
async fn emit_progress(
    tx: &mpsc::Sender<LanEvent>,
    start: Instant,
    last: &mut Instant,
    last_bytes: &mut u64,
    total: u64,
) {
    let now = Instant::now();
    let since = now.duration_since(*last).as_secs_f64();
    if since >= 0.25 {
        let inst = ((total - *last_bytes) as f64 / since) as u64;
        let elapsed = now.duration_since(start).as_millis() as u64;
        let _ = tx
            .send(LanEvent::Progress {
                total_bytes: total,
                elapsed_ms: elapsed,
                inst_bps: inst,
            })
            .await;
        *last = now;
        *last_bytes = total;
    }
}

/// 取一个活跃物理接口的 IPv4，用于服务端显示监听地址。
fn local_ipv4() -> Option<String> {
    let interfaces = net::get_interfaces();
    interfaces
        .iter()
        .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
        .and_then(|i| i.ipv4.first().cloned())
}
