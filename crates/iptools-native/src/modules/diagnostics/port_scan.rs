//! TCP 连接式端口扫描工具。
//!
//! 采用纯异步 `TcpStream::connect` + 超时，不依赖任何外部程序，天然跨平台。
//! 任务由 RuntimeSupervisor 统一拥有，通过有界 RuntimeEvent 通道回传。

use super::FocusArea;
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::session::PortScanPersist;
use crate::ui::mru::MruState;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::textinput::{TextInput, filter_host};
use crossterm::event::{KeyCode, KeyEvent};
use futures::{StreamExt, stream};
use iptools_core::{JobId, RuntimeEvent, ToolKind};
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Cell, Gauge, List, ListItem, ListState, Paragraph, Row, Table, TableState,
    },
};
use std::cell::RefCell;
use std::net::{IpAddr, SocketAddr};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::runtime::RuntimeSupervisor;

#[derive(Debug, Clone)]
struct PortScanConfig {
    /// 这些字段以带光标的文本输入保存以便就地编辑，启动时再解析校验。
    target: TextInput,
    start_port: TextInput,
    end_port: TextInput,
    timeout_ms: TextInput,
}

impl Default for PortScanConfig {
    fn default() -> Self {
        Self {
            target: TextInput::with_text("127.0.0.1"),
            start_port: TextInput::with_text("1"),
            end_port: TextInput::with_text("1024"),
            timeout_ms: TextInput::with_text("300"),
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

    scanned: u64,
    total: u64,
    generation: u64,
    job: Option<JobId>,
    runtime: RuntimeSupervisor,

    history: Rc<RefCell<HistoryStore>>,
    mru: MruState,
}

impl PortScanTool {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        Self {
            config: PortScanConfig::default(),
            config_state,
            running: false,
            error_key: None,
            open_ports: Vec::new(),
            result_state: TableState::default(),
            scanned: 0,
            total: 0,
            generation: 0,
            job: None,
            runtime: RuntimeSupervisor::new(),
            history,
            mru: MruState::default(),
        }
    }

    /// 导出可持久化参数（按界面文本原样保存）。
    pub fn export_persist(&self) -> PortScanPersist {
        PortScanPersist {
            target: self.config.target.value(),
            start_port: self.config.start_port.value(),
            end_port: self.config.end_port.value(),
            timeout_ms: self.config.timeout_ms.value(),
        }
    }

    /// 回灌持久化参数。
    pub fn apply_persist(&mut self, p: &PortScanPersist) {
        self.config.target = TextInput::with_text(&p.target);
        self.config.start_port = TextInput::with_text(&p.start_port);
        self.config.end_port = TextInput::with_text(&p.end_port);
        self.config.timeout_ms = TextInput::with_text(&p.timeout_ms);
    }

    pub fn update(&mut self) {
        self.runtime.reap_finished();
        while let Some(event) = self.runtime.try_recv() {
            match event {
                RuntimeEvent::PortScanOpen { job, port } if self.job == Some(job) => {
                    if let Err(pos) = self.open_ports.binary_search(&port) {
                        self.open_ports.insert(pos, port);
                    }
                    if self.result_state.selected().is_none() {
                        self.result_state.select(Some(0));
                    }
                }
                RuntimeEvent::DiagnosticProgress { job, progress, .. } if self.job == Some(job) => {
                    self.scanned = self.total.saturating_mul(progress as u64) / 100;
                }
                RuntimeEvent::DiagnosticFailed { job, error } if self.job == Some(job) => {
                    self.error_key = Some(error);
                    self.running = false;
                    self.job = None;
                }
                RuntimeEvent::DiagnosticFinished { job, .. } if self.job == Some(job) => {
                    self.scanned = self.total;
                    self.running = false;
                    self.job = None;
                }
                _ => {}
            }
        }
    }

    pub fn on_key(
        &mut self,
        key: KeyEvent,
        action: Option<Action>,
        focus: FocusArea,
        concurrency: usize,
    ) {
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
        // 运行中不允许改配置。带光标编辑：目标接受主机名字符，端口/超时仅数字。
        let on_target = self.config_state.selected() == Some(0);
        if (self.mru.open || (on_target && !self.running))
            && crate::ui::mru::handle_mru_key(
                &mut self.config.target,
                &mut self.mru,
                &self.history.borrow().targets,
                key,
                action,
                self.running,
            )
        {
            return;
        }
        if !self.running
            && let Some(idx) = self.config_state.selected()
        {
            let too_long = matches!(key.code, KeyCode::Char(_)) && self.field_mut(idx).len() >= 64;
            if !too_long {
                let consumed = if idx == 0 {
                    self.field_mut(idx).handle_key(key.code, filter_host)
                } else {
                    self.field_mut(idx)
                        .handle_key(key.code, |c| c.is_ascii_digit())
                };
                if consumed {
                    return;
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
            1 => &mut self.config.start_port,
            2 => &mut self.config.end_port,
            _ => &mut self.config.timeout_ms,
        }
    }

    fn next_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| (i + 1) % 4)
            .unwrap_or(0);
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
        let start: u32 = self.config.start_port.value().parse().unwrap_or(0);
        let end: u32 = self.config.end_port.value().parse().unwrap_or(0);
        let timeout_ms: u64 = self
            .config
            .timeout_ms
            .value()
            .parse()
            .unwrap_or(300)
            .clamp(20, 10000);

        if self.config.target.value().trim().is_empty()
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
        self.scanned = 0;
        self.total = (end - start + 1) as u64;

        let target = self.config.target.value().trim().to_string();
        if !target.is_empty() {
            self.history.borrow_mut().targets.record(&target);
        }
        let concurrency = concurrency.clamp(1, 1024);
        self.generation = self.generation.saturating_add(1);
        let job = JobId {
            tool: ToolKind::PortScan,
            generation: self.generation,
        };
        self.job = Some(job);
        let total = self.total;

        self.runtime.spawn(job, move |token, events| async move {
            let _ = events.send(RuntimeEvent::DiagnosticStarted { job }).await;
            // 解析目标为 IP（支持域名异步解析）
            let resolved = async {
                match target.parse::<IpAddr>() {
                    Ok(ip) => Some(ip),
                    Err(_) => tokio::net::lookup_host((target.as_str(), 0u16))
                        .await
                        .ok()
                        .and_then(|mut it| it.next().map(|address| address.ip())),
                }
            };
            let Some(ip) = (tokio::select! {
                _ = token.cancelled() => None,
                ip = resolved => ip,
            }) else {
                let _ = events
                    .send(RuntimeEvent::DiagnosticFailed {
                        job,
                        error: "diag_port_err_target".into(),
                    })
                    .await;
                return;
            };

            let ports: Vec<u16> = (start as u16..=end as u16).collect();
            let scanned = Arc::new(AtomicU64::new(0));
            let mut scan = stream::iter(ports)
                .map(|port| {
                    let events = events.clone();
                    let scanned = Arc::clone(&scanned);
                    let token = token.clone();
                    async move {
                        if token.is_cancelled() {
                            return;
                        }
                        let addr = SocketAddr::new(ip, port);
                        let fut = tokio::net::TcpStream::connect(addr);
                        if let Ok(Ok(_stream)) =
                            tokio::time::timeout(Duration::from_millis(timeout_ms), fut).await
                        {
                            let _ = events.send(RuntimeEvent::PortScanOpen { job, port }).await;
                        }
                        scanned.fetch_add(1, Ordering::Relaxed);
                    }
                })
                .buffer_unordered(concurrency);
            let mut ticker = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = ticker.tick() => {
                        let current = scanned.load(Ordering::Relaxed);
                        let progress = current
                            .saturating_mul(100)
                            .checked_div(total)
                            .unwrap_or(0)
                            .min(100) as u8;
                        let _ = events.send(RuntimeEvent::DiagnosticProgress {
                            job,
                            progress,
                            primary: current.to_string(),
                            detail: total.to_string(),
                        }).await;
                    }
                    item = scan.next() => if item.is_none() { break },
                }
            }
            let _ = events
                .send(RuntimeEvent::DiagnosticFinished {
                    job,
                    summary: if token.is_cancelled() {
                        "cancelled"
                    } else {
                        "done"
                    }
                    .into(),
                })
                .await;
        });
    }

    fn stop(&mut self) {
        self.running = false;
        if let Some(job) = self.job {
            self.runtime.cancel(job);
        }
    }

    pub async fn shutdown(&mut self) {
        self.runtime.shutdown().await;
        self.job = None;
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
        let scanned = self.scanned;
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
            .row_highlight_style(Style::default().bg(Color::DarkGray))
            .highlight_symbol("> ");
        f.render_stateful_widget(table, chunks[1], &mut self.result_state);

        // 3. 进度条
        let ratio = if self.total > 0 {
            (scanned as f64 / self.total as f64).min(1.0)
        } else {
            0.0
        };
        let gauge = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(theme::COLOR_SECONDARY)
                    .bg(Color::DarkGray),
            )
            .ratio(ratio)
            .label(format!("{:.0}%", ratio * 100.0));
        f.render_widget(gauge, chunks[2]);

        // 4. 状态行
        let (status_text, status_style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!(
                    "{} | {}",
                    i18n.t("scan_status_scanning"),
                    i18n.t("diag_msg_stop")
                ),
                Style::default().fg(Color::Green),
            )
        } else {
            (
                format!(
                    "{} | {}",
                    i18n.t("diag_status_stopped"),
                    i18n.t("diag_msg_start")
                ),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(status_text).style(status_style), chunks[3]);
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
            i18n.t("diag_port_target"),
            i18n.t("diag_port_start"),
            i18n.t("diag_port_end"),
            i18n.t("diag_port_timeout"),
        ];
        let selected = self.config_state.selected();

        let mut list_items: Vec<ListItem> = Vec::with_capacity(4);
        for (i, label) in labels.iter().enumerate() {
            let is_sel = selected == Some(i);
            let label_line = Line::from(Span::styled(
                format!("{}:", label),
                Style::default().fg(if is_sel { Color::Yellow } else { Color::Gray }),
            ));
            let val_base = if is_sel && is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let marker = if is_sel { ">> " } else { "   " };
            let input = match i {
                0 => &self.config.target,
                1 => &self.config.start_port,
                2 => &self.config.end_port,
                _ => &self.config.timeout_ms,
            };
            let active = is_sel && is_active && !self.running;
            let mut value_spans = vec![Span::styled(marker.to_string(), val_base)];
            if i == 0 {
                // 目标字段：带灰字历史补全。
                value_spans.extend(crate::ui::mru::mru_ghost_spans(
                    &self.config.target,
                    &self.history.borrow().targets,
                    active,
                    val_base,
                ));
            } else {
                value_spans.extend(input.render_spans(active, val_base));
            }
            // 全部为「直接输入」字段；目标可含主机名，其余仅数字——提示分别标注。
            if is_sel && is_active && !self.running {
                let hint = if i == 0 {
                    i18n.t("diag_hint_input")
                } else {
                    i18n.t("diag_hint_digits")
                };
                value_spans.push(Span::styled(
                    format!("  ({})", hint),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            list_items.push(ListItem::new(vec![label_line, Line::from(value_spans)]));
        }

        f.render_widget(List::new(list_items), inner);
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
