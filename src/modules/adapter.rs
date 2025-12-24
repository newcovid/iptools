use crate::app::App;
use crate::utils::i18n::I18n;
use crate::utils::net::{self, InterfaceInfo};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table},
};
use std::collections::HashMap;
use std::time::Instant;
use sysinfo::Networks;

/// 流量数据快照
struct TrafficStats {
    total_rx: u64,
    total_tx: u64,
    last_update: Instant,
    rx_speed: u64,
    tx_speed: u64,
}

pub struct AdapterModule {
    pub state: ListState,
    pub interfaces: Vec<InterfaceInfo>,
    networks: Networks,
    traffic_history: HashMap<String, TrafficStats>,
}

impl AdapterModule {
    pub fn new() -> Self {
        let mut state = ListState::default();
        let interfaces = net::get_interfaces();
        if !interfaces.is_empty() {
            state.select(Some(0));
        }

        Self {
            state,
            interfaces,
            networks: Networks::new_with_refreshed_list(),
            traffic_history: HashMap::new(),
        }
    }

    pub fn update(&mut self) {
        self.networks.refresh(true);
        let now = Instant::now();

        for (name, data) in &self.networks {
            let rx = data.total_received();
            let tx = data.total_transmitted();

            if let Some(prev) = self.traffic_history.get_mut(name) {
                let duration = now.duration_since(prev.last_update).as_secs_f64();
                if duration > 0.1 {
                    prev.rx_speed = ((rx - prev.total_rx) as f64 / duration) as u64;
                    prev.tx_speed = ((tx - prev.total_tx) as f64 / duration) as u64;
                    prev.total_rx = rx;
                    prev.total_tx = tx;
                    prev.last_update = now;
                }
            } else {
                self.traffic_history.insert(
                    name.clone(),
                    TrafficStats {
                        total_rx: rx,
                        total_tx: tx,
                        last_update: now,
                        rx_speed: 0,
                        tx_speed: 0,
                    },
                );
            }
        }
    }

    pub fn reload(&mut self) {
        self.interfaces = net::get_interfaces();
        if let Some(selected) = self.state.selected() {
            if selected >= self.interfaces.len() {
                self.state
                    .select(Some(self.interfaces.len().saturating_sub(1)));
            }
        } else if !self.interfaces.is_empty() {
            self.state.select(Some(0));
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Char('r') => self.reload(),
            _ => {}
        }
    }

    fn next(&mut self) {
        if self.interfaces.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.interfaces.len().saturating_sub(1) {
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
        if self.interfaces.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.interfaces.len().saturating_sub(1)
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
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let i18n = &app.i18n;
    let adapter_module = &mut app.adapter;

    // --- 左侧列表 ---
    let items: Vec<ListItem> = adapter_module
        .interfaces
        .iter()
        .map(|iface| {
            let color = if iface.is_up {
                Color::White
            } else {
                Color::DarkGray
            };
            // 简单标记：[P]hysical / [V]irtual
            let prefix = if iface.is_physical { "[P] " } else { "[V] " };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(Color::Cyan)),
                Span::styled(iface.name.clone(), Style::default().fg(color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("adapter_list_title")),
        )
        .highlight_style(Style::default().bg(Color::DarkGray))
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[0], &mut adapter_module.state);

    // --- 右侧详情 (使用 Table) ---
    let block = Block::default()
        .borders(Borders::ALL)
        .title(i18n.t("adapter_detail_title"));

    if let Some(index) = adapter_module.state.selected() {
        if let Some(iface) = adapter_module.interfaces.get(index) {
            let stats = adapter_module.traffic_history.get(&iface.name);
            let table = render_detail_table(i18n, iface, stats);
            f.render_widget(table.block(block), chunks[1]);
        } else {
            f.render_widget(Paragraph::new("Select an adapter").block(block), chunks[1]);
        }
    } else {
        f.render_widget(
            Paragraph::new("No adapter selected").block(block),
            chunks[1],
        );
    }
}

/// 生成规整的详情表格
fn render_detail_table<'a>(
    i18n: &'a I18n,
    iface: &'a InterfaceInfo,
    stats: Option<&TrafficStats>,
) -> Table<'a> {
    // 样式定义
    let key_style = Style::default().fg(Color::Gray);
    let val_style = Style::default().fg(Color::White);
    let val_highlight = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut rows = Vec::new();

    // 1. 基础信息行 (Name & Desc)
    // 名称较长时，这里单独作为一行高亮显示可能更好，或者放入表格第一行
    // 这里选择放入表格，作为 "Name" 字段
    rows.push(
        Row::new(vec![
            Cell::from(Span::styled("Name / Desc", key_style)),
            Cell::from(vec![
                Line::from(Span::styled(&iface.name, val_highlight)),
                Line::from(Span::styled(
                    &iface.description,
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
        ])
        .height(2),
    ); // 给描述留2行高度，防止过紧

    // 2. 状态 & SSID
    let status_str = if iface.is_up {
        i18n.t("adapter_status_up")
    } else {
        i18n.t("adapter_status_down")
    };
    let status_color = if iface.is_up {
        Color::Green
    } else {
        Color::Red
    };

    let ssid_str = if let Some(ssid) = &iface.ssid {
        ssid.as_str()
    } else {
        "-"
    };
    let ssid_style = if iface.ssid.is_some() {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_status"), key_style)),
        Cell::from(Line::from(vec![
            Span::styled(status_str, Style::default().fg(status_color)),
            Span::raw("  (SSID: "),
            Span::styled(ssid_str, ssid_style),
            Span::raw(")"),
        ])),
    ]));

    // 3. 连接类型
    let conn_type = if iface.is_physical {
        i18n.t("adapter_conn_physical")
    } else {
        i18n.t("adapter_conn_virtual")
    };
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_conn_type"), key_style)),
        Cell::from(format!("{} [{}]", conn_type, iface.interface_type)),
    ]));

    // 4. IP 分配方式
    let ip_type = if iface.dhcp_enabled {
        i18n.t("adapter_type_dhcp")
    } else {
        i18n.t("adapter_type_static")
    };
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_ip_type"), key_style)),
        Cell::from(ip_type),
    ]));

    // 5. MAC 地址
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_mac"), key_style)),
        Cell::from(iface.mac.as_str()),
    ]));

    // 6. IPv4 (支持多行)
    let ipv4_content = if iface.ipv4.is_empty() {
        vec![Line::from("-")]
    } else {
        iface
            .ipv4
            .iter()
            .map(|ip| Line::from(format!("• {}", ip)))
            .collect()
    };
    rows.push(
        Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_ipv4"), key_style)),
            Cell::from(ipv4_content),
        ])
        .height(iface.ipv4.len().max(1) as u16),
    ); // 自动调整高度

    // 7. IPv6
    let ipv6_content = if iface.ipv6.is_empty() {
        vec![Line::from("-")]
    } else {
        iface
            .ipv6
            .iter()
            .map(|ip| Line::from(format!("• {}", ip)))
            .collect()
    };
    rows.push(
        Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_ipv6"), key_style)),
            Cell::from(ipv6_content),
        ])
        .height(iface.ipv6.len().max(1) as u16),
    );

    // 8. 实时速率 (分割线后)
    // 插入一个空行作为视觉分隔
    rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));

    if let Some(s) = stats {
        rows.push(Row::new(vec![
            Cell::from(Span::styled("Traffic Rate", key_style)),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("↓ {:<10}", format_speed(s.rx_speed)),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    format!("↑ {:<10}", format_speed(s.tx_speed)),
                    Style::default().fg(Color::Yellow),
                ),
            ])),
        ]));
        rows.push(Row::new(vec![
            Cell::from(Span::styled("Total Data", key_style)),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("Rx: {:<10}", format_bytes(s.total_rx)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("Tx: {:<10}", format_bytes(s.total_tx)),
                    Style::default().fg(Color::White),
                ),
            ])),
        ]));
    } else {
        rows.push(Row::new(vec![
            Cell::from(Span::styled("Traffic", key_style)),
            Cell::from(Span::styled(
                "Calculating...",
                Style::default().fg(Color::DarkGray),
            )),
        ]));
    }

    // 构建表格
    Table::new(
        rows,
        [
            Constraint::Length(16), // 标签列固定宽度
            Constraint::Min(0),     // 内容列自适应
        ],
    )
    .column_spacing(1)
    .style(val_style)
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
        format!("{:.2} GB", kb / 1024.0 / 1024.0)
    }
}
