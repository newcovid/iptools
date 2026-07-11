use crate::app::App;
use crate::config::Config;
use crate::keymap::Action;
use crate::modules::dashboard::Dashboard;
use crate::utils::i18n::{I18n, Language};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

pub struct SettingsModule {
    state: ListState,
    items: Vec<SettingItem>,
    /// 「清空参数记忆」被触发——由 `App` 在 dispatch 后取走并执行（设置页够不到其他模块）。
    pending_reset: bool,
    /// 刚清空过——在值列显示「已清空 ✓」，移动选择即消除。
    just_reset: bool,
}

enum SettingItem {
    Language,
    Concurrency,
    ResetSession,
}

impl SettingsModule {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));

        Self {
            state,
            items: vec![
                SettingItem::Language,
                SettingItem::Concurrency,
                SettingItem::ResetSession,
            ],
            pending_reset: false,
            just_reset: false,
        }
    }

    /// `App` 调用：取走并清掉「清空参数记忆」请求标志。
    pub fn take_reset(&mut self) -> bool {
        std::mem::take(&mut self.pending_reset)
    }

    /// `App` 在执行清空后回调，用于把值列切到「已清空 ✓」。
    pub fn mark_reset_done(&mut self) {
        self.just_reset = true;
    }

    pub fn on_key(
        &mut self,
        action: Option<Action>,
        config: &mut Config,
        i18n: &mut I18n,
        dashboard: &mut Dashboard,
    ) {
        match action {
            Some(Action::Down) => self.next(),
            Some(Action::Up) => self.previous(),
            // 箭头仅调数值类项；ResetSession 需 Confirm 触发，避免左右导航误清空。
            Some(Action::Left) => self.change_value(config, i18n, dashboard, -1, false),
            Some(Action::Right) => self.change_value(config, i18n, dashboard, 1, false),
            Some(Action::Confirm) => self.change_value(config, i18n, dashboard, 1, true),
            _ => {}
        }
    }

    /// 鼠标：点击设置项第 `row` 行选中。
    pub fn click_row(&mut self, row: usize) {
        if row < self.items.len() {
            self.state.select(Some(row));
        }
    }

    fn next(&mut self) {
        self.just_reset = false;
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
        self.just_reset = false;
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
        activate: bool,
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
                    current = current.clamp(10, 500);

                    config.scan_concurrency = current as usize;
                    config.save();
                }
                SettingItem::ResetSession => {
                    // 仅 Confirm 触发，实际清空交给 App（它能访问所有模块）。
                    if activate {
                        self.pending_reset = true;
                    }
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

    // 登记鼠标区域：设置项列表内容区（去掉边框）。
    app.mouse.settings_list = Some(Block::default().borders(Borders::ALL).inner(chunks[0]));

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
                SettingItem::ResetSession => {
                    let val = if settings.just_reset {
                        i18n.t("setting_reset_done")
                    } else {
                        i18n.t("setting_reset_action")
                    };
                    (i18n.t("setting_reset_session"), val)
                }
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
