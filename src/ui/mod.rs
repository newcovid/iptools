use crate::app::{App, CurrentTab};
use crate::keymap::Action;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
};
use unicode_width::UnicodeWidthStr;

pub mod mru;
pub mod theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    // 每帧重置可点击区域，由 render_tabs 与各模块 draw 重新登记，杜绝陈旧坐标。
    app.mouse = crate::app::MouseRegions::default();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_tabs(f, chunks[0], app);

    match app.current_tab {
        CurrentTab::Dashboard => {
            crate::modules::dashboard::draw(f, chunks[1], app);
        }
        CurrentTab::Adapter => {
            crate::modules::adapter::draw(f, chunks[1], app);
        }
        CurrentTab::Scanner => {
            crate::modules::scanner::draw(f, chunks[1], app);
        }
        CurrentTab::Traffic => {
            crate::modules::traffic::draw(f, chunks[1], app);
        }
        CurrentTab::Diagnostics => {
            crate::modules::diagnostics::draw(f, chunks[1], app, app.diag_focused);
        }
        CurrentTab::Settings => {
            crate::modules::settings::draw(f, chunks[1], app);
        }
    }

    let footer = Paragraph::new(footer_text(app))
        .style(Style::default().fg(theme::COLOR_SUBTEXT))
        .alignment(Alignment::Left);
    f.render_widget(footer, chunks[2]);

    // 模态：快捷键帮助浮层覆盖在最上层
    if app.show_help {
        render_help_overlay(f, app);
    }
}

/// 居中弹层快捷键速查（内容取自当前键位映射，反映用户自定义绑定）。
fn render_help_overlay(f: &mut Frame, app: &App) {
    let area = centered_rect(f.area(), 60, 80);
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::COLOR_SECONDARY))
        .title(format!(" {} ", app.t("help_title")));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(inner);

    let rows: Vec<Row> = Action::ALL
        .iter()
        .map(|a| {
            Row::new(vec![
                Cell::from(app.keymap.all_labels(*a))
                    .style(Style::default().fg(theme::COLOR_PRIMARY)),
                Cell::from(app.t(a.desc_key())).style(Style::default().fg(Color::White)),
            ])
        })
        .collect();
    let table = Table::new(rows, [Constraint::Length(20), Constraint::Min(0)]).column_spacing(2);
    f.render_widget(table, layout[0]);

    let hint = Paragraph::new(vec![
        Line::from(Span::styled(
            app.t("help_customize"),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            app.t("help_hint"),
            Style::default().fg(Color::Yellow),
        )),
    ])
    .alignment(Alignment::Center);
    f.render_widget(hint, layout[1]);
}

/// 在给定区域内取居中的 percent_x% × percent_y% 子矩形。
fn centered_rect(r: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(v[1])[1]
}

/// 动态生成底部帮助栏：按键标签直接取自当前键位映射，
/// 用户在 config 里改了绑定，这里会随之更新（不再写死 Tab/Ctrl-c）。
fn footer_text(app: &App) -> String {
    let km = &app.keymap;
    format!(
        " [{}/{}] {}   [{}] {}   [{}] {}   [{}] {} ",
        km.primary_label(Action::NextTab),
        km.primary_label(Action::PrevTab),
        app.t("footer_switch"),
        km.primary_label(Action::ToggleLanguage),
        app.t("footer_lang"),
        km.primary_label(Action::Help),
        app.t("footer_help_label"),
        km.primary_label(Action::Quit),
        app.t("footer_quit"),
    )
}

/// 手工渲染标签栏（而非 Tabs widget），以便登记每个标签的精确点击矩形。
fn render_tabs(f: &mut Frame, area: Rect, app: &mut App) {
    let tabs_list = [
        (CurrentTab::Dashboard, "tab_dashboard"),
        (CurrentTab::Adapter, "tab_adapter"),
        (CurrentTab::Scanner, "tab_scanner"),
        (CurrentTab::Traffic, "tab_traffic"),
        (CurrentTab::Diagnostics, "tab_diagnostics"),
        (CurrentTab::Settings, "tab_settings"),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" IP Tools CLI ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let (active_fg, active_bg) = if app.diag_focused {
        (Color::White, Color::DarkGray)
    } else {
        (theme::COLOR_PRIMARY, theme::COLOR_HIGHLIGHT_BG)
    };

    let y = inner.y;
    let max_x = inner.x + inner.width;
    let mut x = inner.x;
    for (i, (tab_enum, key)) in tabs_list.iter().enumerate() {
        if x >= max_x {
            break;
        }
        let label = format!(" {} ", app.t(key));
        let w = (label.width() as u16).min(max_x - x);
        let style = if *tab_enum == app.current_tab {
            Style::default()
                .fg(active_fg)
                .bg(active_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::COLOR_PRIMARY)
        };
        let rect = Rect::new(x, y, w, 1);
        f.render_widget(Paragraph::new(Span::styled(label, style)), rect);
        app.mouse.tabs.push((rect, *tab_enum));
        x += w;
        // 分隔符
        if i + 1 < tabs_list.len() && x < max_x {
            f.render_widget(
                Paragraph::new(Span::styled("|", Style::default().fg(theme::COLOR_SUBTEXT))),
                Rect::new(x, y, 1, 1),
            );
            x += 1;
        }
    }
}

#[allow(dead_code)]
fn render_placeholder(f: &mut Frame, area: Rect, app: &App) {
    let text = format!("\n\n  {} {:?}  ", app.t("label_loading"), app.current_tab);
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}
