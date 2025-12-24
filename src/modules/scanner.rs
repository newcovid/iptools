use crate::app::App;
use crate::utils::net;
use crossterm::event::{KeyCode, KeyEvent};
use ipnetwork::Ipv4Network;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct ScanResult {
    pub ip: Ipv4Addr,
    pub mac: String,
    pub hostname: String,
}

#[derive(PartialEq)]
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
            if let Some(cidr) = &iface.cidr {
                default_cidr = cidr.clone();
            }
        }

        Self {
            cidr_input: default_cidr,
            input_mode: false,
            results: Vec::new(),
            status: ScanStatus::Idle,
            table_state: TableState::default(),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, concurrency: usize) {
        if self.input_mode {
            match key.code {
                KeyCode::Enter => {
                    self.input_mode = false;
                    self.start_scan(concurrency);
                }
                KeyCode::Esc => {
                    self.input_mode = false;
                }
                KeyCode::Backspace => {
                    self.cidr_input.pop();
                }
                KeyCode::Char(c) => {
                    self.cidr_input.push(c);
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

        *self.abort_flag.lock().unwrap() = false;
        let abort_flag = self.abort_flag.clone();
        let tx = self.tx.clone();
        let cidr_str = self.cidr_input.clone();

        tokio::spawn(async move {
            if let Ok(network) = cidr_str.parse::<Ipv4Network>() {
                // 修复 panic: 使用更安全的逻辑生成 IP 列表
                let ips: Vec<Ipv4Addr> = if network.prefix() < 31 {
                    // 常规网段 (如 /24)，跳过网络号和广播地址 (首尾)
                    // size() - 2 有可能溢出 (虽然这里判了 < 31，但保险起见)
                    // 其实 skip(1) 和 take(size - 2) 已经够了，重点是 size 必须足够大
                    network
                        .iter()
                        .skip(1)
                        .take(network.size().saturating_sub(2) as usize)
                        .collect()
                } else {
                    // /31 (点对点) 或 /32 (单机)，全部扫描
                    network.iter().collect()
                };

                let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

                let mut handles = Vec::new();

                for ip in ips {
                    if *abort_flag.lock().unwrap() {
                        break;
                    }

                    let tx_thread = tx.clone();
                    let sem_clone = semaphore.clone();

                    let handle = tokio::spawn(async move {
                        let _permit = sem_clone.acquire().await.unwrap();

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
                        .unwrap();

                        if let Some(mac) = mac_opt {
                            let host_opt = tokio::task::spawn_blocking(move || {
                                net::resolve_hostname(std::net::IpAddr::V4(ip))
                            })
                            .await
                            .unwrap();

                            let hostname = host_opt.unwrap_or_default();

                            let _ = tx_thread.send(ScanResult { ip, mac, hostname }).await;
                        }
                    });
                    handles.push(handle);
                }

                for h in handles {
                    let _ = h.await;
                }
            }
        });
    }

    pub fn update(&mut self) {
        while let Ok(res) = self.rx.try_recv() {
            self.results.push(res);
            self.results.sort_by(|a, b| a.ip.cmp(&b.ip));
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

    // 辅助函数：计算当前输入对应的 IP 数量
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
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let i18n = &app.i18n;
    let scanner = &mut app.scanner;

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

    // 计算预计数量
    let count_text = scanner.calculate_ip_count();

    // 格式： [Scan Range]: 192.168.1.0/24   [Count]: 254   [Status]   Start / Stop
    let control_text = format!(
        " {} {}   {} {}   [{}]   {} / {}",
        i18n.t("scan_range_label"),
        scanner.cidr_input,
        i18n.t("scan_count_label"),
        count_text,
        status_text,
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

    let header_labels = vec![
        i18n.t("scan_col_ip"),
        i18n.t("scan_col_mac"),
        i18n.t("scan_col_host"),
    ];

    let header_cells = header_labels
        .iter()
        .map(|h| Cell::from(h.as_str()).style(Style::default().fg(Color::Gray)));

    let header = Row::new(header_cells).height(1).bottom_margin(1);

    let rows = scanner.results.iter().map(|item| {
        let cells = vec![
            Cell::from(item.ip.to_string()),
            Cell::from(item.mac.clone()),
            Cell::from(item.hostname.clone()),
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
    .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
    .highlight_symbol(">> ");

    f.render_stateful_widget(t, chunks[1], &mut scanner.table_state);
}
