//! 链路质量评测（有线/无线）。
//!
//! 可选定具体网卡，探测从该网卡源 IP 发出；测试期间持续采样延迟与无线射频状态
//! （RSSI/信号质量/速率），按多维加权模型给出评级。

use super::{config_field_item, FocusArea};
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::session::{LinkParams, LinkQualityPersist};
use crate::ui::mru::MruState;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::textinput::{filter_host, TextInput};
use crate::utils::wlan::{self, WirelessInfo};
use crate::utils::{format, net};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// 一轮探测采样：延迟 + 无线动态字段（仅无线时有值）+ 有线实时协商速率。
#[derive(Debug, Clone, Copy)]
struct Sample {
    latency_ms: Option<u64>,
    rssi_dbm: Option<i32>,
    quality: Option<u32>,
    /// 该轮读到的发送链路速率（bit/s）；有线每轮实时刷新，无线为 None（速率走 wireless）。
    link_speed_bps: Option<u64>,
}

#[derive(Debug)]
enum LinkEvent {
    Sample(Sample),
    Done,
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
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
}

/// 一块可选网卡。
#[derive(Debug, Clone)]
struct IfaceChoice {
    name: String,
    ipv4: Ipv4Addr,
    guid: String,
    is_wifi: bool,
    link_speed_bps: Option<u64>,
    mac: String,
}

#[derive(Debug, Clone)]
struct LinkConfig {
    target: TextInput,
    count: TextInput,
    interval_ms: TextInput,
    timeout_ms: TextInput,
    packet_size: TextInput,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            target: TextInput::with_text("8.8.8.8"),
            count: TextInput::with_text("20"),
            interval_ms: TextInput::with_text("200"),
            timeout_ms: TextInput::with_text("1000"),
            packet_size: TextInput::with_text("32"),
        }
    }
}

/// 开始测试时对所选网卡静态信息的快照。
#[derive(Debug, Clone)]
struct LinkSnapshot {
    iface_name: String,
    is_wifi: bool,
    link_speed_bps: Option<u64>,
    mac: String,
    ipv4: Ipv4Addr,
    wireless: Option<WirelessInfo>,
}

const N_FIELDS: usize = 6; // 0=网卡选择器 + 5 文本字段

pub struct LinkQualityTool {
    config: LinkConfig,
    config_state: ListState,
    ifaces: Vec<IfaceChoice>,
    iface_idx: usize,

    /// 按网卡键（GUID→MAC→名称回退）保存的各自一套参数。
    saved_adapters: BTreeMap<String, LinkParams>,
    /// 当前 `config` 字段所属网卡的键（live 参数归属）；用于切换时归档/载入。
    current_key: Option<String>,

    running: bool,
    error_key: Option<String>,

    samples: Vec<Sample>,
    lat_history: VecDeque<u64>,
    rssi_history: VecDeque<u64>, // 存 (rssi+100) 便于 sparkline 的 u64
    /// 有线测试期间最近一次读到的实时协商速率（bit/s）；反映降速/重协商。
    live_link_speed: Option<u64>,
    total: u64,

    snapshot: Option<LinkSnapshot>,

    tx: mpsc::Sender<LinkEvent>,
    rx: mpsc::Receiver<LinkEvent>,
    abort_flag: Arc<Mutex<bool>>,

    history: Rc<RefCell<HistoryStore>>,
    mru: MruState,
}

impl LinkQualityTool {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(128);
        let mut s = Self {
            config: LinkConfig::default(),
            config_state,
            ifaces: Vec::new(),
            iface_idx: 0,
            saved_adapters: BTreeMap::new(),
            current_key: None,
            running: false,
            error_key: None,
            samples: Vec::new(),
            lat_history: VecDeque::with_capacity(100),
            rssi_history: VecDeque::with_capacity(100),
            live_link_speed: None,
            total: 0,
            snapshot: None,
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
            history,
            mru: MruState::default(),
        };
        s.refresh_ifaces();
        s.current_key = s.ifaces.get(s.iface_idx).map(iface_key);
        s
    }

    // -------------------------------------------------------------------------
    // 按网卡持久化
    // -------------------------------------------------------------------------

    /// 当前 live 参数快照。
    fn live_params(&self) -> LinkParams {
        LinkParams {
            target: self.config.target.value(),
            count: self.config.count.value(),
            interval_ms: self.config.interval_ms.value(),
            timeout_ms: self.config.timeout_ms.value(),
            packet_size: self.config.packet_size.value(),
        }
    }

    /// 把一套参数写入 live 编辑字段。
    fn set_live_params(&mut self, p: &LinkParams) {
        self.config.target = TextInput::with_text(&p.target);
        self.config.count = TextInput::with_text(&p.count);
        self.config.interval_ms = TextInput::with_text(&p.interval_ms);
        self.config.timeout_ms = TextInput::with_text(&p.timeout_ms);
        self.config.packet_size = TextInput::with_text(&p.packet_size);
    }

    /// 把当前 live 参数归档到当前网卡键下（离开该网卡前调用）。
    fn stash_current(&mut self) {
        if let Some(k) = self.current_key.clone() {
            let params = self.live_params();
            self.saved_adapters.insert(k, params);
        }
    }

    /// 按 `iface_idx` 指向的网卡载入其参数：已存则载入，未存则用默认。
    fn load_current(&mut self) {
        let key = self.ifaces.get(self.iface_idx).map(iface_key);
        self.current_key = key.clone();
        if let Some(k) = key {
            let params = self.saved_adapters.get(&k).cloned().unwrap_or_default();
            self.set_live_params(&params);
        }
    }

    /// 导出可持久化参数（含当前 live 参数合并进当前网卡键）。
    pub fn export_persist(&self) -> LinkQualityPersist {
        let mut adapters = self.saved_adapters.clone();
        if let Some(k) = &self.current_key {
            adapters.insert(k.clone(), self.live_params());
        }
        LinkQualityPersist {
            adapters,
            selected: self.current_key.clone(),
        }
    }

    /// 回灌持久化参数：恢复按网卡的参数表，并按上次选中的网卡键重新定位 + 载入。
    pub fn apply_persist(&mut self, p: &LinkQualityPersist) {
        self.saved_adapters = p.adapters.clone();
        self.refresh_ifaces();
        if let Some(sel) = &p.selected {
            if let Some(idx) = self.ifaces.iter().position(|c| &iface_key(c) == sel) {
                self.iface_idx = idx;
            }
        }
        self.load_current();
    }

    /// 重新枚举可选网卡（活跃物理网卡且有 IPv4），并夹紧当前选择。
    fn refresh_ifaces(&mut self) {
        let mut choices = Vec::new();
        for i in net::get_interfaces() {
            if !(i.is_up && i.is_physical && !i.ipv4.is_empty()) {
                continue;
            }
            let ipv4 = i.ipv4.iter().find_map(|s| s.parse::<Ipv4Addr>().ok());
            let ipv4 = match ipv4 {
                Some(v) => v,
                None => continue,
            };
            let is_wifi = i.interface_type.contains("Ieee80211") || i.ssid.is_some();
            choices.push(IfaceChoice {
                name: i.name.clone(),
                ipv4,
                guid: i.guid.clone(),
                is_wifi,
                link_speed_bps: i.link_speed_bps,
                mac: i.mac.clone(),
            });
        }
        self.ifaces = choices;
        if self.ifaces.is_empty() {
            self.iface_idx = 0;
        } else if self.iface_idx >= self.ifaces.len() {
            self.iface_idx = self.ifaces.len() - 1;
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LinkEvent::Sample(s) => {
                    if let Some(l) = s.latency_ms {
                        push_cap(&mut self.lat_history, l, 100);
                    }
                    if let Some(r) = s.rssi_dbm {
                        push_cap(&mut self.rssi_history, (r + 100).max(0) as u64, 100);
                    }
                    if let Some(ls) = s.link_speed_bps {
                        self.live_link_speed = Some(ls);
                    }
                    self.samples.push(s);
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
        let idx = self.config_state.selected().unwrap_or(0);

        // 网卡选择器：Left/Right 循环切换。切换前归档当前网卡参数，切换后载入目标网卡参数，
        // 实现「无线/有线各记各的目标 IP 等参数，切换自动跟随」。
        if idx == 0 && !self.running {
            match action {
                Some(Action::Left) => {
                    self.stash_current();
                    self.refresh_ifaces();
                    if !self.ifaces.is_empty() {
                        self.iface_idx =
                            (self.iface_idx + self.ifaces.len() - 1) % self.ifaces.len();
                    }
                    self.load_current();
                    return;
                }
                Some(Action::Right) => {
                    self.stash_current();
                    self.refresh_ifaces();
                    if !self.ifaces.is_empty() {
                        self.iface_idx = (self.iface_idx + 1) % self.ifaces.len();
                    }
                    self.load_current();
                    return;
                }
                _ => {}
            }
        }

        // MRU 历史下拉 / 行尾灰字采纳 / Ctrl+R 开下拉，仅对目标字段（idx==1）启用。
        let on_target = idx == 1;
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

        // 文本字段（idx 1..=5）：带光标编辑
        if idx >= 1 && !self.running {
            let too_long =
                matches!(key.code, KeyCode::Char(_)) && self.field_mut(idx).value().len() >= 64;
            if !too_long {
                let consumed = if idx == 1 {
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
            1 => &mut self.config.target,
            2 => &mut self.config.count,
            3 => &mut self.config.interval_ms,
            4 => &mut self.config.timeout_ms,
            _ => &mut self.config.packet_size,
        }
    }

    fn next_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| (i + 1) % N_FIELDS)
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| if i == 0 { N_FIELDS - 1 } else { i - 1 })
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    /// 连通性统计 (已发, 已收, 丢包率%, min, avg, max, 抖动)。
    fn stats(&self) -> (u64, u64, f64, u64, u64, u64, u64) {
        let sent = self.samples.len() as u64;
        let lat: Vec<u64> = self.samples.iter().filter_map(|s| s.latency_ms).collect();
        let recv = lat.len() as u64;
        let loss = if sent > 0 {
            ((sent - recv) as f64 / sent as f64) * 100.0
        } else {
            0.0
        };
        let avg = if recv > 0 {
            lat.iter().sum::<u64>() / recv
        } else {
            0
        };
        let min = lat.iter().copied().min().unwrap_or(0);
        let max = lat.iter().copied().max().unwrap_or(0);
        let jitter = if lat.len() > 1 {
            let s: u64 = lat.windows(2).map(|w| w[1].abs_diff(w[0])).sum();
            s / (lat.len() as u64 - 1)
        } else {
            0
        };
        (sent, recv, loss, min, avg, max, jitter)
    }

    /// 无线 RSSI 统计 (min, avg, max)；无样本时返回 None。
    fn rssi_stats(&self) -> Option<(i32, i32, i32)> {
        let v: Vec<i32> = self.samples.iter().filter_map(|s| s.rssi_dbm).collect();
        if v.is_empty() {
            return None;
        }
        let min = *v.iter().min().unwrap();
        let max = *v.iter().max().unwrap();
        let avg = (v.iter().sum::<i32>()) / (v.len() as i32);
        Some((min, avg, max))
    }

    /// 无线信号质量统计 (min, avg, max)%；无样本时 None。
    fn quality_stats(&self) -> Option<(u32, u32, u32)> {
        let v: Vec<u32> = self.samples.iter().filter_map(|s| s.quality).collect();
        if v.is_empty() {
            return None;
        }
        let min = *v.iter().min().unwrap();
        let max = *v.iter().max().unwrap();
        let avg = v.iter().sum::<u32>() / (v.len() as u32);
        Some((min, avg, max))
    }

    /// 计算各维度 (标签 i18n key, 子评分, 权重) 列表。
    fn dimensions(&self) -> Vec<(&'static str, f64, f64)> {
        let (_, recv, loss, _, avg, _, jitter) = self.stats();
        // 无任何回包时延迟/抖动无有效测量，按最差计（否则 0ms 会被误判为满分）。
        let (latency, jit) = if recv == 0 {
            (0.0, 0.0)
        } else {
            (
                score::latency_score(avg as f64),
                score::jitter_score(jitter as f64),
            )
        };
        let los = score::loss_score(loss);

        let is_wifi = self.snapshot.as_ref().map(|s| s.is_wifi).unwrap_or(false);
        if is_wifi {
            let (sig, rate, phy) = self.wifi_dim_scores();
            vec![
                ("diag_link_dim_loss", los, 25.0),
                ("diag_link_dim_latency", latency, 20.0),
                ("diag_link_dim_jitter", jit, 15.0),
                ("diag_link_dim_signal", sig, 25.0),
                ("diag_link_dim_rate", rate, 10.0),
                ("diag_link_dim_phy", phy, 5.0),
            ]
        } else {
            vec![
                ("diag_link_dim_loss", los, 40.0),
                ("diag_link_dim_latency", latency, 35.0),
                ("diag_link_dim_jitter", jit, 25.0),
            ]
        }
    }

    /// 无线三个维度子评分：信号(用采样 RSSI 均值)、速率(协商 Tx)、制式(代际)。
    fn wifi_dim_scores(&self) -> (f64, f64, f64) {
        let w = self.snapshot.as_ref().and_then(|s| s.wireless.as_ref());
        let rssi_avg = self
            .rssi_stats()
            .map(|(_, a, _)| a as f64)
            .or_else(|| w.map(|w| w.rssi_dbm as f64))
            .unwrap_or(-100.0);
        let sig = score::signal_score(rssi_avg);
        let rate = score::rate_score(w.map(|w| w.tx_rate_mbps as f64).unwrap_or(0.0));
        let phy = score::phy_score(w.map(|w| w.wifi_gen).unwrap_or(0));
        (sig, rate, phy)
    }

    fn overall_grade(&self) -> Option<(f64, Grade, usize)> {
        let (sent, _recv, _, _, _, _, _) = self.stats();
        if sent == 0 {
            return None;
        }
        let dims = self.dimensions();
        let pairs: Vec<(f64, f64)> = dims.iter().map(|(_, s, w)| (*s, *w)).collect();
        let o = score::overall(&pairs);
        // 最弱维度索引
        let weakest = dims
            .iter()
            .enumerate()
            .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1))
            .map(|(i, _)| i)
            .unwrap_or(0);
        Some((o, score::grade_from_score(o), weakest))
    }

    fn start(&mut self) {
        self.refresh_ifaces();
        // 保持 live 参数归属键与选中项一致（刷新可能夹紧索引）。
        self.current_key = self.ifaces.get(self.iface_idx).map(iface_key);
        let iface = match self.ifaces.get(self.iface_idx) {
            Some(c) => c.clone(),
            None => {
                self.error_key = Some("diag_link_no_iface".to_string());
                return;
            }
        };

        let count: u64 = self
            .config
            .count
            .value()
            .parse()
            .unwrap_or(20)
            .clamp(5, 100);
        let interval_ms: u64 = self
            .config
            .interval_ms
            .value()
            .parse()
            .unwrap_or(200)
            .clamp(50, 5000);
        let timeout_ms: u32 = self
            .config
            .timeout_ms
            .value()
            .parse()
            .unwrap_or(1000)
            .clamp(100, 10000);
        let packet_size: usize = self
            .config
            .packet_size
            .value()
            .parse()
            .unwrap_or(32)
            .clamp(0, 1472);
        let target = self.config.target.value().trim().to_string();
        if target.is_empty() {
            self.error_key = Some("diag_link_err".to_string());
            return;
        }
        self.history.borrow_mut().targets.record(&target);

        // 静态快照：无线则查一次完整无线信息
        let wireless = if iface.is_wifi {
            wlan::query(&iface.guid)
        } else {
            None
        };
        self.snapshot = Some(LinkSnapshot {
            iface_name: iface.name.clone(),
            is_wifi: iface.is_wifi,
            link_speed_bps: iface.link_speed_bps,
            mac: iface.mac.clone(),
            ipv4: iface.ipv4,
            wireless,
        });

        self.running = true;
        self.error_key = None;
        self.samples.clear();
        self.lat_history.clear();
        self.rssi_history.clear();
        self.live_link_speed = None;
        self.total = count;
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();
        let src = iface.ipv4;
        let guid = iface.guid.clone();
        let is_wifi = iface.is_wifi;

        tokio::spawn(async move {
            let dest: Ipv4Addr = match resolve_v4(&target).await {
                Some(v) => v,
                None => {
                    let _ = tx.send(LinkEvent::Error("diag_link_err".into())).await;
                    return;
                }
            };

            for i in 0..count {
                if *abort.lock().unwrap() {
                    return;
                }
                let res = match tokio::task::spawn_blocking(move || {
                    super::icmp::echo_once_from(src, dest, 128, timeout_ms, packet_size)
                })
                .await
                {
                    Ok(r) => r,
                    Err(_) => return,
                };

                if res.status == u32::MAX {
                    let _ = tx
                        .send(LinkEvent::Error("diag_link_unsupported".into()))
                        .await;
                    return;
                }

                let latency = if res.reached() { res.rtt_ms } else { None };

                // 动态采样：无线取 RSSI/信号质量，有线实时读协商速率（反映降速/重协商）。
                let (rssi, quality, link_speed) = if is_wifi {
                    let g = guid.clone();
                    match tokio::task::spawn_blocking(move || wlan::query(&g)).await {
                        Ok(Some(w)) => (Some(w.rssi_dbm), Some(w.signal_quality), None),
                        _ => (None, None, None),
                    }
                } else {
                    let g = guid.clone();
                    let ls = tokio::task::spawn_blocking(move || net::link_speed_for_guid(&g))
                        .await
                        .ok()
                        .flatten();
                    (None, None, ls)
                };

                let _ = tx
                    .send(LinkEvent::Sample(Sample {
                        latency_ms: latency,
                        rssi_dbm: rssi,
                        quality,
                        link_speed_bps: link_speed,
                    }))
                    .await;

                if i + 1 < count {
                    tokio::time::sleep(Duration::from_millis(interval_ms)).await;
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

        let is_wifi = self.snapshot.as_ref().map(|s| s.is_wifi).unwrap_or(false);
        let n_dims = if is_wifi { 6 } else { 3 };
        let rssi_rows = if is_wifi { 3 } else { 0 };
        let metrics_rows = if is_wifi { 6 } else { 4 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                   // header
                Constraint::Length(1),                   // overall gauge
                Constraint::Length(n_dims as u16),       // dim bars
                Constraint::Length(metrics_rows as u16), // metrics grid
                Constraint::Min(3),                      // latency sparkline
                Constraint::Length(rssi_rows as u16),    // rssi sparkline (wifi)
                Constraint::Length(1),                   // status
            ])
            .split(inner);

        self.draw_header(f, chunks[0], i18n);
        self.draw_overall(f, chunks[1], i18n);
        self.draw_dim_bars(f, chunks[2], i18n);
        self.draw_metrics(f, chunks[3], i18n, is_wifi);

        let data: Vec<u64> = self.lat_history.iter().cloned().collect();
        let spark = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(i18n.t("diag_link_history")),
            )
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(spark, chunks[4]);

        if is_wifi {
            let rdata: Vec<u64> = self.rssi_history.iter().cloned().collect();
            let rspark = Sparkline::default()
                .block(
                    Block::default()
                        .borders(Borders::TOP)
                        .title(i18n.t("diag_link_rssi_history")),
                )
                .data(&rdata)
                .style(Style::default().fg(Color::Magenta));
            f.render_widget(rspark, chunks[5]);
        }

        self.draw_status(f, chunks[6], i18n);
    }

    fn draw_header(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let mut spans = vec![Span::styled(
            format!("{}: ", i18n.t("diag_link_interface")),
            Style::default().fg(Color::Gray),
        )];
        match &self.snapshot {
            Some(s) => {
                let badge = if s.is_wifi {
                    i18n.t("diag_link_wireless")
                } else {
                    i18n.t("diag_link_wired")
                };
                spans.push(Span::styled(
                    format!("{} [{}]", s.iface_name, badge),
                    Style::default().fg(theme::COLOR_SECONDARY),
                ));
                if s.is_wifi {
                    match &s.wireless {
                        Some(w) => spans.push(Span::styled(
                            format!("  {}: {}", i18n.t("diag_link_ssid"), w.ssid),
                            Style::default().fg(Color::White),
                        )),
                        None => spans.push(Span::styled(
                            format!("  {}", i18n.t("diag_link_wifi_na")),
                            Style::default().fg(Color::DarkGray),
                        )),
                    }
                } else if let Some(sp) = s.link_speed_bps {
                    spans.push(Span::styled(
                        format!("  {}", format::format_speed(sp / 8)),
                        Style::default().fg(Color::White),
                    ));
                }
            }
            None => {
                let name = self
                    .ifaces
                    .get(self.iface_idx)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| i18n.t("diag_link_no_iface"));
                spans.push(Span::styled(
                    name,
                    Style::default().fg(theme::COLOR_SECONDARY),
                ));
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_overall(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let (label, ratio, gcolor) = match self.overall_grade() {
            Some((sc, g, _)) => (
                format!(
                    "{}: {} ({:.0})",
                    i18n.t("diag_link_grade"),
                    i18n.t(g.i18n_key()),
                    sc
                ),
                (sc / 100.0).clamp(0.0, 1.0),
                g.color(),
            ),
            None => (
                format!("{}: -", i18n.t("diag_link_grade")),
                0.0,
                Color::DarkGray,
            ),
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(gcolor).bg(Color::DarkGray))
            .ratio(ratio)
            .label(label);
        f.render_widget(gauge, area);
    }

    fn draw_dim_bars(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let weakest = self.overall_grade().map(|(_, _, w)| w);
        let dims = self.dimensions();
        let lines: Vec<Line> = dims
            .iter()
            .enumerate()
            .map(|(i, (key, sc, _))| {
                let bar = score_bar(*sc, 12);
                let c = bar_color(*sc);
                let mark = if Some(i) == weakest { " ◀" } else { "" };
                Line::from(vec![
                    Span::styled(
                        format!("{:<8}", i18n.t(key)),
                        Style::default().fg(Color::Gray),
                    ),
                    Span::styled(bar, Style::default().fg(c)),
                    Span::styled(format!(" {:>3.0}{}", sc, mark), Style::default().fg(c)),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), area);
    }

    fn draw_metrics(&self, f: &mut Frame, area: Rect, i18n: &I18n, is_wifi: bool) {
        let (sent, _recv, loss, min, avg, max, jitter) = self.stats();
        let g = |k: &str| -> String { i18n.t(k) };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_avg")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{}/{}/{} ms  ", min, avg, max),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{}: ", g("diag_link_jitter")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("{} ms", jitter), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_loss")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{:.1}%  ", loss),
                    Style::default().fg(if loss > 5.0 { Color::Red } else { Color::Green }),
                ),
                Span::styled(
                    format!("{}: ", g("diag_link_sent")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    format!("{}/{}", sent, self.total),
                    Style::default().fg(theme::COLOR_SECONDARY),
                ),
            ]),
        ];

        if is_wifi {
            let w = self.snapshot.as_ref().and_then(|s| s.wireless.as_ref());
            let rssi_txt = match self.rssi_stats() {
                Some((mn, av, mx)) => format!("{}/{}/{} dBm", mn, av, mx),
                None => w
                    .map(|w| format!("{} dBm", w.rssi_dbm))
                    .unwrap_or_else(|| "-".into()),
            };
            // 行1：RSSI(min/avg/max) + 信道(频段, 频率)
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_rssi")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("{}  ", rssi_txt), Style::default().fg(Color::White)),
                Span::styled(
                    format!("{}: ", g("diag_link_channel")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    w.map(|w| format!("{} ({}, {} MHz)", w.channel, w.band, w.freq_mhz))
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
            // 行2：信号质量(min/avg/max %) + 制式
            let q_txt = match self.quality_stats() {
                Some((mn, av, mx)) => format!("{}/{}/{} %", mn, av, mx),
                None => w
                    .map(|w| format!("{} %", w.signal_quality))
                    .unwrap_or_else(|| "-".into()),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_signal_q")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(format!("{}  ", q_txt), Style::default().fg(Color::White)),
                Span::styled(
                    format!("{}: ", g("diag_link_phy")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    w.map(|w| w.phy_type.clone()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
            // 行3：发送/接收协商速率
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}/{}: ", g("diag_link_rate_tx"), g("diag_link_rate_rx")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    w.map(|w| format!("{}/{} Mbps", w.tx_rate_mbps, w.rx_rate_mbps))
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
            // 行4：BSSID + 加密(认证 / 加密算法)
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_bssid")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    w.map(|w| w.bssid.clone()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("  {}: ", g("diag_link_auth")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    w.map(|w| format!("{} / {}", w.auth, w.cipher))
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
        } else {
            let s = self.snapshot.as_ref();
            // 速率优先用实时采样（反映降速/重协商）；尚无采样时退回开始快照。
            let speed_bps = if self.samples.is_empty() {
                s.and_then(|s| s.link_speed_bps)
            } else {
                self.live_link_speed
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{}: ", g("diag_link_speed")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    speed_bps
                        .map(|sp| format::format_speed(sp / 8))
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("  {}: ", g("diag_link_media")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(g("diag_link_media_up"), Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("MAC: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.map(|s| s.mac.clone()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled("  IPv4: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.map(|s| s.ipv4.to_string()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        f.render_widget(Paragraph::new(lines), area);
    }

    fn draw_status(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!(
                    "{} | {}",
                    i18n.t("diag_status_running"),
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
        f.render_widget(Paragraph::new(text).style(style), area);
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
        let selected = self.config_state.selected();

        // 网卡选择器显示值（只读文本形式复用 config_field_item）
        let iface_display = match self.ifaces.get(self.iface_idx) {
            Some(c) => format!("{} ({})", c.name, c.ipv4),
            None => i18n.t("diag_link_no_iface"),
        };
        let iface_input = TextInput::with_text(&iface_display);

        let labels = [
            i18n.t("diag_link_interface"),
            i18n.t("diag_link_target"),
            i18n.t("diag_link_count"),
            i18n.t("diag_link_interval"),
            i18n.t("diag_link_timeout"),
            i18n.t("diag_link_packet"),
        ];

        let mut items: Vec<ListItem> = Vec::with_capacity(N_FIELDS);
        for i in 0..N_FIELDS {
            let is_sel = selected == Some(i);
            let active = is_sel && is_active && !self.running;
            if i == 0 {
                // 选择器：不显示光标，提示 ←→ 切换
                let hint = if active {
                    Some(i18n.t("diag_hint_switch"))
                } else {
                    None
                };
                items.push(config_field_item(
                    &labels[0],
                    is_sel,
                    is_active,
                    &iface_input,
                    false,
                    hint,
                ));
            } else if i == 1 {
                // 目标字段：带 MRU 灰字补全，手动拼装（不走 config_field_item）。
                let val_base = if is_sel && is_active {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let marker = if is_sel { ">> " } else { "   " };
                let mut spans = vec![Span::styled(marker.to_string(), val_base)];
                spans.extend(crate::ui::mru::mru_ghost_spans(
                    &self.config.target,
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
                    format!("{}:", labels[1]),
                    Style::default().fg(if is_sel { Color::Yellow } else { Color::Gray }),
                ));
                items.push(ListItem::new(vec![label_line, Line::from(spans)]));
            } else {
                let input = match i {
                    2 => &self.config.count,
                    3 => &self.config.interval_ms,
                    4 => &self.config.timeout_ms,
                    _ => &self.config.packet_size,
                };
                let hint = if active {
                    Some(i18n.t("diag_hint_digits"))
                } else {
                    None
                };
                items.push(config_field_item(
                    &labels[i], is_sel, is_active, input, active, hint,
                ));
            }
        }

        f.render_widget(List::new(items), inner);
    }
}

/// 网卡稳定标识：优先 GUID（Windows 重启稳定），次选 MAC，再退回名称。
/// 用作「按网卡持久化」的键，使无线/有线各自记住自己的参数。
fn iface_key(c: &IfaceChoice) -> String {
    if !c.guid.is_empty() {
        c.guid.clone()
    } else if !c.mac.is_empty() {
        c.mac.clone()
    } else {
        c.name.clone()
    }
}

/// 把值推入定长 ring buffer。
fn push_cap(buf: &mut VecDeque<u64>, v: u64, cap: usize) {
    if buf.len() >= cap {
        buf.pop_front();
    }
    buf.push_back(v);
}

/// 0-100 分数 → 定宽方块条字符串。
fn score_bar(score: f64, width: usize) -> String {
    let filled = ((score / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn bar_color(score: f64) -> Color {
    if score >= 85.0 {
        Color::Green
    } else if score >= 70.0 {
        Color::Cyan
    } else if score >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// 解析目标为 IPv4（直接 IP 或 DNS 解析取首个 v4）。
async fn resolve_v4(target: &str) -> Option<Ipv4Addr> {
    if let Ok(IpAddr::V4(v4)) = target.parse::<IpAddr>() {
        return Some(v4);
    }
    if let Ok(it) = tokio::net::lookup_host((target, 0u16)).await {
        for sa in it {
            if let IpAddr::V4(v4) = sa.ip() {
                return Some(v4);
            }
        }
    }
    None
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
