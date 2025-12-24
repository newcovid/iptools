use crate::app::App;
use crate::utils::i18n::I18n;
use crate::utils::net::{self, InterfaceInfo};
use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use serde::Deserialize;
use std::env;
use std::time::Instant;
use sysinfo::{Networks, System};
use tokio::sync::mpsc;

struct DashTrafficStats {
    total_rx: u64,
    total_tx: u64,
    last_update: Instant,
    rx_speed: u64,
    tx_speed: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct IpApiResponse {
    #[serde(rename = "query")]
    ip: String,
    country: String,
    #[serde(rename = "regionName")]
    region: String,
    city: String,
    isp: String,
}

#[derive(Debug, Clone)]
enum PublicInfoState {
    Loading,
    Success(IpApiResponse),
    Failed(String, String),
}

pub struct Dashboard {
    // 系统信息
    hostname: String,
    os_name: String,
    os_version: String,

    // 网络信息
    sys_networks: Networks,
    active_interface: Option<InterfaceInfo>,
    traffic_stats: Option<DashTrafficStats>,

    // 公网信息
    public_info: PublicInfoState,
    proxy_setting: Option<String>,
    rx: mpsc::Receiver<Result<IpApiResponse, (String, String)>>,
    tx: mpsc::Sender<Result<IpApiResponse, (String, String)>>,
}

impl Dashboard {
    pub fn new() -> Self {
        let hostname = System::host_name().unwrap_or_default();
        let os_name = System::name().unwrap_or_default();
        let os_version = System::os_version().unwrap_or_default();

        let (tx, rx) = mpsc::channel(10);

        let mut dashboard = Self {
            hostname,
            os_name,
            os_version,
            sys_networks: Networks::new_with_refreshed_list(),
            active_interface: None,
            traffic_stats: None,
            public_info: PublicInfoState::Loading,
            proxy_setting: None,
            rx,
            tx,
        };

        dashboard.detect_proxy();
        dashboard.refresh_active_interface();

        // 修复 1: 移除默认的英文请求，避免启动时闪烁
        // 这里什么都不做，等待 App 初始化完成后调用 fetch_public_ip

        dashboard
    }

    fn detect_proxy(&mut self) {
        let env_proxy = env::var("HTTP_PROXY")
            .or_else(|_| env::var("http_proxy"))
            .or_else(|_| env::var("HTTPS_PROXY"))
            .or_else(|_| env::var("https_proxy"))
            .ok();

        if let Some(p) = env_proxy {
            self.proxy_setting = Some(p);
            return;
        }

        #[cfg(target_os = "windows")]
        {
            use winreg::enums::*;
            use winreg::RegKey;

            self.proxy_setting = None;
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok(settings) =
                hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            {
                let enabled: u32 = settings.get_value("ProxyEnable").unwrap_or(0);
                if enabled == 1 {
                    if let Ok(server) = settings.get_value::<String, _>("ProxyServer") {
                        if !server.is_empty() {
                            self.proxy_setting = Some(server);
                        }
                    }
                }
            }
        }
    }

    pub fn fetch_public_ip(&mut self, lang_code: &str) {
        self.public_info = PublicInfoState::Loading;
        let tx = self.tx.clone();

        let api_lang = if lang_code.starts_with("zh") {
            "zh-CN"
        } else {
            "en"
        };
        let url = format!("http://ip-api.com/json/?lang={}", api_lang);

        tokio::spawn(async move {
            let client_builder = reqwest::Client::builder().no_proxy();

            let client = match client_builder.build() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx
                        .send(Err(("dash_err_req".to_string(), e.to_string())))
                        .await;
                    return;
                }
            };

            let result = match client.get(&url).send().await {
                Ok(resp) => match resp.json::<IpApiResponse>().await {
                    Ok(info) => Ok(info),
                    Err(e) => Err(("dash_err_parse".to_string(), e.to_string())),
                },
                Err(e) => Err(("dash_err_req".to_string(), e.to_string())),
            };

            let _ = tx.send(result).await;
        });
    }

    fn refresh_active_interface(&mut self) {
        let mut interfaces = net::get_interfaces();
        interfaces.sort_by(|a, b| {
            let score_a = score_interface(a);
            let score_b = score_interface(b);
            score_b.cmp(&score_a)
        });

        if let Some(best) = interfaces.first() {
            if self.active_interface.as_ref().map(|i| &i.name) != Some(&best.name) {
                self.traffic_stats = None;
            }
            self.active_interface = Some(best.clone());
        }
    }

    pub fn update(&mut self) {
        if let Ok(result) = self.rx.try_recv() {
            self.public_info = match result {
                Ok(info) => PublicInfoState::Success(info),
                Err((key, debug)) => PublicInfoState::Failed(key, debug),
            };
        }

        self.sys_networks.refresh(true);

        if let Some(iface) = &self.active_interface {
            if let Some(net_data) = self.sys_networks.iter().find(|(n, _)| **n == iface.name) {
                let (_, data) = net_data;
                let now = Instant::now();
                let rx = data.total_received();
                let tx = data.total_transmitted();

                if let Some(stats) = &mut self.traffic_stats {
                    let duration = now.duration_since(stats.last_update).as_secs_f64();
                    if duration > 0.5 {
                        stats.rx_speed = ((rx - stats.total_rx) as f64 / duration) as u64;
                        stats.tx_speed = ((tx - stats.total_tx) as f64 / duration) as u64;
                        stats.total_rx = rx;
                        stats.total_tx = tx;
                        stats.last_update = now;
                    }
                } else {
                    self.traffic_stats = Some(DashTrafficStats {
                        total_rx: rx,
                        total_tx: tx,
                        last_update: now,
                        rx_speed: 0,
                        tx_speed: 0,
                    });
                }
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, current_lang: &str) {
        match key.code {
            KeyCode::Char('r') => {
                self.refresh_active_interface();
                self.detect_proxy();
                self.fetch_public_ip(current_lang);
            }
            _ => {}
        }
    }
}

fn score_interface(iface: &InterfaceInfo) -> u8 {
    let mut score = 0;
    if iface.is_up {
        score += 10;
    }
    if iface.is_physical {
        score += 5;
    }
    if !iface.ipv4.is_empty() {
        score += 5;
    }
    if iface.dhcp_enabled {
        score += 1;
    }
    score
}

pub fn draw(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let i18n = &app.i18n;
    let dash = &app.dashboard;

    draw_local_panel(f, chunks[0], dash, i18n);
    draw_public_panel(f, chunks[1], dash, i18n);
}

fn draw_local_panel(f: &mut Frame, area: Rect, dash: &Dashboard, i18n: &I18n) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(i18n.t("dashboard_local_title"));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let key_style = Style::default().fg(Color::Gray);
    let val_highlight = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut rows = Vec::new();

    let time_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("dash_time"), key_style)),
        Cell::from(time_str),
    ]));
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("label_hostname"), key_style)),
        Cell::from(format!("{} ({})", dash.hostname, dash.os_name)),
    ]));

    rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));

    if let Some(iface) = &dash.active_interface {
        let name_display = if let Some(ssid) = &iface.ssid {
            format!("{} (SSID: {})", iface.name, ssid)
        } else {
            iface.name.clone()
        };

        rows.push(
            Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_iface"), key_style)),
                Cell::from(vec![
                    Line::from(Span::styled(name_display, val_highlight)),
                    Line::from(Span::styled(
                        &iface.description,
                        Style::default().fg(Color::DarkGray),
                    )),
                ]),
            ])
            .height(2),
        );

        let conn_type = if iface.is_physical {
            i18n.t("adapter_conn_physical")
        } else {
            i18n.t("adapter_conn_virtual")
        };
        let ip_type = if iface.dhcp_enabled {
            i18n.t("adapter_type_dhcp")
        } else {
            i18n.t("adapter_type_static")
        };

        rows.push(Row::new(vec![
            Cell::from(Span::styled(i18n.t("dash_ip_detail"), key_style)),
            Cell::from(format!("{} / {}", conn_type, ip_type)),
        ]));

        let ip_display = iface
            .ipv4
            .first()
            .cloned()
            .unwrap_or_else(|| "-".to_string());
        rows.push(Row::new(vec![
            Cell::from(Span::styled(i18n.t("label_local_ip"), key_style)),
            Cell::from(ip_display),
        ]));

        rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));

        if let Some(stats) = &dash.traffic_stats {
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_speed"), key_style)),
                Cell::from(Line::from(vec![
                    Span::styled(
                        format!("↓ {:<9}", format_speed(stats.rx_speed)),
                        Style::default().fg(Color::Green),
                    ),
                    Span::styled(
                        format!("↑ {:<9}", format_speed(stats.tx_speed)),
                        Style::default().fg(Color::Yellow),
                    ),
                ])),
            ]));
            // 修复 2: 使用本地化的 dash_rx 和 dash_tx
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_usage"), key_style)),
                Cell::from(Line::from(vec![
                    Span::styled(
                        format!("{}: {:<9}", i18n.t("dash_rx"), format_bytes(stats.total_rx)),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("{}: {:<9}", i18n.t("dash_tx"), format_bytes(stats.total_tx)),
                        Style::default().fg(Color::White),
                    ),
                ])),
            ]));
        }
    } else {
        rows.push(Row::new(vec![
            Cell::from(Span::styled("Status", key_style)),
            Cell::from(Span::styled(
                "No active interface found",
                Style::default().fg(Color::Red),
            )),
        ]));
    }

    let table = Table::new(rows, [Constraint::Length(14), Constraint::Min(0)]).column_spacing(1);

    f.render_widget(table, inner_area);
}

fn draw_public_panel(f: &mut Frame, area: Rect, dash: &Dashboard, i18n: &I18n) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(i18n.t("dashboard_public_title"));

    let inner_area = block.inner(area);
    f.render_widget(block, area);

    let key_style = Style::default().fg(Color::Gray);
    let val_highlight = Style::default()
        .fg(Color::LightCyan)
        .add_modifier(Modifier::BOLD);

    let mut rows = Vec::new();

    let (proxy_str, proxy_color) = match &dash.proxy_setting {
        Some(p) => (p.clone(), Color::Yellow),
        None => (i18n.t("dash_proxy_none"), Color::DarkGray),
    };

    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("dash_proxy"), key_style)),
        Cell::from(Span::styled(proxy_str, Style::default().fg(proxy_color))),
    ]));

    rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));

    match &dash.public_info {
        PublicInfoState::Loading => {
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_public_ip"), key_style)),
                Cell::from(Span::styled(
                    i18n.t("dash_fetch_loading"),
                    Style::default().fg(Color::Yellow),
                )),
            ]));
        }
        PublicInfoState::Failed(key, debug_msg) => {
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_public_ip"), key_style)),
                Cell::from(Span::styled(
                    i18n.t("dash_fetch_failed"),
                    Style::default().fg(Color::Red),
                )),
            ]));
            rows.push(Row::new(vec![
                Cell::from(Span::styled("Type", key_style)),
                Cell::from(Span::styled(i18n.t(key), Style::default().fg(Color::Red))),
            ]));
            let safe_msg = if debug_msg.len() > 30 {
                format!("{}...", &debug_msg[..30])
            } else {
                debug_msg.clone()
            };
            rows.push(Row::new(vec![
                Cell::from(Span::styled("Debug", key_style)),
                Cell::from(Span::styled(safe_msg, Style::default().fg(Color::DarkGray))),
            ]));
        }
        PublicInfoState::Success(info) => {
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_public_ip"), key_style)),
                Cell::from(Span::styled(info.ip.clone(), val_highlight)),
            ]));

            let loc_str = format!("{}, {}, {}", info.city, info.region, info.country);
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_geo"), key_style)),
                Cell::from(loc_str),
            ]));

            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_isp"), key_style)),
                Cell::from(info.isp.clone()),
            ]));
        }
    }

    rows.push(Row::new(vec![Cell::from(""), Cell::from("")]).height(1));
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("dash_note"), key_style)),
        Cell::from(Span::styled(
            i18n.t("dash_note_text"),
            Style::default().fg(Color::DarkGray),
        )),
    ]));

    let table = Table::new(rows, [Constraint::Length(14), Constraint::Min(0)]).column_spacing(1);

    f.render_widget(table, inner_area);
}

fn format_speed(bps: u64) -> String {
    let kbps = bps as f64 / 1024.0;
    if kbps < 1024.0 {
        format!("{:.1} KB/s", kbps)
    } else {
        format!("{:.1} MB/s", kbps / 1024.0)
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
