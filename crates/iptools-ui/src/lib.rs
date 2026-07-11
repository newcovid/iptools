//! Backend-independent Ratatui rendering for iptools.

use iptools_core::{Action, AppModel, DiagnosticTool, Language, Page, TaskStatus};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, List, ListItem, Paragraph, Row, Table, Wrap},
};
use unicode_width::UnicodeWidthStr;

const PRIMARY: Color = Color::Cyan;
const SECONDARY: Color = Color::Magenta;
const MUTED: Color = Color::DarkGray;
const SELECTED: Color = Color::Rgb(24, 52, 72);

/// Ephemeral layout state. No application or platform state is stored here.
#[derive(Debug, Default)]
pub struct UiState {
    page_regions: Vec<(Rect, Page)>,
    diagnostic_regions: Vec<(Rect, DiagnosticTool)>,
    scanner_action: Option<Rect>,
    diagnostic_action: Option<Rect>,
}

impl UiState {
    pub fn hit_test(&self, column: u16, row: u16) -> Option<Action> {
        if let Some((_, page)) = self
            .page_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectPage(*page as u8));
        }
        if let Some((_, tool)) = self
            .diagnostic_regions
            .iter()
            .find(|(area, _)| contains(*area, column, row))
        {
            return Some(Action::SelectDiagnostic(*tool as u8));
        }
        if self
            .scanner_action
            .is_some_and(|area| contains(area, column, row))
            || self
                .diagnostic_action
                .is_some_and(|area| contains(area, column, row))
        {
            return Some(Action::Toggle);
        }
        None
    }
}

/// Render the shared application model using any Ratatui backend.
pub fn render(frame: &mut Frame, model: &AppModel, ui: &mut UiState) {
    *ui = UiState::default();
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(2),
        ])
        .split(frame.area());

    render_tabs(frame, areas[0], model, ui);
    match model.page {
        Page::Dashboard => render_dashboard(frame, areas[1], model),
        Page::Adapters => render_adapters(frame, areas[1], model),
        Page::Scanner => render_scanner(frame, areas[1], model, ui),
        Page::Traffic => render_traffic(frame, areas[1], model),
        Page::Diagnostics => render_diagnostics(frame, areas[1], model, ui),
        Page::Settings => render_settings(frame, areas[1], model),
    }
    render_footer(frame, areas[2], model);

    if model.show_help {
        render_help(frame, model.language);
    }
}

fn render_tabs(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let block = Block::default().borders(Borders::ALL).title(if model.demo {
        " IP Tools · DEMO "
    } else {
        " IP Tools "
    });
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut x = inner.x;
    for page in Page::ALL {
        let label = format!(" {} ", page_label(page, model.language));
        let width = label.width() as u16;
        let tab = Rect::new(x, inner.y, width.min(inner.right().saturating_sub(x)), 1);
        let style = if model.page == page {
            Style::default()
                .fg(Color::White)
                .bg(SELECTED)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PRIMARY)
        };
        frame.render_widget(Paragraph::new(label).style(style), tab);
        ui.page_regions.push((tab, page));
        x = x.saturating_add(width + 1);
        if x >= inner.right() {
            break;
        }
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, model: &AppModel) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let local = vec![
        line("Host", &model.dashboard.hostname),
        line(
            "Mode",
            if model.demo {
                "Deterministic demo"
            } else {
                "Native"
            },
        ),
        line("Download", &format_rate(model.dashboard.download_bps)),
        line("Upload", &format_rate(model.dashboard.upload_bps)),
    ];
    frame.render_widget(
        Paragraph::new(local)
            .block(Block::bordered().title(tr(model.language, " 本地概览 ", " Local Overview ")))
            .wrap(Wrap { trim: true }),
        cols[0],
    );
    let public = vec![
        line("Public IP", &model.dashboard.public_ip),
        Line::from(""),
        Line::from(Span::styled(
            tr(
                model.language,
                "在线展览只使用模拟数据，不会访问您的局域网。",
                "The online exhibit uses simulated data and never accesses your LAN.",
            ),
            Style::default().fg(Color::Yellow),
        )),
    ];
    frame.render_widget(
        Paragraph::new(public)
            .block(Block::bordered().title(tr(model.language, " 公网信息 ", " Public Network ")))
            .wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn render_adapters(frame: &mut Frame, area: Rect, model: &AppModel) {
    let rows = model.adapters.iter().enumerate().map(|(index, adapter)| {
        let style = if index == model.adapter_selected {
            Style::default().bg(SELECTED)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(adapter.name.clone()),
            Cell::from(adapter.kind.clone()),
            Cell::from(adapter.ipv4.clone()),
            Cell::from(adapter.mac.clone()),
            Cell::from(adapter.status.clone()),
        ])
        .style(style)
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(23),
                Constraint::Length(10),
                Constraint::Length(17),
                Constraint::Length(18),
                Constraint::Min(8),
            ],
        )
        .header(
            Row::new(["Adapter", "Type", "IPv4", "MAC", "Status"])
                .style(Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD)),
        )
        .block(Block::bordered().title(tr(model.language, " 适配器 ", " Adapters "))),
        area,
    );
}

fn render_scanner(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(5),
        ])
        .split(area);
    let input_style = if model.scanner.editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::White)
    };
    frame.render_widget(
        Paragraph::new(format!(
            "CIDR: {}{}",
            model.scanner.cidr,
            if model.scanner.editing { "█" } else { "" }
        ))
        .style(input_style)
        .block(Block::bordered().title(tr(
            model.language,
            " 扫描范围 · E 编辑 ",
            " Scan Range · E to edit ",
        ))),
        rows[0],
    );
    let ratio = if model.scanner.total == 0 {
        0.0
    } else {
        model.scanner.current as f64 / model.scanner.total as f64
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::bordered().title(task_label(&model.scanner.status, model.language)))
            .gauge_style(Style::default().fg(SECONDARY))
            .ratio(ratio.clamp(0.0, 1.0)),
        rows[1],
    );
    ui.scanner_action = Some(rows[1]);

    let table_rows = model
        .scanner
        .results
        .iter()
        .enumerate()
        .map(|(index, host)| {
            Row::new(vec![
                host.ip.clone(),
                host.mac.clone(),
                host.hostname.clone(),
            ])
            .style(if index == model.scanner.selected {
                Style::default().bg(SELECTED)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        Table::new(
            table_rows,
            [
                Constraint::Length(17),
                Constraint::Length(19),
                Constraint::Min(10),
            ],
        )
        .header(Row::new(["IP", "MAC", "Hostname"]).style(Style::default().fg(PRIMARY)))
        .block(Block::bordered().title(tr(
            model.language,
            " 发现设备 ",
            " Discovered Hosts ",
        ))),
        rows[2],
    );
}

fn render_traffic(frame: &mut Frame, area: Rect, model: &AppModel) {
    let rows = model.traffic.iter().map(|row| {
        Row::new(vec![
            row.name.clone(),
            format_rate(row.download_bps),
            format_rate(row.upload_bps),
            format_bytes(row.total_download),
            format_bytes(row.total_upload),
        ])
    });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Percentage(28),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Length(14),
                Constraint::Min(12),
            ],
        )
        .header(
            Row::new(["Adapter", "Download", "Upload", "Total RX", "Total TX"])
                .style(Style::default().fg(PRIMARY)),
        )
        .block(Block::bordered().title(tr(model.language, " 实时流量 ", " Live Traffic "))),
        area,
    );
}

fn render_diagnostics(frame: &mut Frame, area: Rect, model: &AppModel, ui: &mut UiState) {
    let common = model.diagnostics.active_common();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Min(30),
            Constraint::Length(26),
        ])
        .split(area);

    let mut items = Vec::new();
    let menu_inner = Block::bordered().inner(cols[0]);
    for (index, tool) in DiagnosticTool::ALL.into_iter().enumerate() {
        let row = Rect::new(
            menu_inner.x,
            menu_inner.y + index as u16,
            menu_inner.width,
            1,
        );
        ui.diagnostic_regions.push((row, tool));
        items.push(
            ListItem::new(tool_label(tool)).style(if tool == model.diagnostics.tool {
                Style::default().fg(Color::White).bg(SELECTED)
            } else {
                Style::default().fg(PRIMARY)
            }),
        );
    }
    frame.render_widget(
        List::new(items).block(Block::bordered().title(" Tools ")),
        cols[0],
    );

    let mut lines = vec![
        Line::from(Span::styled(
            common.primary.clone(),
            Style::default().fg(SECONDARY).add_modifier(Modifier::BOLD),
        )),
        Line::from(common.detail.clone()),
        Line::from(""),
    ];
    lines.extend(
        model
            .diagnostics
            .active_common()
            .log
            .iter()
            .rev()
            .take(cols[1].height.saturating_sub(6) as usize)
            .rev()
            .cloned()
            .map(Line::from),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title(format!(" {} ", tool_label(model.diagnostics.tool))))
            .wrap(Wrap { trim: true }),
        cols[1],
    );

    let config_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(cols[2]);
    frame.render_widget(
        Paragraph::new(format!("Target\n{}", model.diagnostics.active_target()))
            .block(Block::bordered().title(" Configuration ")),
        config_rows[0],
    );
    frame.render_widget(
        Gauge::default()
            .block(Block::bordered().title(task_label(&common.status, model.language)))
            .gauge_style(Style::default().fg(SECONDARY))
            .percent(common.progress.min(100) as u16),
        config_rows[1],
    );
    ui.diagnostic_action = Some(config_rows[1]);
}

fn render_settings(frame: &mut Frame, area: Rect, model: &AppModel) {
    let language = match model.language {
        Language::Zh => "简体中文",
        Language::En => "English",
    };
    let rows = vec![
        Row::new(["Language".to_string(), language.to_string()]),
        Row::new([
            "Scan concurrency".to_string(),
            model.scan_concurrency.to_string(),
        ]),
        Row::new([
            "Runtime".to_string(),
            if model.demo { "Demo" } else { "Native" }.to_string(),
        ]),
        Row::new([
            "Persistence".to_string(),
            "Browser local / native config.json".to_string(),
        ]),
    ];
    frame.render_widget(
        Table::new(rows, [Constraint::Length(24), Constraint::Min(20)])
            .block(Block::bordered().title(tr(model.language, " 设置 ", " Settings "))),
        area,
    );
}

fn render_footer(frame: &mut Frame, area: Rect, model: &AppModel) {
    frame.render_widget(
        Paragraph::new(tr(
            model.language,
            " [Tab] 切页  [方向键] 导航  [Enter/Space] 开始/停止  [E] 编辑  [F1] 帮助 ",
            " [Tab] Pages  [Arrows] Navigate  [Enter/Space] Start/Stop  [E] Edit  [F1] Help ",
        ))
        .style(Style::default().fg(MUTED))
        .alignment(Alignment::Center),
        area,
    );
}

fn render_help(frame: &mut Frame, language: Language) {
    let area = centered(frame.area(), 66, 72);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(tr(
            language,
            "键盘与触控快捷键\n\nTab / Shift+Tab  切换页面\n方向键 / WASD    导航\nEnter / Space     开始或停止\nE                 编辑\nCtrl+L            切换语言\nF1 / Esc          打开或关闭帮助\n\n在线版本使用确定性模拟数据。",
            "Keyboard and touch shortcuts\n\nTab / Shift+Tab  switch pages\nArrows / WASD     navigate\nEnter / Space     start or stop\nE                 edit\nCtrl+L            toggle language\nF1 / Esc          open or close help\n\nThe online version uses deterministic simulated data.",
        ))
        .block(Block::bordered().title(tr(language, " 帮助 ", " Help ")))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height) / 2),
        Constraint::Percentage(height),
        Constraint::Percentage((100 - height) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width) / 2),
        Constraint::Percentage(width),
        Constraint::Percentage((100 - width) / 2),
    ])
    .split(vertical[1])[1]
}

fn contains(area: Rect, x: u16, y: u16) -> bool {
    x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
}

fn tr<'a>(language: Language, zh: &'a str, en: &'a str) -> &'a str {
    match language {
        Language::Zh => zh,
        Language::En => en,
    }
}

fn page_label(page: Page, language: Language) -> &'static str {
    match (page, language) {
        (Page::Dashboard, Language::Zh) => "概览",
        (Page::Adapters, Language::Zh) => "适配器",
        (Page::Scanner, Language::Zh) => "扫描",
        (Page::Traffic, Language::Zh) => "流量",
        (Page::Diagnostics, Language::Zh) => "诊断",
        (Page::Settings, Language::Zh) => "设置",
        (Page::Dashboard, Language::En) => "Dashboard",
        (Page::Adapters, Language::En) => "Adapters",
        (Page::Scanner, Language::En) => "Scanner",
        (Page::Traffic, Language::En) => "Traffic",
        (Page::Diagnostics, Language::En) => "Diagnostics",
        (Page::Settings, Language::En) => "Settings",
    }
}

fn tool_label(tool: DiagnosticTool) -> &'static str {
    match tool {
        DiagnosticTool::Ping => "Ping",
        DiagnosticTool::Trace => "Trace Route",
        DiagnosticTool::PortScan => "Port Scan",
        DiagnosticTool::PublicSpeed => "Public Speed",
        DiagnosticTool::LinkQuality => "Link Quality",
        DiagnosticTool::LanSpeed => "LAN Speed",
    }
}

fn task_label(status: &TaskStatus, language: Language) -> &'static str {
    match status {
        TaskStatus::Idle => tr(language, " 空闲 ", " Idle "),
        TaskStatus::Running => tr(language, " 运行中 · 点击停止 ", " Running · click to stop "),
        TaskStatus::Done => tr(
            language,
            " 完成 · 点击重新开始 ",
            " Done · click to restart ",
        ),
        TaskStatus::Failed(_) => tr(language, " 失败 · 点击重试 ", " Failed · click to retry "),
    }
}

fn line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<12}"), Style::default().fg(MUTED)),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

fn format_rate(value: u64) -> String {
    format!("{}/s", format_bytes(value))
}

fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut amount = value as f64;
    let mut unit = 0;
    while amount >= 1024.0 && unit < UNITS.len() - 1 {
        amount /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else {
        format!("{amount:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_demo_banner_and_hit_regions() {
        let backend = TestBackend::new(120, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        let model = AppModel::default();
        let mut ui = UiState::default();
        terminal
            .draw(|frame| render(frame, &model, &mut ui))
            .unwrap();
        let text = terminal.backend().to_string();
        assert!(text.contains("DEMO"));
        assert_eq!(ui.hit_test(2, 1), Some(Action::SelectPage(0)));
    }
}
