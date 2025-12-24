use crate::app::{App, CurrentTab};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Tabs},
};

pub mod theme;

pub fn draw(f: &mut Frame, app: &mut App) {
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

    let footer_text = app.t("footer_help");
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(theme::COLOR_SUBTEXT))
        .alignment(Alignment::Left);
    f.render_widget(footer, chunks[2]);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let tabs_list = vec![
        (CurrentTab::Dashboard, "tab_dashboard"),
        (CurrentTab::Adapter, "tab_adapter"),
        (CurrentTab::Scanner, "tab_scanner"),
        (CurrentTab::Traffic, "tab_traffic"),
        (CurrentTab::Diagnostics, "tab_diagnostics"),
        (CurrentTab::Settings, "tab_settings"),
    ];

    let (active_fg, active_bg) = if app.diag_focused {
        (Color::White, Color::DarkGray)
    } else {
        (theme::COLOR_PRIMARY, theme::COLOR_HIGHLIGHT_BG)
    };

    let titles: Vec<Line> = tabs_list
        .iter()
        .map(|(tab_enum, i18n_key)| {
            let text = format!(" {} ", app.t(i18n_key));
            if *tab_enum == app.current_tab {
                Line::from(Span::styled(
                    text,
                    Style::default()
                        .fg(active_fg)
                        .bg(active_bg)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    text,
                    Style::default().fg(theme::COLOR_PRIMARY),
                ))
            }
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" IP Tools CLI "),
        )
        .divider(Span::raw("|").fg(theme::COLOR_SUBTEXT))
        .select(app.current_tab as usize);

    f.render_widget(tabs, area);
}

#[allow(dead_code)]
fn render_placeholder(f: &mut Frame, area: Rect, app: &App) {
    let text = format!("\n\n  {} {:?}  ", app.t("label_loading"), app.current_tab);
    let p = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}
