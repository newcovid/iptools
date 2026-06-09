//! 链路质量评测（有线/无线）。
//!
//! 向可靠目标发送一组 ICMP 探测，统计平均延迟、抖动、丢包，综合给出评级；
//! 并识别当前连接介质（无线则显示 SSID）。复用 `icmp::echo_once`。

use super::{config_field_item, FocusArea};
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::net;
use crate::utils::textinput::{filter_host, TextInput};
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
    target: TextInput,
    count: TextInput,
    timeout_ms: TextInput,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            target: TextInput::with_text("8.8.8.8"),
            count: TextInput::with_text("20"),
            timeout_ms: TextInput::with_text("1000"),
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
        // 带光标编辑：目标接受主机名字符，次数/超时仅数字。
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
        let count: u64 = self.config.count.value().parse().unwrap_or(20).clamp(5, 100);
        let timeout_ms: u32 = self
            .config
            .timeout_ms
            .value()
            .parse()
            .unwrap_or(1000)
            .clamp(100, 10000);
        let target = self.config.target.value().trim().to_string();
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

        let is_active = is_focused && active_focus == FocusArea::Config;
        let labels = [
            i18n.t("diag_link_target"),
            i18n.t("diag_link_count"),
            i18n.t("diag_link_timeout"),
        ];
        let inputs = [
            &self.config.target,
            &self.config.count,
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

/// 多维评分纯函数：各维度映射为 0-100，加权汇总，划定评级。与 UI 解耦，便于单测。
pub(super) mod score {
    use super::Grade;

    /// 线性映射并夹紧到 0..100：value=best→100，value=worst→0（best/worst 大小关系任意）。
    pub fn lerp_score(value: f64, best: f64, worst: f64) -> f64 {
        if (best - worst).abs() < f64::EPSILON {
            return 0.0;
        }
        let t = (value - worst) / (best - worst);
        t.clamp(0.0, 1.0) * 100.0
    }

    pub fn latency_score(avg_ms: f64) -> f64 {
        lerp_score(avg_ms, 20.0, 300.0)
    }
    pub fn jitter_score(jitter_ms: f64) -> f64 {
        lerp_score(jitter_ms, 2.0, 80.0)
    }
    pub fn loss_score(loss_pct: f64) -> f64 {
        lerp_score(loss_pct, 0.0, 10.0)
    }
    pub fn signal_score(rssi_dbm: f64) -> f64 {
        lerp_score(rssi_dbm, -50.0, -85.0)
    }
    pub fn rate_score(mbps: f64) -> f64 {
        lerp_score(mbps, 433.0, 6.0)
    }
    pub fn phy_score(wifi_gen: u8) -> f64 {
        match wifi_gen {
            7 | 6 => 100.0,
            5 => 80.0,
            4 => 60.0,
            _ => 30.0,
        }
    }

    /// 加权汇总：dims 为 (子评分, 权重)。返回 0-100。
    pub fn overall(dims: &[(f64, f64)]) -> f64 {
        let wsum: f64 = dims.iter().map(|(_, w)| w).sum();
        if wsum <= 0.0 {
            return 0.0;
        }
        dims.iter().map(|(s, w)| s * w).sum::<f64>() / wsum
    }

    pub fn grade_from_score(s: f64) -> Grade {
        if s >= 85.0 {
            Grade::Excellent
        } else if s >= 70.0 {
            Grade::Good
        } else if s >= 50.0 {
            Grade::Fair
        } else {
            Grade::Poor
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn linear_endpoints_and_mid() {
            assert!((latency_score(20.0) - 100.0).abs() < 1e-6);
            assert!((latency_score(300.0) - 0.0).abs() < 1e-6);
            assert!((latency_score(160.0) - 50.0).abs() < 1.0);
            assert!((loss_score(0.0) - 100.0).abs() < 1e-6);
            assert!((loss_score(10.0) - 0.0).abs() < 1e-6);
            assert!((signal_score(-50.0) - 100.0).abs() < 1e-6);
            assert!((signal_score(-85.0) - 0.0).abs() < 1e-6);
            assert!((rate_score(433.0) - 100.0).abs() < 1e-6);
            assert!((rate_score(6.0) - 0.0).abs() < 1e-6);
        }

        #[test]
        fn phy_tiers() {
            assert_eq!(phy_score(6), 100.0);
            assert_eq!(phy_score(5), 80.0);
            assert_eq!(phy_score(4), 60.0);
            assert_eq!(phy_score(0), 30.0);
        }

        #[test]
        fn weighted_and_grade() {
            // 全满 → Excellent
            let dims = [(100.0, 40.0), (100.0, 35.0), (100.0, 25.0)];
            let o = overall(&dims);
            assert!((o - 100.0).abs() < 1e-6);
            assert_eq!(grade_from_score(o), Grade::Excellent);
            // 分级边界
            assert_eq!(grade_from_score(86.0), Grade::Excellent);
            assert_eq!(grade_from_score(72.0), Grade::Good);
            assert_eq!(grade_from_score(55.0), Grade::Fair);
            assert_eq!(grade_from_score(40.0), Grade::Poor);
        }
    }
}
