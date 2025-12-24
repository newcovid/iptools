use super::FocusArea;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

// -----------------------------------------------------------------------------
// 数据结构
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PingConfig {
    target: String,
    interval_ms: u64,
    timeout_ms: u64,
    packet_size: u64,
}

impl Default for PingConfig {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".to_string(),
            interval_ms: 1000,
            timeout_ms: 2000,
            packet_size: 32,
        }
    }
}

#[allow(dead_code)] // 暂时允许死代码，直到 update 被 App 调用
struct PingStats {
    sent: u64,
    recv: u64,
    min_latency: Option<u64>,
    max_latency: Option<u64>,
    total_latency: u64,
    jitter_sum: u64,
    last_latency: Option<u64>,
    prev_latency: Option<u64>,
    history: VecDeque<u64>,
    logs: VecDeque<String>,
}

impl Default for PingStats {
    fn default() -> Self {
        Self {
            sent: 0,
            recv: 0,
            min_latency: None,
            max_latency: None,
            total_latency: 0,
            jitter_sum: 0,
            last_latency: None,
            prev_latency: None,
            history: VecDeque::with_capacity(100),
            logs: VecDeque::with_capacity(20),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)] // 暂时允许死代码，直到 update 被 App 调用
pub enum PingEvent {
    Result {
        seq: u64,
        latency: u64,
        ttl: u8,
        size: usize,
    },
    Timeout {
        seq: u64,
    },
    Error(String),
}

// -----------------------------------------------------------------------------
// PingTool 结构体
// -----------------------------------------------------------------------------

pub struct PingTool {
    config: PingConfig,
    stats: PingStats,
    running: bool,
    config_state: ListState,

    tx: mpsc::Sender<PingEvent>,
    rx: mpsc::Receiver<PingEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl PingTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(100);

        Self {
            config: PingConfig::default(),
            stats: PingStats::default(),
            running: false,
            config_state,
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                PingEvent::Result {
                    seq,
                    latency,
                    ttl,
                    size,
                } => {
                    self.stats.recv += 1;
                    self.stats.sent = seq + 1;

                    if let Some(min) = self.stats.min_latency {
                        if latency < min {
                            self.stats.min_latency = Some(latency);
                        }
                    } else {
                        self.stats.min_latency = Some(latency);
                    }

                    if let Some(max) = self.stats.max_latency {
                        if latency > max {
                            self.stats.max_latency = Some(latency);
                        }
                    } else {
                        self.stats.max_latency = Some(latency);
                    }

                    self.stats.total_latency += latency;

                    if let Some(prev) = self.stats.prev_latency {
                        let diff = if latency > prev {
                            latency - prev
                        } else {
                            prev - latency
                        };
                        self.stats.jitter_sum += diff;
                    }

                    self.stats.prev_latency = Some(latency);
                    self.stats.last_latency = Some(latency);

                    if self.stats.history.len() >= 100 {
                        self.stats.history.pop_front();
                    }
                    self.stats.history.push_back(latency);

                    let log = format!("Seq={} bytes={} ttl={} time={}ms", seq, size, ttl, latency);
                    self.add_log(log);
                }
                PingEvent::Timeout { seq } => {
                    self.stats.sent = seq + 1;
                    if self.stats.history.len() >= 100 {
                        self.stats.history.pop_front();
                    }
                    self.stats.history.push_back(0);
                    self.add_log(format!("Seq={} Request timed out", seq));
                }
                PingEvent::Error(msg) => {
                    self.add_log(format!("Error: {}", msg));
                    self.stop();
                }
            }
        }
    }

    fn add_log(&mut self, msg: String) {
        if self.stats.logs.len() >= 20 {
            self.stats.logs.pop_front();
        }
        self.stats.logs.push_back(msg);
    }

    // -------------------------------------------------------------------------
    // 交互逻辑
    // -------------------------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent, active_focus: FocusArea) {
        match active_focus {
            FocusArea::Main => {
                if key.code == KeyCode::Char(' ') {
                    if self.running {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
            }
            FocusArea::Config => self.handle_config_key(key),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next_config(),
            KeyCode::Up | KeyCode::Char('k') => self.prev_config(),

            KeyCode::Left | KeyCode::Char('h') => self.adjust_numeric_config(-1),
            KeyCode::Right | KeyCode::Char('l') => self.adjust_numeric_config(1),

            KeyCode::Backspace => {
                if self.config_state.selected() == Some(0) && !self.running {
                    self.config.target.pop();
                }
            }
            KeyCode::Char(c) => {
                self.edit_config_value(c);
            }
            _ => {}
        }
    }

    fn next_config(&mut self) {
        let max = 4;
        let i = match self.config_state.selected() {
            Some(i) => {
                if i >= max - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let max = 4;
        let i = match self.config_state.selected() {
            Some(i) => {
                if i == 0 {
                    max - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.config_state.select(Some(i));
    }

    fn edit_config_value(&mut self, c: char) {
        if self.running {
            return;
        }
        if let Some(idx) = self.config_state.selected() {
            if idx == 0 {
                if c.is_ascii() {
                    self.config.target.push(c);
                }
            }
        }
    }

    fn adjust_numeric_config(&mut self, dir: i64) {
        if self.running {
            return;
        }
        if let Some(idx) = self.config_state.selected() {
            match idx {
                1 => {
                    let new_val = (self.config.interval_ms as i64) + (dir * 100);
                    self.config.interval_ms = new_val.clamp(100, 10000) as u64;
                }
                2 => {
                    let new_val = (self.config.timeout_ms as i64) + (dir * 100);
                    self.config.timeout_ms = new_val.clamp(100, 10000) as u64;
                }
                3 => {
                    let new_val = (self.config.packet_size as i64) + (dir * 8);
                    self.config.packet_size = new_val.clamp(0, 65500) as u64;
                }
                _ => {}
            }
        }
    }

    // -------------------------------------------------------------------------
    // 异步任务管理
    // -------------------------------------------------------------------------

    fn start(&mut self) {
        if self.running {
            return;
        }
        self.running = true;
        self.stats = PingStats::default();
        *self.abort_flag.lock().unwrap() = false;

        let abort = self.abort_flag.clone();
        let tx = self.tx.clone();
        let target_str = self.config.target.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let ip_addr = match target_str.parse::<IpAddr>() {
                Ok(ip) => ip,
                Err(_) => match dns_lookup::lookup_host(&target_str) {
                    Ok(ips) => {
                        if let Some(first) = ips.first() {
                            *first
                        } else {
                            let _ = tx.send(PingEvent::Error("DNS No result".into())).await;
                            return;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(PingEvent::Error(format!("DNS Error: {}", e))).await;
                        return;
                    }
                },
            };

            #[cfg(target_os = "windows")]
            {
                if let IpAddr::V4(ipv4) = ip_addr {
                    run_ping_windows(ipv4, config, tx, abort).await;
                } else {
                    let _ = tx
                        .send(PingEvent::Error("Windows IPv6 ping not supported".into()))
                        .await;
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                run_ping_unix(ip_addr, config, tx, abort).await;
            }
        });
    }

    fn stop(&mut self) {
        self.running = false;
        *self.abort_flag.lock().unwrap() = true;
    }

    // -------------------------------------------------------------------------
    // 绘图逻辑
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
        // --- Draw Main Panel ---
        let main_color = if is_focused && active_focus == FocusArea::Main {
            Color::Yellow
        } else {
            Color::Gray
        };

        let main_block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_main_title"))
            .border_style(Style::default().fg(main_color));

        let main_inner = main_block.inner(main_area);
        f.render_widget(main_block, main_area);

        if !is_focused {
            let p = Paragraph::new(i18n.t("diag_msg_focus_hint"))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(p, centered_rect(main_inner, 50, 10));
        } else {
            self.draw_dashboard(f, main_inner, i18n);
        }

        // --- Draw Config Panel ---
        let conf_color = if is_focused && active_focus == FocusArea::Config {
            Color::Yellow
        } else {
            Color::Gray
        };

        let conf_block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_config_title"))
            .border_style(Style::default().fg(conf_color));

        let conf_inner = conf_block.inner(config_area);
        f.render_widget(conf_block, config_area);

        self.draw_config_list(
            f,
            conf_inner,
            i18n,
            is_focused && active_focus == FocusArea::Config,
        );
    }

    fn draw_dashboard(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(10),
                Constraint::Length(8),
                Constraint::Length(1),
            ])
            .split(area);

        // 1. Stats Grid
        let stats = &self.stats;
        let avg = if stats.recv > 0 {
            stats.total_latency / stats.recv
        } else {
            0
        };
        let loss = if stats.sent > 0 {
            ((stats.sent - stats.recv) as f64 / stats.sent as f64) * 100.0
        } else {
            0.0
        };
        let jitter = if stats.recv > 1 {
            stats.jitter_sum / (stats.recv - 1)
        } else {
            0
        };

        let row1 = vec![
            format!(
                "{}: {} ms",
                i18n.t("diag_stat_last"),
                stats.last_latency.unwrap_or(0)
            ),
            format!(
                "{}: {} ms",
                i18n.t("diag_stat_min"),
                stats.min_latency.unwrap_or(0)
            ),
            format!(
                "{}: {} ms",
                i18n.t("diag_stat_max"),
                stats.max_latency.unwrap_or(0)
            ),
        ];
        let row2 = vec![
            format!("{}: {} ms", i18n.t("diag_stat_avg"), avg),
            format!("{}: {} ms", i18n.t("diag_stat_jitter"), jitter),
            format!("{}: {:.1}%", i18n.t("diag_stat_loss"), loss),
        ];

        let styles = [
            Style::default().fg(theme::COLOR_SECONDARY),
            Style::default().fg(Color::Green),
            Style::default().fg(Color::Red),
        ];

        for (i, text) in row1.iter().enumerate() {
            let x_offset = (area.width / 3) * i as u16;
            f.render_widget(
                Paragraph::new(text.as_str()).style(styles[i % 3]),
                Rect::new(chunks[0].x + x_offset, chunks[0].y, area.width / 3, 1),
            );
        }
        for (i, text) in row2.iter().enumerate() {
            let x_offset = (area.width / 3) * i as u16;
            f.render_widget(
                Paragraph::new(text.as_str()).style(Style::default().fg(Color::White)),
                Rect::new(chunks[0].x + x_offset, chunks[0].y + 1, area.width / 3, 1),
            );
        }

        f.render_widget(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(Color::DarkGray)),
            Rect::new(chunks[0].x, chunks[0].y + 2, chunks[0].width, 1),
        );

        // 2. Sparkline
        let data: Vec<u64> = stats.history.iter().cloned().collect();
        let sparkline = Sparkline::default()
            .block(
                Block::default()
                    .title("Latency History")
                    .borders(Borders::NONE),
            )
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(sparkline, chunks[1]);

        // 3. Log
        let logs: Vec<ListItem> = stats
            .logs
            .iter()
            .rev()
            .map(|s| ListItem::new(Line::from(s.as_str())))
            .collect();
        let log_list = List::new(logs).block(
            Block::default()
                .borders(Borders::TOP)
                .title("Log Stream")
                .style(Style::default().fg(Color::Gray)),
        );
        f.render_widget(log_list, chunks[2]);

        // 4. Status Bar
        let status_text = if self.running {
            format!(
                "{} | {}",
                i18n.t("diag_status_running"),
                i18n.t("diag_msg_stop")
            )
        } else {
            format!(
                "{} | {}",
                i18n.t("diag_status_stopped"),
                i18n.t("diag_msg_start")
            )
        };
        let status_style = if self.running {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };

        f.render_widget(Paragraph::new(status_text).style(status_style), chunks[3]);
    }

    fn draw_config_list(&mut self, f: &mut Frame, area: Rect, i18n: &I18n, is_active: bool) {
        let cfg = &self.config;
        let items = vec![
            (i18n.t("diag_ping_target"), cfg.target.clone()),
            (i18n.t("diag_ping_interval"), format!("{}", cfg.interval_ms)),
            (i18n.t("diag_ping_timeout"), format!("{}", cfg.timeout_ms)),
            (i18n.t("diag_ping_size"), format!("{}", cfg.packet_size)),
        ];

        let list_items: Vec<ListItem> = items
            .iter()
            .map(|(k, v)| {
                let content = format!("{}:\n  > {}", k, v);
                ListItem::new(content)
            })
            .collect();

        let list = List::new(list_items)
            .block(Block::default().borders(Borders::NONE))
            .highlight_style(if is_active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol(">> ");

        f.render_stateful_widget(list, area, &mut self.config_state);
    }
}

fn centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let popup_layout = Layout::default()
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
        .split(popup_layout[1])[1]
}

// -----------------------------------------------------------------------------
// 平台特定实现 (Windows)
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
async fn run_ping_windows(
    target_ip: std::net::Ipv4Addr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    abort: Arc<Mutex<bool>>,
) {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY,
    };

    let ip_u32 = u32::from_le_bytes(target_ip.octets());
    const REPLY_SIZE: usize = 2048 + 65535; // 足够大的缓冲区
    let mut seq = 0;

    loop {
        if *abort.lock().unwrap() {
            break;
        }

        let payload = vec![0u8; config.packet_size as usize];
        let timeout = config.timeout_ms as u32;

        let ping_task_result = tokio::task::spawn_blocking(move || {
            let handle = unsafe { IcmpCreateFile() }.map_err(|e| e.to_string())?;

            let mut reply_buffer = vec![0u8; REPLY_SIZE];

            let ret_count = unsafe {
                IcmpSendEcho(
                    handle,
                    ip_u32,
                    payload.as_ptr() as *const c_void,
                    payload.len() as u16,
                    None,
                    reply_buffer.as_mut_ptr() as *mut c_void,
                    REPLY_SIZE as u32,
                    timeout,
                )
            };

            // 修复：处理 CloseHandle 的返回值，使用 let _ = ... 忽略
            unsafe {
                let _ = IcmpCloseHandle(handle);
            };

            Ok::<(u32, Vec<u8>, usize), String>((ret_count, reply_buffer, payload.len()))
        })
        .await;

        match ping_task_result {
            Ok(Ok((count, reply_buf, sent_size))) => {
                if count > 0 {
                    let reply = unsafe { &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY) };
                    if reply.Status == 0 {
                        let _ = tx
                            .send(PingEvent::Result {
                                seq,
                                latency: reply.RoundTripTime as u64,
                                ttl: reply.Options.Ttl,
                                size: sent_size,
                            })
                            .await;
                    } else {
                        let _ = tx.send(PingEvent::Timeout { seq }).await;
                    }
                } else {
                    let _ = tx.send(PingEvent::Timeout { seq }).await;
                }
            }
            Ok(Err(e)) => {
                let _ = tx
                    .send(PingEvent::Error(format!("Ping error: {}", e)))
                    .await;
                break;
            }
            Err(_) => {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(config.interval_ms)).await;
        seq += 1;
    }
}

#[cfg(not(target_os = "windows"))]
async fn run_ping_unix(
    target_ip: std::net::IpAddr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    abort: Arc<Mutex<bool>>,
) {
    let payload = vec![0u8; config.packet_size as usize];
    let mut pinger = match surge_ping::Pinger::new(target_ip).await {
        Ok(p) => p,
        Err(e) => {
            let msg = if e.to_string().contains("Permission") {
                "Permission denied (Root required on Unix)".to_string()
            } else {
                e.to_string()
            };
            let _ = tx.send(PingEvent::Error(msg)).await;
            return;
        }
    };

    let mut seq = 0;
    let mut interval = tokio::time::interval(Duration::from_millis(config.interval_ms));

    loop {
        if *abort.lock().unwrap() {
            break;
        }
        interval.tick().await;

        match pinger
            .ping(surge_ping::PingIdentifier(seq as u16), &payload)
            .await
        {
            Ok((_packet, duration)) => {
                let ms = duration.as_millis() as u64;
                let _ = tx
                    .send(PingEvent::Result {
                        seq,
                        latency: ms,
                        ttl: 64,
                        size: payload.len(),
                    })
                    .await;
            }
            Err(_) => {
                let _ = tx.send(PingEvent::Timeout { seq }).await;
            }
        }
        seq += 1;
    }
}
