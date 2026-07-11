use crate::app::App;
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::modules::adapter_edit::{EditForm, EditOutcome};
use crate::session::AdapterEditPersist;
use crate::ui::theme; // 引入主题
use crate::utils::format::{format_bytes, format_speed};
use crate::utils::i18n::I18n;
use crate::utils::net::{self, InterfaceInfo};
use crossterm::event::KeyEvent;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table},
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use sysinfo::Networks;
use tokio::sync::mpsc;

/// 后台重新枚举网卡的最小间隔——足够快地反映 USB 网卡插拔 / 链路启停，
/// 又不至于每个 tick 都做一次系统 API 枚举。
const IFACE_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

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
    /// 进入编辑态时为 Some；只读视图时为 None。
    pub edit: Option<EditForm>,
    /// 持久化的适配器编辑参数（按网卡 GUID 保存）。
    persist: AdapterEditPersist,
    /// 适配器编辑 MRU 历史
    pub history: Rc<RefCell<HistoryStore>>,
    /// 后台网卡枚举结果回传通道（遵循模块异步契约：spawn → mpsc → update 消费）。
    iface_tx: mpsc::UnboundedSender<Vec<InterfaceInfo>>,
    iface_rx: mpsc::UnboundedReceiver<Vec<InterfaceInfo>>,
    /// 防止多个后台枚举任务叠加。
    scan_in_flight: Arc<AtomicBool>,
    /// 上次发起后台枚举的时刻，用于节流。
    last_scan: Instant,
}

impl AdapterModule {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
        let mut state = ListState::default();
        let interfaces = net::get_interfaces();
        if !interfaces.is_empty() {
            state.select(Some(0));
        }

        let (iface_tx, iface_rx) = mpsc::unbounded_channel();

        Self {
            state,
            interfaces,
            networks: Networks::new_with_refreshed_list(),
            traffic_history: HashMap::new(),
            edit: None,
            persist: AdapterEditPersist::default(),
            history,
            iface_tx,
            iface_rx,
            scan_in_flight: Arc::new(AtomicBool::new(false)),
            last_scan: Instant::now(),
        }
    }

    /// 初始化持久化数据（从配置加载）。
    pub fn apply_persist(&mut self, persist: &AdapterEditPersist) {
        self.persist = persist.clone();
    }

    /// 导出当前持久化快照（用于保存到配置）。
    pub fn export_persist(&self) -> AdapterEditPersist {
        // 如果当前有编辑态，更新对应网卡的参数
        if let Some(form) = &self.edit {
            let guid = form.guid().to_string();
            let params = form.export_persist();
            let mut persist = self.persist.clone();
            persist.adapters.insert(guid, params);
            persist
        } else {
            self.persist.clone()
        }
    }

    pub fn update(&mut self) {
        if let Some(form) = &mut self.edit {
            form.update();
        }

        // 消费后台枚举结果：状态/插拔变化会在这里自动并入 UI。
        while let Ok(list) = self.iface_rx.try_recv() {
            self.apply_interface_update(list);
        }

        // 周期性地在后台线程重新枚举网卡，自动反映活跃/不活跃切换与插拔。
        // 编辑态下暂停，避免列表在用户编辑时变动导致索引错位。
        if self.edit.is_none()
            && !self.scan_in_flight.load(Ordering::Relaxed)
            && self.last_scan.elapsed() >= IFACE_REFRESH_INTERVAL
        {
            self.last_scan = Instant::now();
            self.scan_in_flight.store(true, Ordering::Relaxed);
            let tx = self.iface_tx.clone();
            let flag = self.scan_in_flight.clone();
            // get_interfaces 是同步 FFI，放进阻塞线程池，绝不阻塞渲染线程。
            tokio::task::spawn_blocking(move || {
                let list = net::get_interfaces();
                let _ = tx.send(list);
                flag.store(false, Ordering::Relaxed);
            });
        }

        self.networks.refresh(true);
        let now = Instant::now();

        for (name, data) in &self.networks {
            let rx = data.total_received();
            let tx = data.total_transmitted();

            if let Some(prev) = self.traffic_history.get_mut(name) {
                let duration = now.duration_since(prev.last_update).as_secs_f64();
                if duration > 0.1 {
                    // saturating_sub：避免计数器回绕时下溢 panic
                    prev.rx_speed = (rx.saturating_sub(prev.total_rx) as f64 / duration) as u64;
                    prev.tx_speed = (tx.saturating_sub(prev.total_tx) as f64 / duration) as u64;
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

    /// 手动刷新（'r' 键）：同步枚举一次，立即反馈。
    pub fn reload(&mut self) {
        // 立刻拿到结果，同时重置节流计时器，避免随后又马上后台再扫一次。
        self.last_scan = Instant::now();
        let list = net::get_interfaces();
        self.apply_interface_update(list);
    }

    /// 用新枚举结果替换网卡列表，并尽量保留当前选中项（按 GUID，退化按名称）。
    /// 后台自动刷新与手动刷新共用，确保选中焦点在插拔/重排后不丢失。
    fn apply_interface_update(&mut self, new_list: Vec<InterfaceInfo>) {
        let selected_key = self
            .state
            .selected()
            .and_then(|i| self.interfaces.get(i))
            .map(|i| (i.guid.clone(), i.name.clone()));

        self.interfaces = new_list;

        if self.interfaces.is_empty() {
            self.state.select(None);
            return;
        }

        let new_idx = selected_key.and_then(|(guid, name)| {
            self.interfaces
                .iter()
                .position(|i| (!guid.is_empty() && i.guid == guid) || i.name == name)
        });

        self.state.select(Some(new_idx.unwrap_or(0)));
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>) {
        // 编辑态：所有按键交给表单处理
        if let Some(form) = &mut self.edit {
            match form.on_key(key, action) {
                EditOutcome::Stay => {}
                EditOutcome::Cancel => self.edit = None,
                EditOutcome::Done => {
                    self.edit = None;
                    self.reload();
                }
            }
            return;
        }

        // 只读态。本页唯一的主操作就是「编辑选中网卡的 IP」，因此在不产生歧义的
        // 前提下，Edit(e)/Confirm(Enter)/Toggle(Space) 都触发它，符合直觉。
        match action {
            Some(Action::Down) => self.next(),
            Some(Action::Up) => self.previous(),
            Some(Action::Refresh) => self.reload(),
            Some(Action::Edit) | Some(Action::Confirm) | Some(Action::Toggle) => self.enter_edit(),
            _ => {}
        }
    }

    fn enter_edit(&mut self) {
        if let Some(idx) = self.state.selected()
            && let Some(iface) = self.interfaces.get(idx)
        {
            // 优先使用持久化数据，否则使用系统当前状态
            let form = if let Some(params) = self.persist.adapters.get(&iface.guid) {
                EditForm::from_persist(iface, params, self.history.clone())
            } else {
                EditForm::from_interface(iface, self.history.clone())
            };
            self.edit = Some(form);
        }
    }

    /// 鼠标：点击只读列表第 `row` 行选中该网卡。
    pub fn click_list(&mut self, row: usize) {
        if row < self.interfaces.len() {
            self.state.select(Some(row));
        }
    }

    /// 鼠标：点击编辑表单，选中对应字段并（文本字段）定位光标。
    pub fn click_edit(&mut self, x: u16, y: u16, area: Rect) {
        if let Some(form) = &mut self.edit {
            form.click(x, y, area);
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
    // 动态取「编辑」动作的绑定键，避免写死 'e'（用户可在 config 改键）。
    let edit_label = app.keymap.primary_label(Action::Edit);

    // 登记鼠标可点击区域（编辑态登记字段区，只读态登记网卡列表区）。
    if app.adapter.edit.is_some() {
        app.mouse.adapter_edit = Some(EditForm::field_list_rect(area));
    } else {
        let list_area = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(area)[0];
        app.mouse.adapter_list = Some(Block::default().borders(Borders::ALL).inner(list_area));
    }

    let i18n = &app.i18n;
    let keymap = &app.keymap;
    let adapter_module = &mut app.adapter;

    // 编辑态：整块区域交给编辑表单
    if let Some(form) = &adapter_module.edit {
        form.draw(f, area, i18n, keymap);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    let items: Vec<ListItem> = adapter_module
        .interfaces
        .iter()
        .map(|iface| {
            let color = if iface.is_up {
                Color::White
            } else {
                Color::DarkGray
            };
            let prefix = if iface.is_physical { "[P] " } else { "[V] " };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(theme::COLOR_SECONDARY)), // Use Theme Cyan
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
        .highlight_style(Style::default().bg(theme::COLOR_HIGHLIGHT_BG)) // Use Theme
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[0], &mut adapter_module.state);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(i18n.t("adapter_detail_title"))
        .title(
            Line::from(Span::styled(
                format!(" [{}] {} ", edit_label, i18n.t("adapter_edit_enter")),
                Style::default().fg(theme::COLOR_SECONDARY),
            ))
            .alignment(Alignment::Right),
        );

    if let Some(index) = adapter_module.state.selected() {
        if let Some(iface) = adapter_module.interfaces.get(index) {
            let stats = adapter_module.traffic_history.get(&iface.name);
            let table = render_detail_table(i18n, iface, stats);
            f.render_widget(table.block(block), chunks[1]);
        } else {
            f.render_widget(
                Paragraph::new(i18n.t("adapter_select_hint")).block(block),
                chunks[1],
            );
        }
    } else {
        f.render_widget(
            Paragraph::new(i18n.t("adapter_none_selected")).block(block),
            chunks[1],
        );
    }
}

fn render_detail_table<'a>(
    i18n: &'a I18n,
    iface: &'a InterfaceInfo,
    stats: Option<&TrafficStats>,
) -> Table<'a> {
    let key_style = Style::default().fg(theme::COLOR_SUBTEXT);
    let val_style = Style::default().fg(Color::White);
    let val_highlight = Style::default()
        .fg(theme::COLOR_SECONDARY)
        .add_modifier(Modifier::BOLD);

    let mut rows = Vec::new();

    rows.push(
        Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_name_desc"), key_style)),
            Cell::from(vec![
                Line::from(Span::styled(&iface.name, val_highlight)),
                Line::from(Span::styled(
                    &iface.description,
                    Style::default().fg(Color::DarkGray),
                )),
            ]),
        ])
        .height(2),
    );

    let status_str = if iface.is_up {
        i18n.t("adapter_status_up")
    } else {
        i18n.t("adapter_status_down")
    };
    // Use Theme Colors
    let status_color = if iface.is_up {
        theme::COLOR_UP
    } else {
        theme::COLOR_DOWN
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

    let conn_type = if iface.is_physical {
        i18n.t("adapter_conn_physical")
    } else {
        i18n.t("adapter_conn_virtual")
    };
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_conn_type"), key_style)),
        Cell::from(format!("{} [{}]", conn_type, iface.interface_type)),
    ]));

    let ip_type = if iface.dhcp_enabled {
        i18n.t("adapter_type_dhcp")
    } else {
        i18n.t("adapter_type_static")
    };
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_ip_type"), key_style)),
        Cell::from(ip_type),
    ]));

    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("adapter_mac"), key_style)),
        Cell::from(iface.mac.as_str()),
    ]));

    // 修复：利用 cidr 字段优化显示
    let ipv4_content = if iface.ipv4.is_empty() {
        vec![Line::from("-")]
    } else {
        iface
            .ipv4
            .iter()
            .map(|ip| {
                // 如果 cidr 存在且当前 ip 包含在 cidr 字符串中 (简单匹配)，则显示 cidr
                // 否则显示 ip
                let display_text = if let Some(cidr) = &iface.cidr {
                    if cidr.starts_with(ip) {
                        format!("• {}", cidr) // 显示 "• 192.168.1.100/24"
                    } else {
                        format!("• {}", ip)
                    }
                } else {
                    format!("• {}", ip)
                };
                Line::from(display_text)
            })
            .collect()
    };
    rows.push(
        Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_ipv4"), key_style)),
            Cell::from(ipv4_content),
        ])
        .height(iface.ipv4.len().max(1) as u16),
    );

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

    rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));

    if let Some(s) = stats {
        rows.push(Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_traffic_rate"), key_style)),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("↓ {:<10}", format_speed(s.rx_speed)),
                    Style::default().fg(theme::COLOR_UP),
                ), // Green
                Span::styled(
                    format!("↑ {:<10}", format_speed(s.tx_speed)),
                    Style::default().fg(Color::Yellow),
                ),
            ])),
        ]));
        rows.push(Row::new(vec![
            Cell::from(Span::styled(i18n.t("adapter_traffic_total"), key_style)),
            Cell::from(Line::from(vec![
                Span::styled(
                    format!("{}: {:<10}", i18n.t("dash_rx"), format_bytes(s.total_rx)),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{}: {:<10}", i18n.t("dash_tx"), format_bytes(s.total_tx)),
                    Style::default().fg(Color::White),
                ),
            ])),
        ]));
    } else {
        rows.push(Row::new(vec![
            Cell::from(Span::styled(i18n.t("common_traffic"), key_style)),
            Cell::from(Span::styled(
                i18n.t("adapter_traffic_calc"),
                Style::default().fg(Color::DarkGray),
            )),
        ]));
    }

    Table::new(rows, [Constraint::Length(16), Constraint::Min(0)])
        .column_spacing(1)
        .style(val_style)
}
