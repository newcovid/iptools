use crate::app::App;
use crate::utils::net;
use crossterm::event::{KeyCode, KeyEvent};
use futures::{stream, StreamExt};
use ipnetwork::Ipv4Network;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, TableState},
};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub hostname: String,
}

#[derive(PartialEq, Clone, Copy)]
enum ScanStatus {
    Idle,
    Scanning,
    Done,
}

pub struct ScannerModule {
    cidr_input: String,
    input_mode: bool,
    results: Vec<ScanResult>,
    status: ScanStatus,
    table_state: TableState,

    total_scan_count: u64,
    current_scan_count: Arc<AtomicU64>,

    tx: mpsc::Sender<ScanResult>,
    rx: mpsc::Receiver<ScanResult>,
    abort_flag: Arc<Mutex<bool>>,
}

impl ScannerModule {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);

        let mut default_cidr = "192.168.1.0/24".to_string();
        let interfaces = net::get_interfaces();
        if let Some(iface) = interfaces
            .iter()
            .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
        {
            if let Some(first_ip) = iface.ipv4.first() {
                if let Ok(ip) = first_ip.parse::<Ipv4Addr>() {
                    if let Ok(net) = Ipv4Network::new(ip, 24) {
                        default_cidr = net.to_string();
                    }
                }
            }
        }

        Self {
            cidr_input: default_cidr,
            input_mode: false,
            results: Vec::new(),
            status: ScanStatus::Idle,
            table_state: TableState::default(),
            total_scan_count: 0,
            current_scan_count: Arc::new(AtomicU64::new(0)),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, concurrency: usize) {
        if self.input_mode {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    self.input_mode = false;
                }
                KeyCode::Backspace => {
                    self.cidr_input.pop();
                }
                KeyCode::Char(c) => {
                    if c.is_ascii_digit() || c == '.' || c == '/' {
                        self.cidr_input.push(c);
                    }
                }
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Char('e') => {
                    self.input_mode = true;
                }
                KeyCode::Enter => {
                    if self.status != ScanStatus::Scanning {
                        self.start_scan(concurrency);
                    }
                }
                KeyCode::Char('s') => {
                    if self.status == ScanStatus::Scanning {
                        *self.abort_flag.lock().unwrap() = true;
                        self.status = ScanStatus::Done;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => self.next(),
                KeyCode::Up | KeyCode::Char('k') => self.previous(),
                _ => {}
            }
        }
    }

    fn start_scan(&mut self, concurrency: usize) {
        self.results.clear();
        self.status = ScanStatus::Scanning;
        self.table_state.select(None);

        self.current_scan_count.store(0, Ordering::Relaxed);
        let current_count_arc = self.current_scan_count.clone();

        *self.abort_flag.lock().unwrap() = false;
        let abort_flag = self.abort_flag.clone();
        let tx = self.tx.clone();
        let cidr_str = self.cidr_input.clone();

        if let Ok(network) = self.cidr_input.parse::<Ipv4Network>() {
            let count = if network.prefix() < 31 {
                network.size().saturating_sub(2)
            } else {
                network.size()
            };
            self.total_scan_count = count as u64;
        }

        tokio::spawn(async move {
            if let Ok(network) = cidr_str.parse::<Ipv4Network>() {
                let ips: Vec<Ipv4Addr> = if network.prefix() < 31 {
                    network
                        .iter()
                        .skip(1)
                        .take(network.size().saturating_sub(2) as usize)
                        .collect()
                } else {
                    network.iter().collect()
                };

                let stream = stream::iter(ips)
                    .map(|ip| {
                        let abort_inner = abort_flag.clone();
                        let counter_clone = current_count_arc.clone();
                        let tx_inner = tx.clone();

                        async move {
                            if *abort_inner.lock().unwrap() {
                                return;
                            }

                            let mac_opt = tokio::task::spawn_blocking(move || {
                                #[cfg(target_os = "windows")]
                                {
                                    net::resolve_mac_address(ip)
                                }
                                #[cfg(not(target_os = "windows"))]
                                {
                                    None
                                }
                            })
                            .await
                            .unwrap_or(None);

                            if let Some(mac) = mac_opt {
                                if *abort_inner.lock().unwrap() {
                                    return;
                                }

                                let host_opt = tokio::task::spawn_blocking(move || {
                                    net::resolve_hostname(std::net::IpAddr::V4(ip))
                                })
                                .await
                                .unwrap_or(None);

                                let hostname = host_opt.unwrap_or_default();

                                let _ = tx_inner.send(ScanResult { ip, mac, hostname }).await;
                            }

                            if !*abort_inner.lock().unwrap() {
                                counter_clone.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    })
                    .buffer_unordered(concurrency);

                stream.collect::<Vec<()>>().await;
            }
        });
    }

    pub fn update(&mut self) {
        let mut new_items_count = 0;
        while let Ok(res) = self.rx.try_recv() {
            self.results.push(res);
            new_items_count += 1;
        }

        if new_items_count > 0 {
            self.results.sort_by(|a, b| a.ip.cmp(&b.ip));
            if self.table_state.selected().is_none() && !self.results.is_empty() {
                self.table_state.select(Some(0));
            }
        }

        if self.status == ScanStatus::Scanning {
            let current = self.current_scan_count.load(Ordering::Relaxed);
            if self.total_scan_count > 0 && current >= self.total_scan_count {
                self.status = ScanStatus::Done;
            }
            if *self.abort_flag.lock().unwrap() {
                self.status = ScanStatus::Done;
            }
        }
    }

    fn next(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.results.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn previous(&mut self) {
        if self.results.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.results.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }

    fn calculate_ip_count(&self) -> String {
        if let Ok(network) = self.cidr_input.parse::<Ipv4Network>() {
            let count = if network.prefix() < 31 {
                network.size().saturating_sub(2)
            } else {
                network.size()
            };
            count.to_string()
        } else {
            "-".to_string()
        }
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let i18n = &app.i18n;
    let scanner = &mut app.scanner;

    // --- 1. 顶部控制栏 ---
    let input_style = if scanner.input_mode {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };

    let status_text = match scanner.status {
        ScanStatus::Idle => i18n.t("scan_status_idle"),
        ScanStatus::Scanning => i18n.t("scan_status_scanning"),
        ScanStatus::Done => i18n.t("scan_status_done"),
    };

    let count_text = scanner.calculate_ip_count();

    let control_text = format!(
        " {} {}   {} {}   [{}]   {} / {} / {}",
        i18n.t("scan_range_label"),
        scanner.cidr_input,
        i18n.t("scan_count_label"),
        count_text,
        status_text,
        i18n.t("scan_btn_edit"),
        i18n.t("scan_btn_start"),
        i18n.t("scan_btn_stop")
    );

    let control_block = Paragraph::new(control_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("scan_title")),
        )
        .style(input_style);

    f.render_widget(control_block, chunks[0]);

    if scanner.input_mode {
        let label_str = i18n.t("scan_range_label");
        let prefix_width = label_str.width();
        let x_offset = 1 + 1 + prefix_width + 1 + scanner.cidr_input.width();

        f.set_cursor_position(Position::new(
            chunks[0].x + x_offset as u16,
            chunks[0].y + 1,
        ));
    }

    // --- 2. 结果列表 ---

    // 技巧：表头第一列固定加3个空格，和内容的 `   ` / `>> ` 保持长度一致
    let col_ip_header = format!("   {}", i18n.t("scan_col_ip"));

    let header_cells = vec![
        Cell::from(col_ip_header).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("scan_col_mac")).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("scan_col_host")).style(Style::default().fg(Color::Gray)),
    ];

    let header = Row::new(header_cells).height(1).bottom_margin(1);

    // 获取当前选中行索引，用于手动渲染高亮符号
    let selected_index = scanner.table_state.selected();

    let rows = scanner.results.iter().enumerate().map(|(i, item)| {
        let is_selected = Some(i) == selected_index;

        // 关键：手动处理高亮符号的文本
        // 如果选中，显示 ">> "，如果未选中，显示 "   "
        // 这样第一列的实际数据起始位置永远固定，不会跳变
        let prefix = if is_selected { ">> " } else { "   " };
        let ip_text = format!("{}{}", prefix, item.ip);

        // 如果选中，文字变青色；背景色交给 highlight_style 处理
        let text_color = if is_selected {
            Color::Cyan
        } else {
            Color::White
        };

        let cells = vec![
            Cell::from(ip_text).style(Style::default().fg(text_color)),
            Cell::from(item.mac.clone()).style(Style::default().fg(text_color)),
            Cell::from(item.hostname.clone()).style(Style::default().fg(text_color)),
        ];

        Row::new(cells).height(1)
    });

    let t = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(35),
            Constraint::Percentage(40),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(format!(
        "{} ({})",
        i18n.t("scan_devices_found"),
        scanner.results.len()
    )))
    // 使用标准的高亮样式 (背景深灰)，这与其他页面风格统一
    .highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    // 关键：将 highlight_symbol 设为空，因为我们已经手动在内容里加了 ">> "
    .highlight_symbol("");

    f.render_stateful_widget(t, chunks[1], &mut scanner.table_state);

    // --- 3. 进度条 ---
    if scanner.status == ScanStatus::Scanning || scanner.status == ScanStatus::Done {
        let current = scanner.current_scan_count.load(Ordering::Relaxed) as f64;
        let total = scanner.total_scan_count as f64;
        let ratio = if total > 0.0 {
            (current / total).min(1.0)
        } else {
            0.0
        };

        let label = format!("{:.1}% ({}/{})", ratio * 100.0, current, total);

        let color = if scanner.status == ScanStatus::Done && ratio < 1.0 {
            Color::Yellow
        } else {
            Color::Cyan
        };

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(Style::default().fg(color).bg(Color::DarkGray))
            .ratio(ratio)
            .label(label);

        f.render_widget(gauge, chunks[2]);
    }
}
