use crate::app::App;
use crate::config::Config;
use crate::modules::dashboard::Dashboard;
use crate::utils::i18n::{I18n, Language};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

pub struct SettingsModule {
    state: ListState,
    items: Vec<SettingItem>,
}

enum SettingItem {
    Language,
    Concurrency,
}

impl SettingsModule {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        Self {
            state,
            items: vec![SettingItem::Language, SettingItem::Concurrency],
        }
    }

    pub fn on_key(
        &mut self,
        key: KeyEvent,
        config: &mut Config,
        i18n: &mut I18n,
        dashboard: &mut Dashboard,
    ) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next(),
            KeyCode::Up | KeyCode::Char('k') => self.previous(),
            KeyCode::Left | KeyCode::Char('h') => self.change_value(config, i18n, dashboard, -1),
            KeyCode::Right | KeyCode::Char('l') => self.change_value(config, i18n, dashboard, 1),
            KeyCode::Enter => self.change_value(config, i18n, dashboard, 1),
            _ => {}
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
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
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn change_value(
        &mut self,
        config: &mut Config,
        i18n: &mut I18n,
        dashboard: &mut Dashboard,
        dir: i32,
    ) {
        if let Some(i) = self.state.selected() {
            match self.items[i] {
                SettingItem::Language => {
                    let new_lang = i18n.get_lang().next();
                    i18n.set_lang(new_lang);
                    config.language = new_lang;
                    config.save();
                    dashboard.fetch_public_ip(new_lang.as_str());
                }
                SettingItem::Concurrency => {
                    let mut current = config.scan_concurrency as i32;
                    current += dir * 10;
                    if current < 10 {
                        current = 10;
                    }
                    if current > 500 {
                        current = 500;
                    }

                    config.scan_concurrency = current as usize;
                    config.save();
                }
            }
        }
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let i18n = &app.i18n;
    let config = &app.config;
    let settings = &mut app.settings;

    // 修复：显式指定类型为 usize，以匹配 unicode_width 的返回值类型
    let max_label_width: usize = 20;

    let items: Vec<ListItem> = settings
        .items
        .iter()
        .map(|item| {
            let (label_text, value_text) = match item {
                SettingItem::Language => {
                    let val_str = match config.language {
                        Language::Zh => "简体中文",
                        Language::En => "English",
                    };
                    (i18n.t("setting_lang"), val_str.to_string())
                }
                SettingItem::Concurrency => (
                    i18n.t("setting_concurrency"),
                    config.scan_concurrency.to_string(),
                ),
            };

            // .width() 返回 usize，现在 max_label_width 也是 usize，可以进行减法运算
            let width = label_text.width();
            let padding_len = max_label_width.saturating_sub(width);
            let padding = " ".repeat(padding_len);

            let content = Line::from(vec![
                Span::styled(
                    format!("{}{}", label_text, padding),
                    Style::default().fg(Color::Gray),
                ),
                Span::raw(" : "),
                Span::styled(
                    value_text,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("setting_title")),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(list, chunks[0], &mut settings.state);

    let help_text = i18n.t("setting_help_edit");
    let help = Paragraph::new(help_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center);

    f.render_widget(help, chunks[1]);
}
