//! TCP 连接式端口扫描工具。
//!
//! 采用纯异步 `TcpStream::connect` + 超时，不依赖任何外部程序，天然跨平台。
//! 遵循与 PingTool 一致的结构：config/state + mpsc 回传 + abort flag。

use super::FocusArea;
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crossterm::event::{KeyCode, KeyEvent};
use futures::{stream, StreamExt};
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    },
};
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// 扫描事件：发现开放端口，或目标解析失败。
#[derive(Debug)]
enum PortScanEvent {
    Open(u16),
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone)]
struct PortScanConfig {
    /// 这些字段以字符串保存以便就地编辑，启动时再解析校验。
    target: String,
    start_port: String,
    end_port: String,
    timeout_ms: String,
}

impl Default for PortScanConfig {
    fn default() -> Self {
        Self {
            target: "127.0.0.1".to_string(),
            start_port: "1".to_string(),
            end_port: "1024".to_string(),
            timeout_ms: "300".to_string(),
        }
    }
}

pub struct PortScanTool {
    config: PortScanConfig,
    config_state: ListState,
    running: bool,
    error_key: Option<String>,

    open_ports: Vec<u16>,
    result_state: TableState,

    scanned: Arc<AtomicU64>,
    total: u64,

    tx: mpsc::Sender<PortScanEvent>,
    rx: mpsc::Receiver<PortScanEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl PortScanTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(256);

        Self {
            config: PortScanConfig::default(),
            config_state,
            running: false,
            error_key: None,
            open_ports: Vec::new(),
            result_state: TableState::default(),
            scanned: Arc::new(AtomicU64::new(0)),
            total: 0,
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                PortScanEvent::Open(port) => {
                    if let Err(pos) = self.open_ports.binary_search(&port) {
                        self.open_ports.insert(pos, port);
                    }
                    if self.result_state.selected().is_none() {
                        self.result_state.select(Some(0));
                    }
                }
                PortScanEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                }
            }
        }

        // 扫描完成检测
        if self.running && self.total > 0 && self.scanned.load(Ordering::Relaxed) >= self.total {
            self.running = false;
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>, focus: FocusArea, concurrency: usize) {
        match focus {
            FocusArea::Main => {
                if action == Some(Action::Toggle) {
                    if self.running {
                        self.stop();
                    } else {
                        self.start(concurrency);
                    }
                } else if !self.open_ports.is_empty() {
                    match action {
                        Some(Action::Down) => self.next_result(),
                        Some(Action::Up) => self.prev_result(),
                        _ => {}
                    }
                }
            }
            FocusArea::Config => self.handle_config_key(key, action),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, action: Option<Action>) {
        // 运行中不允许改配置
        if !self.running {
            if let Some(idx) = self.config_state.selected() {
                match key.code {
                    KeyCode::Backspace => {
                        self.field_mut(idx).pop();
                        return;
                    }
                    KeyCode::Char(c) => {
                        // 目标字段接受常规可见字符；端口/超时仅接受数字
                        let accept = if idx == 0 {
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
        }

        match action {
            Some(Action::Down) => self.next_config(),
            Some(Action::Up) => self.prev_config(),
            _ => {}
        }
    }

    fn field_mut(&mut self, idx: usize) -> &mut String {
        match idx {
            0 => &mut self.config.target,
            1 => &mut self.config.start_port,
            2 => &mut self.config.end_port,
            _ => &mut self.config.timeout_ms,
        }
    }

    fn next_config(&mut self) {
        let i = self.config_state.selected().map(|i| (i + 1) % 4).unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| if i == 0 { 3 } else { i - 1 })
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn next_result(&mut self) {
        if self.open_ports.is_empty() {
            return;
        }
        let i = self
            .result_state
            .selected()
            .map(|i| (i + 1) % self.open_ports.len())
            .unwrap_or(0);
        self.result_state.select(Some(i));
    }

    fn prev_result(&mut self) {
        if self.open_ports.is_empty() {
            return;
        }
        let n = self.open_ports.len();
        let i = self
            .result_state
            .selected()
            .map(|i| if i == 0 { n - 1 } else { i - 1 })
            .unwrap_or(0);
        self.result_state.select(Some(i));
    }

    fn start(&mut self, concurrency: usize) {
        // 解析与校验配置
        let start: u32 = self.config.start_port.parse().unwrap_or(0);
        let end: u32 = self.config.end_port.parse().unwrap_or(0);
        let timeout_ms: u64 = self.config.timeout_ms.parse().unwrap_or(300).clamp(20, 10000);

        if self.config.target.trim().is_empty()
            || start == 0
            || end == 0
            || start > 65535
            || end > 65535
            || start > end
        {
            self.error_key = Some("diag_port_err_target".to_string());
            return;
        }

        self.running = true;
        self.error_key = None;
        self.open_ports.clear();
        self.result_state.select(None);
        self.scanned.store(0, Ordering::Relaxed);
        self.total = (end - start + 1) as u64;

        *self.abort_flag.lock().unwrap() = false;
        let abort = self.abort_flag.clone();
        let tx = self.tx.clone();
        let scanned = self.scanned.clone();
        let target = self.config.target.trim().to_string();
        let concurrency = concurrency.clamp(1, 1024);

        tokio::spawn(async move {
            // 解析目标为 IP（支持域名异步解析）
            let ip: IpAddr = match target.parse::<IpAddr>() {
                Ok(ip) => ip,
                Err(_) => match tokio::net::lookup_host((target.as_str(), 0u16)).await {
                    Ok(mut it) => match it.next() {
                        Some(sa) => sa.ip(),
                        None => {
                            let _ = tx
                                .send(PortScanEvent::Error("diag_port_err_target".into()))
                                .await;
                            return;
                        }
                    },
                    Err(_) => {
                        let _ = tx
                            .send(PortScanEvent::Error("diag_port_err_target".into()))
                            .await;
                        return;
                    }
                },
            };

            let ports: Vec<u16> = (start as u16..=end as u16).collect();
            let scan = stream::iter(ports)
                .map(|port| {
                    let tx = tx.clone();
                    let scanned = scanned.clone();
                    let abort = abort.clone();
                    async move {
                        if *abort.lock().unwrap() {
                            return;
                        }
                        let addr = SocketAddr::new(ip, port);
                        let fut = tokio::net::TcpStream::connect(addr);
                        if let Ok(Ok(_stream)) =
                            tokio::time::timeout(Duration::from_millis(timeout_ms), fut).await
                        {
                            let _ = tx.send(PortScanEvent::Open(port)).await;
                        }
                        scanned.fetch_add(1, Ordering::Relaxed);
                    }
                })
                .buffer_unordered(concurrency);

            scan.collect::<Vec<()>>().await;
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
            .constraints([
                Constraint::Length(2), // stats
                Constraint::Min(3),    // open port list
                Constraint::Length(1), // progress
                Constraint::Length(1), // status
            ])
            .split(inner);

        // 1. 统计行
        let scanned = self.scanned.load(Ordering::Relaxed);
        let stat_line = Line::from(vec![
            Span::styled(
                format!("{}: ", i18n.t("diag_port_stat_open")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:<6}", self.open_ports.len()),
                Style::default()
                    .fg(theme::COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}: ", i18n.t("diag_port_stat_scanned")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{} / {}", scanned, self.total),
                Style::default().fg(theme::COLOR_SECONDARY),
            ),
        ]);
        f.render_widget(Paragraph::new(stat_line), chunks[0]);

        // 2. 开放端口表（端口 + 知名服务名）
        let header = Row::new(vec![
            Cell::from(i18n.t("diag_port_col_port")).style(Style::default().fg(Color::Gray)),
            Cell::from(i18n.t("diag_port_col_service")).style(Style::default().fg(Color::Gray)),
        ])
        .height(1);

        let rows = self.open_ports.iter().map(|p| {
            Row::new(vec![
                Cell::from(format!("{}", p)).style(Style::default().fg(Color::White)),
                Cell::from(well_known_service(*p).unwrap_or("-"))
                    .style(Style::default().fg(theme::COLOR_SECONDARY)),
            ])
        });
        let table = Table::new(rows, [Constraint::Length(10), Constraint::Min(0)])
            .header(header)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(i18n.t("diag_port_open_title")),
            )
            .highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("> ");
        f.render_stateful_widget(table, chunks[1], &mut self.result_state);

        // 3. 进度条
        let ratio = if self.total > 0 {
            (scanned as f64 / self.total as f64).min(1.0)
        } else {
            0.0
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(theme::COLOR_SECONDARY).bg(Color::DarkGray))
            .ratio(ratio)
            .label(format!("{:.0}%", ratio * 100.0));
        f.render_widget(gauge, chunks[2]);

        // 4. 状态行
        let (status_text, status_style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!("{} | {}", i18n.t("scan_status_scanning"), i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(
            Paragraph::new(status_text).style(status_style),
            chunks[3],
        );
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

        let items = [
            (i18n.t("diag_port_target"), self.config.target.clone()),
            (i18n.t("diag_port_start"), self.config.start_port.clone()),
            (i18n.t("diag_port_end"), self.config.end_port.clone()),
            (i18n.t("diag_port_timeout"), self.config.timeout_ms.clone()),
        ];
        let list_items: Vec<ListItem> = items
            .iter()
            .map(|(k, v)| ListItem::new(format!("{}:\n  > {}", k, v)))
            .collect();

        let is_active = is_focused && active_focus == FocusArea::Config;
        let list = List::new(list_items)
            .highlight_style(if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol(">> ");
        f.render_stateful_widget(list, inner, &mut self.config_state);
    }
}

/// 少量知名端口的服务名映射（用于结果可读性）。
fn well_known_service(port: u16) -> Option<&'static str> {
    let s = match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 => "SMTP",
        53 => "DNS",
        67 | 68 => "DHCP",
        80 => "HTTP",
        110 => "POP3",
        123 => "NTP",
        135 => "MSRPC",
        139 => "NetBIOS",
        143 => "IMAP",
        389 => "LDAP",
        443 => "HTTPS",
        445 => "SMB",
        587 => "SMTP/TLS",
        993 => "IMAPS",
        995 => "POP3S",
        1433 => "MSSQL",
        1521 => "Oracle",
        3306 => "MySQL",
        3389 => "RDP",
        5432 => "PostgreSQL",
        5900 => "VNC",
        6379 => "Redis",
        8080 => "HTTP-Alt",
        8443 => "HTTPS-Alt",
        9200 => "Elasticsearch",
        27017 => "MongoDB",
        _ => return None,
    };
    Some(s)
}
