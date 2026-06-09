//! 链路质量评测（有线/无线）。
//!
//! 向可靠目标发送一组 ICMP 探测，统计平均延迟、抖动、丢包，综合给出评级；
//! 并识别当前连接介质（无线则显示 SSID）。复用 `icmp::echo_once`。

use super::FocusArea;
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::net;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
enum LinkEvent {
    Sample { latency_ms: Option<u64> },
    Done,
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Grade {
    Excellent,
    Good,
    Fair,
    Poor,
}

impl Grade {
    fn i18n_key(self) -> &'static str {
        match self {
            Grade::Excellent => "diag_link_grade_excellent",
            Grade::Good => "diag_link_grade_good",
            Grade::Fair => "diag_link_grade_fair",
            Grade::Poor => "diag_link_grade_poor",
        }
    }
    fn color(self) -> Color {
        match self {
            Grade::Excellent => Color::Green,
            Grade::Good => Color::Cyan,
            Grade::Fair => Color::Yellow,
            Grade::Poor => Color::Red,
        }
    }
    fn ratio(self) -> f64 {
        match self {
            Grade::Excellent => 1.0,
            Grade::Good => 0.75,
            Grade::Fair => 0.45,
            Grade::Poor => 0.2,
        }
    }
}

#[derive(Debug, Clone)]
struct LinkConfig {
    target: String,
    count: String,
    timeout_ms: String,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".to_string(),
            count: "20".to_string(),
            timeout_ms: "1000".to_string(),
        }
    }
}

pub struct LinkQualityTool {
    config: LinkConfig,
    config_state: ListState,
    running: bool,
    error_key: Option<String>,

    samples: Vec<Option<u64>>,
    history: VecDeque<u64>,
    total: u64,

    medium_wifi: bool,
    ssid: Option<String>,

    tx: mpsc::Sender<LinkEvent>,
    rx: mpsc::Receiver<LinkEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl LinkQualityTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(128);
        Self {
            config: LinkConfig::default(),
            config_state,
            running: false,
            error_key: None,
            samples: Vec::new(),
            history: VecDeque::with_capacity(100),
            total: 0,
            medium_wifi: false,
            ssid: None,
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LinkEvent::Sample { latency_ms } => {
                    self.samples.push(latency_ms);
                    if let Some(l) = latency_ms {
                        if self.history.len() >= 100 {
                            self.history.pop_front();
                        }
                        self.history.push_back(l);
                    }
                }
                LinkEvent::Done => self.running = false,
                LinkEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
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
        if !self.running {
            if let Some(idx) = self.config_state.selected() {
                match key.code {
                    KeyCode::Backspace => {
                        self.field_mut(idx).pop();
                        return;
                    }
                    KeyCode::Char(c) => {
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
            1 => &mut self.config.count,
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

    /// 统计 (已发, 已收, 丢包率%, 平均, 抖动)。
    fn stats(&self) -> (u64, u64, f64, u64, u64) {
        let sent = self.samples.len() as u64;
        let latencies: Vec<u64> = self.samples.iter().filter_map(|s| *s).collect();
        let recv = latencies.len() as u64;
        let loss = if sent > 0 {
            ((sent - recv) as f64 / sent as f64) * 100.0
        } else {
            0.0
        };
        let avg = if recv > 0 {
            latencies.iter().sum::<u64>() / recv
        } else {
            0
        };
        let jitter = if latencies.len() > 1 {
            let mut sum = 0u64;
            for w in latencies.windows(2) {
                sum += w[1].abs_diff(w[0]);
            }
            sum / (latencies.len() as u64 - 1)
        } else {
            0
        };
        (sent, recv, loss, avg, jitter)
    }

    fn grade(&self) -> Option<Grade> {
        let (sent, recv, loss, avg, jitter) = self.stats();
        if sent == 0 || recv == 0 {
            return None;
        }
        // 无线略放宽延迟阈值
        let lat_relax = if self.medium_wifi { 1.3 } else { 1.0 };
        let g = if loss > 5.0 || avg as f64 > 200.0 * lat_relax {
            Grade::Poor
        } else if avg as f64 > 100.0 * lat_relax || jitter > 50 {
            Grade::Fair
        } else if avg as f64 > 50.0 * lat_relax || jitter > 20 {
            Grade::Good
        } else {
            Grade::Excellent
        };
        Some(g)
    }

    fn start(&mut self) {
        let count: u64 = self.config.count.parse().unwrap_or(20).clamp(5, 100);
        let timeout_ms: u32 = self.config.timeout_ms.parse().unwrap_or(1000).clamp(100, 10000);
        let target = self.config.target.trim().to_string();
        if target.is_empty() {
            self.error_key = Some("diag_link_err".to_string());
            return;
        }

        // 识别连接介质（无线/有线）
        let (wifi, ssid) = detect_medium();
        self.medium_wifi = wifi;
        self.ssid = ssid;

        self.running = true;
        self.error_key = None;
        self.samples.clear();
        self.history.clear();
        self.total = count;
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();

        tokio::spawn(async move {
            let dest: Ipv4Addr = match target.parse::<IpAddr>() {
                Ok(IpAddr::V4(v4)) => v4,
                Ok(IpAddr::V6(_)) => {
                    let _ = tx.send(LinkEvent::Error("diag_link_err".into())).await;
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
                                let _ = tx.send(LinkEvent::Error("diag_link_err".into())).await;
                                return;
                            }
                        }
                    },
                    Err(_) => {
                        let _ = tx.send(LinkEvent::Error("diag_link_err".into())).await;
                        return;
                    }
                },
            };

            for i in 0..count {
                if *abort.lock().unwrap() {
                    return;
                }
                let res =
                    match tokio::task::spawn_blocking(move || super::icmp::echo_once(dest, 128, timeout_ms))
                        .await
                    {
                        Ok(r) => r,
                        Err(_) => return,
                    };

                // 平台不支持（本地调用失败）：首个探测即报错退出
                if res.status == u32::MAX {
                    let _ = tx
                        .send(LinkEvent::Error("diag_link_unsupported".into()))
                        .await;
                    return;
                }

                let latency = if res.reached() { res.rtt_ms } else { None };
                let _ = tx.send(LinkEvent::Sample { latency_ms: latency }).await;

                if i + 1 < count {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
            let _ = tx.send(LinkEvent::Done).await;
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
                Constraint::Length(1), // medium
                Constraint::Length(2), // grade gauge
                Constraint::Length(2), // metrics row 1
                Constraint::Length(1), // metrics row 2
                Constraint::Min(3),    // sparkline
                Constraint::Length(1), // status
            ])
            .split(inner);

        // 连接介质
        let medium_text = if self.medium_wifi {
            let ssid = self.ssid.clone().unwrap_or_else(|| "-".to_string());
            format!("{}: {} ({})", i18n.t("diag_link_medium"), i18n.t("diag_link_wireless"), ssid)
        } else {
            format!("{}: {}", i18n.t("diag_link_medium"), i18n.t("diag_link_wired"))
        };
        f.render_widget(
            Paragraph::new(medium_text).style(Style::default().fg(theme::COLOR_SECONDARY)),
            chunks[0],
        );

        // 综合评级 gauge
        let (label, ratio, gcolor) = match self.grade() {
            Some(g) => (
                format!("{}: {}", i18n.t("diag_link_grade"), i18n.t(g.i18n_key())),
                g.ratio(),
                g.color(),
            ),
            None => (format!("{}: -", i18n.t("diag_link_grade")), 0.0, Color::DarkGray),
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(gcolor).bg(Color::DarkGray))
            .ratio(ratio)
            .label(label);
        f.render_widget(gauge, chunks[1]);

        // 指标
        let (sent, _recv, loss, avg, jitter) = self.stats();
        let row1 = Line::from(vec![
            Span::styled(format!("{}: ", i18n.t("diag_link_avg")), Style::default().fg(Color::Gray)),
            Span::styled(format!("{:<8}", format!("{} ms", avg)), Style::default().fg(Color::White)),
            Span::styled(format!("{}: ", i18n.t("diag_link_jitter")), Style::default().fg(Color::Gray)),
            Span::styled(format!("{:<8}", format!("{} ms", jitter)), Style::default().fg(Color::White)),
            Span::styled(format!("{}: ", i18n.t("diag_link_loss")), Style::default().fg(Color::Gray)),
            Span::styled(format!("{:.1}%", loss), Style::default().fg(if loss > 5.0 { Color::Red } else { Color::Green })),
        ]);
        f.render_widget(Paragraph::new(row1), chunks[2]);

        let row2 = Line::from(vec![
            Span::styled(format!("{}: ", i18n.t("diag_link_sent")), Style::default().fg(Color::Gray)),
            Span::styled(format!("{} / {}", sent, self.total), Style::default().fg(theme::COLOR_SECONDARY)),
        ]);
        f.render_widget(Paragraph::new(row2), chunks[3]);

        // 延迟曲线
        let data: Vec<u64> = self.history.iter().cloned().collect();
        let sparkline = Sparkline::default()
            .block(Block::default().borders(Borders::TOP).title(i18n.t("diag_link_history")))
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(sparkline, chunks[4]);

        // 状态
        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!("{} | {}", i18n.t("diag_status_running"), i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(text).style(style), chunks[5]);
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
            (i18n.t("diag_link_target"), self.config.target.clone()),
            (i18n.t("diag_link_count"), self.config.count.clone()),
            (i18n.t("diag_link_timeout"), self.config.timeout_ms.clone()),
        ];
        let list_items: Vec<ListItem> = items
            .iter()
            .map(|(k, v)| ListItem::new(format!("{}:\n  > {}", k, v)))
            .collect();

        let is_active = is_focused && active_focus == FocusArea::Config;
        let list = List::new(list_items)
            .highlight_style(if is_active {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
            .highlight_symbol(">> ");
        f.render_stateful_widget(list, inner, &mut self.config_state);
    }
}

/// 识别当前活跃连接是否为无线，以及 SSID。
fn detect_medium() -> (bool, Option<String>) {
    let interfaces = net::get_interfaces();
    // 优先取有 SSID 的活跃接口（无线）
    if let Some(wifi) = interfaces
        .iter()
        .find(|i| i.is_up && i.ssid.is_some() && !i.ipv4.is_empty())
    {
        return (true, wifi.ssid.clone());
    }
    // 其次看活跃物理接口类型是否为 802.11
    if let Some(iface) = interfaces
        .iter()
        .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
    {
        let is_wifi = iface.interface_type.contains("Ieee80211");
        return (is_wifi, iface.ssid.clone());
    }
    (false, None)
}
