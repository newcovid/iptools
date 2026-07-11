use crate::app::App;
use crate::config::Endpoint;
use crate::keymap::Action;
use crate::ui::theme; // 引入主题
use crate::utils::format::{format_bytes, format_speed};
use crate::utils::i18n::I18n;
use crate::utils::net::{self, InterfaceInfo};
use crate::utils::pubip::{self, PublicInfo};
use chrono::Local;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Row, Table},
};
use std::env;
use std::time::{Duration, Instant};
use sysinfo::{Networks, System};
use tokio::sync::mpsc;

struct DashTrafficStats {
    total_rx: u64,
    total_tx: u64,
    last_update: Instant,
    rx_speed: u64,
    tx_speed: u64,
}

#[derive(Debug, Clone)]
enum PublicInfoState {
    Loading,
    Success(PublicInfo),
    Failed(String, String),
}

pub struct Dashboard {
    hostname: String,
    os_name: String,
    os_version: String,
    sys_networks: Networks,
    active_interface: Option<InterfaceInfo>,
    traffic_stats: Option<DashTrafficStats>,
    public_info: PublicInfoState,
    proxy_setting: Option<String>,
    rx: mpsc::Receiver<Result<PublicInfo, (String, String)>>,
    tx: mpsc::Sender<Result<PublicInfo, (String, String)>>,
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

        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            self.proxy_setting = None;
            // GNOME：mode 为 manual 时取 http host:port。best-effort，非 GNOME/无命令则跳过。
            let mode = Command::new("gsettings")
                .args(["get", "org.gnome.system.proxy", "mode"])
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            if mode.contains("manual") {
                let host = Command::new("gsettings")
                    .args(["get", "org.gnome.system.proxy.http", "host"])
                    .output()
                    .ok()
                    .map(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .trim_matches('\'')
                            .to_string()
                    })
                    .unwrap_or_default();
                let port = Command::new("gsettings")
                    .args(["get", "org.gnome.system.proxy.http", "port"])
                    .output()
                    .ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default();
                if !host.is_empty() {
                    self.proxy_setting = Some(format!("{host}:{port}"));
                }
            }
        }
    }

    /// 多端点 HTTPS 回退 + 尊重系统代理 + 8s 超时版本。
    /// 按 `endpoints` 顺序尝试，首个成功即用；全部失败后回传最后一次错误。
    pub fn fetch_public_ip_with(&mut self, endpoints: Vec<Endpoint>, use_system_proxy: bool) {
        self.public_info = PublicInfoState::Loading;
        let tx = self.tx.clone();

        tokio::spawn(async move {
            let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(8));
            if !use_system_proxy {
                // 仅 power-user 强制直连时禁代理；默认尊重系统/环境代理。
                builder = builder.no_proxy();
            }
            let client = match builder.build() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx
                        .send(Err(("dash_err_req".to_string(), e.to_string())))
                        .await;
                    return;
                }
            };

            let mut last_err = ("dash_err_req".to_string(), String::new());
            for ep in &endpoints {
                match client.get(&ep.url).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        match resp.text().await {
                            Ok(body) => {
                                if let Some(info) = pubip::parse(&ep.kind, &body) {
                                    let _ = tx.send(Ok(info)).await;
                                    return;
                                } else {
                                    last_err = (
                                        "dash_err_parse".to_string(),
                                        format!("{} ({})", ep.url, status),
                                    );
                                }
                            }
                            Err(e) => {
                                last_err = ("dash_err_parse".to_string(), e.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        last_err = ("dash_err_req".to_string(), e.to_string());
                    }
                }
            }
            let _ = tx.send(Err(last_err)).await;
        });
    }

    /// 兼容旧调用点：用默认端点链（ip.sb→ipinfo，尊重代理）抓取。
    pub fn fetch_public_ip(&mut self, _lang_code: &str) {
        let cfg = crate::config::PublicIpConfig::default();
        self.fetch_public_ip_with(cfg.endpoints, cfg.use_system_proxy);
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
                        // saturating_sub：网卡切换 / 计数器回绕时 rx < total_rx 不会 panic
                        stats.rx_speed =
                            (rx.saturating_sub(stats.total_rx) as f64 / duration) as u64;
                        stats.tx_speed =
                            (tx.saturating_sub(stats.total_tx) as f64 / duration) as u64;
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

    pub fn on_key(&mut self, action: Option<Action>, current_lang: &str) {
        if action == Some(Action::Refresh) {
            self.refresh_active_interface();
            self.detect_proxy();
            self.fetch_public_ip(current_lang);
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
        .fg(theme::COLOR_SECONDARY)
        .add_modifier(Modifier::BOLD);

    let mut rows = Vec::new();

    let time_str = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("dash_time"), key_style)),
        Cell::from(time_str),
    ]));
    rows.push(Row::new(vec![
        Cell::from(Span::styled(i18n.t("label_hostname"), key_style)),
        Cell::from(format!(
            "{} ({} {})",
            dash.hostname, dash.os_name, dash.os_version
        )),
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
                        Style::default().fg(theme::COLOR_UP),
                    ),
                    Span::styled(
                        format!("↑ {:<9}", format_speed(stats.tx_speed)),
                        Style::default().fg(Color::Yellow),
                    ),
                ])),
            ]));
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
            Cell::from(Span::styled(i18n.t("common_status"), key_style)),
            Cell::from(Span::styled(
                i18n.t("dash_no_active_iface"),
                Style::default().fg(theme::COLOR_ERROR),
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
                    Style::default().fg(theme::COLOR_ERROR),
                )),
            ]));
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_type"), key_style)),
                Cell::from(Span::styled(
                    i18n.t(key),
                    Style::default().fg(theme::COLOR_ERROR),
                )),
            ]));
            // 按字符（非字节）截断：错误串可能含 OS 本地化/非 ASCII 文本，
            // 直接 &str[..30] 在非字符边界会 panic 崩溃渲染线程。
            let safe_msg = if debug_msg.chars().count() > 30 {
                format!("{}...", debug_msg.chars().take(30).collect::<String>())
            } else {
                debug_msg.clone()
            };
            rows.push(Row::new(vec![
                Cell::from(Span::styled(i18n.t("dash_debug"), key_style)),
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
