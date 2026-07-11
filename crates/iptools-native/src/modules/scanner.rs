use crate::app::App;
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::session::ScannerPersist;
use crate::ui::mru::MruState;
use crate::ui::theme;
use crate::utils::textinput::{TextInput, filter_cidr};
use crate::utils::{net, oui};
use crossterm::event::{KeyCode, KeyEvent};
use ipnetwork::Ipv4Network;
use iptools_core::{JobId, RuntimeEvent, ScanHost, ToolKind};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Gauge, Paragraph, Row, Table, TableState},
};
use std::cell::RefCell;
use std::net::Ipv4Addr;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

use crate::runtime::NativeRuntime;

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

/// 按当前活动物理网卡（有 IPv4）推断默认扫描网段；无可用网卡时退回 192.168.1.0/24。
fn detect_default_cidr() -> String {
    let mut default_cidr = "192.168.1.0/24".to_string();
    let interfaces = net::get_interfaces();
    if let Some(iface) = interfaces
        .iter()
        .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
        && let Some(first_ip) = iface.ipv4.first()
        && let Ok(ip) = first_ip.parse::<Ipv4Addr>()
        && let Ok(net) = Ipv4Network::new(ip, 24)
    {
        default_cidr = net.to_string();
    }
    default_cidr
}

pub struct ScannerModule {
    cidr_input: TextInput,
    input_mode: bool,
    results: Vec<ScanResult>,
    status: ScanStatus,
    table_state: TableState,

    total_scan_count: u64,
    current_scan_count: u64,
    generation: u64,
    job: Option<JobId>,

    history: Rc<RefCell<HistoryStore>>,
    mru: MruState,
}

impl ScannerModule {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
        Self {
            cidr_input: TextInput::with_text(&detect_default_cidr()),
            input_mode: false,
            results: Vec::new(),
            status: ScanStatus::Idle,
            table_state: TableState::default(),
            total_scan_count: 0,
            current_scan_count: 0,
            generation: 0,
            job: None,
            history,
            mru: MruState::default(),
        }
    }

    /// 导出可持久化参数。CIDR 不再持久化——每次启动重新按活动网卡推断默认值；
    /// 用户历史由 MRU 池（`history.cidrs`）独立管理，灰字补全与 Ctrl+R 不受影响。
    pub fn export_persist(&self) -> ScannerPersist {
        ScannerPersist {
            cidr: String::new(),
        }
    }

    /// 回灌持久化参数。空串保留按本机网卡推断的默认 CIDR。
    pub fn apply_persist(&mut self, p: &ScannerPersist) {
        let cidr = p.cidr.trim();
        if !cidr.is_empty() {
            self.cidr_input = TextInput::with_text(cidr);
        }
    }

    /// 重置 CIDR 为按当前活动物理网卡自动推断的默认值（「清空参数记忆」用）。
    pub fn reset_to_default(&mut self) {
        self.cidr_input = TextInput::with_text(&detect_default_cidr());
    }

    pub fn on_key(
        &mut self,
        key: KeyEvent,
        action: Option<Action>,
        concurrency: usize,
        runtime: &mut NativeRuntime,
    ) {
        // 编辑 CIDR 时需要原始按键做文本输入，不走语义动作。
        // 带光标：左右移动、Home/End、中间插入删除；Enter/Esc 退出编辑。
        if self.input_mode {
            if crate::ui::mru::handle_mru_key(
                &mut self.cidr_input,
                &mut self.mru,
                &self.history.borrow().cidrs,
                key,
                action,
                false,
            ) {
                return;
            }
            match key.code {
                // 回车 / 空格 / Esc 均可结束编辑。CIDR 文本本不含空格
                // （filter_cidr 已过滤），故空格用作「完成编辑」与诊断页一致。
                KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Esc => {
                    self.input_mode = false;
                }
                _ => {
                    self.cidr_input.handle_key(key.code, filter_cidr);
                }
            }
            return;
        }

        match action {
            Some(Action::Edit) => {
                self.input_mode = true;
            }
            // Confirm(回车) 与 Toggle(空格) 复用为主操作：未扫描时开始、
            // 扫描中则停止——与诊断页各工具的开始/停止交互一致，无需独立 S 键。
            Some(Action::Confirm) | Some(Action::Toggle) => {
                if self.status == ScanStatus::Scanning {
                    if let Some(job) = self.job {
                        runtime.cancel(job);
                    }
                } else {
                    self.start_scan(concurrency, runtime);
                }
            }
            Some(Action::Down) => self.next(),
            Some(Action::Up) => self.previous(),
            _ => {}
        }
    }

    fn start_scan(&mut self, concurrency: usize, runtime: &mut NativeRuntime) {
        self.results.clear();
        self.status = ScanStatus::Scanning;
        self.table_state.select(None);

        self.current_scan_count = 0;
        self.total_scan_count = 0;
        let cidr_str = self.cidr_input.value();

        if !cidr_str.trim().is_empty() {
            self.history.borrow_mut().cidrs.record(&cidr_str);
        }

        if let Ok(network) = cidr_str.parse::<Ipv4Network>() {
            let count = if network.prefix() < 31 {
                network.size().saturating_sub(2)
            } else {
                network.size()
            };
            self.total_scan_count = count as u64;
        }

        self.generation = self.generation.saturating_add(1);
        let job = JobId {
            tool: ToolKind::Scanner,
            generation: self.generation,
        };
        self.job = Some(job);
        let total = self.total_scan_count;

        runtime.spawn(job, move |token, events| async move {
            let _ = events.send(RuntimeEvent::ScanStarted { job, total }).await;
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
                let completed = Arc::new(AtomicU64::new(0));
                let worker_completed = Arc::clone(&completed);
                let worker_token = token.clone();
                let worker_events = events.clone();
                let worker_count = concurrency.max(1).min(ips.len().max(1));
                let workers = tokio::task::spawn_blocking(move || {
                    let ips = Arc::new(ips);
                    let next = Arc::new(AtomicUsize::new(0));
                    std::thread::scope(|scope| {
                        for _ in 0..worker_count {
                            let ips = Arc::clone(&ips);
                            let next = Arc::clone(&next);
                            let completed = Arc::clone(&worker_completed);
                            let token = worker_token.clone();
                            let events = worker_events.clone();
                            scope.spawn(move || {
                                loop {
                                    if token.is_cancelled() {
                                        break;
                                    }
                                    let index = next.fetch_add(1, Ordering::Relaxed);
                                    let Some(&ip) = ips.get(index) else {
                                        break;
                                    };

                                    // Each worker owns its blocking resolver calls. The parent
                                    // job awaits this scoped pool, so shutdown cannot orphan them.
                                    if let Some(mac) = net::resolve_mac_address(ip)
                                        && !token.is_cancelled()
                                    {
                                        let hostname = net::resolve_hostname(
                                            std::net::IpAddr::V4(ip),
                                        )
                                        .unwrap_or_default();
                                        if !token.is_cancelled() {
                                            let _ = events.blocking_send(
                                                RuntimeEvent::ScanHostFound {
                                                    job,
                                                    host: ScanHost {
                                                        ip: ip.to_string(),
                                                        mac,
                                                        hostname,
                                                    },
                                                },
                                            );
                                        }
                                    }
                                    completed.fetch_add(1, Ordering::Relaxed);
                                }
                            });
                        }
                    });
                });
                tokio::pin!(workers);
                let mut ticker = tokio::time::interval(Duration::from_millis(250));
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let current = completed.load(Ordering::Relaxed);
                            let _ = events.send(RuntimeEvent::ScanProgress { job, current, total }).await;
                        }
                        result = &mut workers => {
                            if let Err(error) = result {
                                tracing::warn!(%error, "scanner worker pool failed to join");
                            }
                            break;
                        }
                    }
                }
                let current = completed.load(Ordering::Relaxed);
                let _ = events.send(RuntimeEvent::ScanProgress { job, current, total }).await;
            }
            let terminal_event = if token.is_cancelled() {
                RuntimeEvent::ScanCancelled { job }
            } else {
                RuntimeEvent::ScanFinished { job }
            };
            let _ = events.send(terminal_event).await;
            Ok(())
        });
    }

    pub fn handle_runtime(&mut self, event: &RuntimeEvent) {
        match event {
            RuntimeEvent::ScanStarted { job, total } if self.job == Some(*job) => {
                self.total_scan_count = *total;
                self.status = ScanStatus::Scanning;
            }
            RuntimeEvent::ScanProgress {
                job,
                current,
                total,
            } if self.job == Some(*job) => {
                self.current_scan_count = *current;
                self.total_scan_count = *total;
            }
            RuntimeEvent::ScanHostFound { job, host } if self.job == Some(*job) => {
                if let Ok(ip) = host.ip.parse() {
                    self.results.push(ScanResult {
                        ip,
                        mac: host.mac.clone(),
                        hostname: host.hostname.clone(),
                    });
                    self.results.sort_by_key(|result| result.ip);
                    if self.table_state.selected().is_none() {
                        self.table_state.select(Some(0));
                    }
                }
            }
            RuntimeEvent::ScanFinished { job } | RuntimeEvent::ScanCancelled { job }
                if self.job == Some(*job) =>
            {
                self.status = ScanStatus::Done;
                self.job = None;
            }
            _ => {}
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

    /// 鼠标：点击 CIDR 行进入编辑并把光标定位到点击列。
    pub fn click_cidr(&mut self, col: usize) {
        self.input_mode = true;
        self.cidr_input.set_cursor_col(col);
    }

    /// 鼠标：点击结果表第 `row` 行选中。
    pub fn click_result(&mut self, row: usize) {
        if row < self.results.len() {
            self.table_state.select(Some(row));
        }
    }

    fn calculate_ip_count(&self) -> String {
        if let Ok(network) = self.cidr_input.value().parse::<Ipv4Network>() {
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
    // 编辑态用黄色，并在 CIDR 文本里内联显示光标块（支持中间编辑）。
    let base = if scanner.input_mode {
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

    // 主操作提示随状态切换：扫描中显示「停止」，否则显示「开始」（同诊断页）。
    let action_btn = if scanner.status == ScanStatus::Scanning {
        i18n.t("scan_btn_stop")
    } else {
        i18n.t("scan_btn_start")
    };

    let mut spans: Vec<Span> = vec![Span::styled(
        format!(" {} ", i18n.t("scan_range_label")),
        base,
    )];
    spans.extend(crate::ui::mru::mru_ghost_spans(
        &scanner.cidr_input,
        &scanner.history.borrow().cidrs,
        scanner.input_mode,
        base,
    ));
    spans.push(Span::styled(
        format!(
            "   {} {}   [{}]   {} / {}",
            i18n.t("scan_count_label"),
            count_text,
            status_text,
            i18n.t("scan_btn_edit"),
            action_btn,
        ),
        base,
    ));

    let control_block = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("scan_title")),
    );

    f.render_widget(control_block, chunks[0]);

    // 登记鼠标区域：CIDR 取值文本起点（用于点击定位光标）+ 结果表体。
    let label_w = format!(" {} ", i18n.t("scan_range_label")).width() as u16;
    app.mouse.scanner_cidr = Some((chunks[0].x + 1 + label_w, chunks[0].y + 1));
    let body = Block::default().borders(Borders::ALL).inner(chunks[1]);
    // 表头占 1 行 + bottom_margin 1 行，数据从 +2 起。
    app.mouse.scanner_results = Some(Rect::new(
        body.x,
        body.y + 2,
        body.width,
        body.height.saturating_sub(2),
    ));

    // --- 2. 结果列表 ---

    // 技巧：表头第一列固定加3个空格，和内容的 `   ` / `>> ` 保持长度一致
    let col_ip_header = format!("   {}", i18n.t("scan_col_ip"));

    let header_cells = vec![
        Cell::from(col_ip_header).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("scan_col_mac")).style(Style::default().fg(Color::Gray)),
        Cell::from(i18n.t("scan_col_vendor")).style(Style::default().fg(Color::Gray)),
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

        let vendor = oui::lookup(&item.mac).unwrap_or("-");

        let cells = vec![
            Cell::from(ip_text).style(Style::default().fg(text_color)),
            Cell::from(item.mac.clone()).style(Style::default().fg(text_color)),
            Cell::from(vendor).style(Style::default().fg(if is_selected {
                Color::Cyan
            } else {
                theme::COLOR_SECONDARY
            })),
            Cell::from(item.hostname.clone()).style(Style::default().fg(text_color)),
        ];

        Row::new(cells).height(1)
    });

    let t = Table::new(
        rows,
        [
            Constraint::Percentage(22),
            Constraint::Percentage(28),
            Constraint::Percentage(22),
            Constraint::Percentage(28),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title(format!(
        "{} ({})",
        i18n.t("scan_devices_found"),
        scanner.results.len()
    )))
    // 使用标准的高亮样式 (背景深灰)，这与其他页面风格统一
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    // 关键：将 highlight_symbol 设为空，因为我们已经手动在内容里加了 ">> "
    .highlight_symbol("");

    f.render_stateful_widget(t, chunks[1], &mut scanner.table_state);

    // --- 3. 进度条 ---
    if scanner.status == ScanStatus::Scanning || scanner.status == ScanStatus::Done {
        let current = scanner.current_scan_count as f64;
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

    // MRU CIDR 历史下拉：仅 input_mode 下有效；离开编辑则关闭，避免悬留。
    if scanner.input_mode {
        if scanner.mru.open {
            let entries: Vec<String> = scanner.history.borrow().cidrs.entries().to_vec();
            crate::ui::mru::draw_mru_popup(f, chunks[1], &entries, scanner.mru.sel, i18n);
        }
    } else {
        scanner.mru.open = false;
    }
}
