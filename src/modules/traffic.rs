use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table, TableState},
};
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::Networks;
use unicode_width::UnicodeWidthStr;

struct TrafficSnapshot {
    total_rx: u64,
    total_tx: u64,
    last_update: Instant,
    rx_speed: u64,
    tx_speed: u64,
}

pub struct TrafficModule {
    state: TableState,
    networks: Networks,
    history: HashMap<String, TrafficSnapshot>,
    initial_stats: HashMap<String, (u64, u64)>,
    display_items: Vec<DisplayItem>,
}

struct DisplayItem {
    name: String,
    rx_speed: u64,
    tx_speed: u64,
    session_rx: u64,
    session_tx: u64,
    total_rx: u64,
    total_tx: u64,
}

impl TrafficModule {
    pub fn new() -> Self {
        let mut networks = Networks::new_with_refreshed_list();
        networks.refresh(true);

        let mut initial_stats = HashMap::new();
        for (name, data) in &networks {
            initial_stats.insert(
                name.clone(),
                (data.total_received(), data.total_transmitted()),
            );
        }

        Self {
            state: TableState::default(),
            networks,
            history: HashMap::new(),
            initial_stats,
            display_items: Vec::new(),
        }
    }

    pub fn update(&mut self) {
        self.networks.refresh(true);
        let now = Instant::now();

        self.display_items.clear();

        // 定义过滤关键词列表
        // 包含这些词的接口通常是：回环、虚拟隧道、驱动钩子(Filter Drivers)或重复映射
        let ignore_keywords = [
            "loopback",      // 本地回环
            "pseudo",        // 伪接口
            "isatap",        // IPv6 隧道
            "teredo",        // IPv6 隧道
            "npcap",         // 抓包驱动
            "packet driver", // 抓包驱动
            "genicam",       // 工业相机 Ethern et 驱动钩子
            "tpacket",       // TPacket 驱动钩子
            "driver-",       // 通用驱动实例后缀，如 Driver-0000
            "lltdio",        // 链路层拓扑发现
            "rspndr",        // 链路层拓扑发现响应器
            "virtual box",   // VirtualBox 虚拟网卡 (可选，视需求而定，暂不过滤)
            "vmware",        // VMware 虚拟网卡 (可选，视需求而定，暂不过滤)
        ];

        for (name, data) in &self.networks {
            // 1. 增强过滤逻辑：
            let name_lower = name.to_lowercase();

            // 如果名称包含任何黑名单关键词，则跳过
            if ignore_keywords.iter().any(|k| name_lower.contains(k)) {
                continue;
            }

            let current_rx = data.total_received();
            let current_tx = data.total_transmitted();

            // 2. 计算速率
            let mut rx_speed = 0;
            let mut tx_speed = 0;

            if let Some(prev) = self.history.get_mut(name) {
                let duration = now.duration_since(prev.last_update).as_secs_f64();
                // 只有当时间间隔足够长时才更新速率，避免除零或波动过大
                if duration > 0.1 {
                    rx_speed = ((current_rx - prev.total_rx) as f64 / duration) as u64;
                    tx_speed = ((current_tx - prev.total_tx) as f64 / duration) as u64;

                    prev.total_rx = current_rx;
                    prev.total_tx = current_tx;
                    prev.last_update = now;
                    prev.rx_speed = rx_speed;
                    prev.tx_speed = tx_speed;
                } else {
                    rx_speed = prev.rx_speed;
                    tx_speed = prev.tx_speed;
                }
            } else {
                self.history.insert(
                    name.clone(),
                    TrafficSnapshot {
                        total_rx: current_rx,
                        total_tx: current_tx,
                        last_update: now,
                        rx_speed: 0,
                        tx_speed: 0,
                    },
                );
            }

            // 3. 计算会话流量
            let (init_rx, init_tx) = *self.initial_stats.get(name).unwrap_or(&(0, 0));
            let session_rx = current_rx.saturating_sub(init_rx);
            let session_tx = current_tx.saturating_sub(init_tx);

            // 4. 保存完整名称
            self.display_items.push(DisplayItem {
                name: name.clone(),
                rx_speed,
                tx_speed,
                session_rx,
                session_tx,
                total_rx: current_rx,
                total_tx: current_tx,
            });
        }

        // 5. 排序：优先按下载速率降序，其次按名称
        self.display_items.sort_by(|a, b| {
            b.rx_speed
                .cmp(&a.rx_speed)
                .then_with(|| a.name.cmp(&b.name))
        });
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            _ => {}
        }
    }

    fn next(&mut self) {
        if self.display_items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.display_items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.display_items.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.display_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let i18n = &app.i18n;
    let traffic = &mut app.traffic;

    let header_cells = vec![
        Cell::from(i18n.t("traffic_col_name")).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("traffic_col_rx_rate")).style(Style::default().fg(Color::Green)),
        Cell::from(i18n.t("traffic_col_tx_rate")).style(Style::default().fg(Color::Yellow)),
        Cell::from(i18n.t("traffic_col_session")).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("traffic_col_total")).style(Style::default().fg(Color::Gray)),
    ];
    let header = Row::new(header_cells).height(1).bottom_margin(1);

    // 动态计算最长名称的宽度
    // 基础宽度至少要能放下表头 "Interface Name"，假设大概 15 宽
    // 使用 unicode-width 正确处理中文
    let mut max_name_width = 15;
    for item in &traffic.display_items {
        let w = item.name.width();
        if w > max_name_width {
            max_name_width = w;
        }
    }

    // 限制最大宽度，防止某个变态长的接口名把其他列挤没了
    // 比如最大 50，如果超过 50 还是得截断或换行，但通常 50 足够
    if max_name_width > 50 {
        max_name_width = 50;
    }

    let rows = traffic.display_items.iter().map(|item| {
        let cells = vec![
            Cell::from(item.name.clone()).style(Style::default().add_modifier(Modifier::BOLD)),
            Cell::from(format!("↓ {}", format_speed(item.rx_speed))),
            Cell::from(format!("↑ {}", format_speed(item.tx_speed))),
            Cell::from(format!(
                "{} / {}",
                format_bytes(item.session_rx),
                format_bytes(item.session_tx)
            )),
            Cell::from(format!(
                "{} / {}",
                format_bytes(item.total_rx),
                format_bytes(item.total_tx)
            )),
        ];
        Row::new(cells).height(1)
    });

    // 列宽规划
    // 第1列：固定长度 (最长名称 + 2个字符padding)
    // 后续列：使用 Min 或 Percentage 分配剩余空间
    let constraints = [
        Constraint::Length((max_name_width + 2) as u16),
        Constraint::Percentage(15),
        Constraint::Percentage(15),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ];

    let t = Table::new(rows, constraints)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("traffic_title")),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(t, area, &mut traffic.state);
}

fn format_speed(bps: u64) -> String {
    let kbps = bps as f64 / 1024.0;
    if kbps < 1024.0 {
        format!("{:.1} KB/s", kbps)
    } else {
        format!("{:.2} MB/s", kbps / 1024.0)
    }
}

fn format_bytes(bytes: u64) -> String {
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        format!("{:.1} KB", kb)
    } else if kb < 1024.0 * 1024.0 {
        format!("{:.1} MB", kb / 1024.0)
    } else {
        format!("{:.1} GB", kb / 1024.0 / 1024.0)
    }
}
