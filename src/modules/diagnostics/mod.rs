use crate::app::App;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph}, // 修复：添加 Paragraph
};

pub mod ping;

use ping::PingTool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticTool {
    Ping,
    Trace,
    PortScan,
    LinkQuality,
    NetSpeed,
    LanSpeed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    Menu,
    Main,
    Config,
}

impl FocusArea {
    fn next(&self) -> Self {
        match self {
            Self::Menu => Self::Main,
            Self::Main => Self::Config,
            Self::Config => Self::Menu,
        }
    }
}

pub struct DiagnosticsModule {
    pub current_tool: DiagnosticTool,
    pub active_focus: FocusArea,
    pub menu_state: ListState,
    tools: Vec<DiagnosticTool>,

    // Sub-tools
    pub ping_tool: PingTool,
}

impl DiagnosticsModule {
    pub fn new() -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        Self {
            current_tool: DiagnosticTool::Ping,
            active_focus: FocusArea::Menu,
            menu_state,
            tools: vec![
                DiagnosticTool::Ping,
                DiagnosticTool::Trace,
                DiagnosticTool::PortScan,
                DiagnosticTool::LinkQuality,
                DiagnosticTool::NetSpeed,
                DiagnosticTool::LanSpeed,
            ],
            ping_tool: PingTool::new(),
        }
    }

    pub fn update(&mut self) {
        match self.current_tool {
            DiagnosticTool::Ping => self.ping_tool.update(),
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        // 1. 全局焦点切换 (Tab)
        if key.code == KeyCode::Tab {
            self.active_focus = self.active_focus.next();
            return;
        }

        // 2. 根据焦点区域分发事件
        match self.active_focus {
            FocusArea::Menu => self.handle_menu_key(key),
            _ => {
                // 将事件传递给当前选中的工具
                match self.current_tool {
                    DiagnosticTool::Ping => self.ping_tool.on_key(key, self.active_focus),
                    _ => {}
                }
            }
        }
    }

    fn handle_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.next_tool(),
            KeyCode::Up | KeyCode::Char('k') => self.prev_tool(),
            _ => {}
        }
    }

    fn next_tool(&mut self) {
        let i = match self.menu_state.selected() {
            Some(i) => {
                if i >= self.tools.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.current_tool = self.tools[i];
    }

    fn prev_tool(&mut self) {
        let i = match self.menu_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.tools.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.current_tool = self.tools[i];
    }
}

pub fn draw(f: &mut Frame, area: Rect, app: &mut App, is_focused: bool) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20), // Menu
            Constraint::Percentage(50), // Main
            Constraint::Percentage(30), // Config
        ])
        .split(area);

    let i18n = &app.i18n;
    let diag = &mut app.diagnostics;

    // --- 1. 左栏：工具菜单 ---
    let menu_color = if is_focused && diag.active_focus == FocusArea::Menu {
        Color::Yellow
    } else {
        Color::Gray
    };

    let items: Vec<ListItem> = diag
        .tools
        .iter()
        .map(|tool| {
            let name_key = match tool {
                DiagnosticTool::Ping => "diag_tool_ping",
                DiagnosticTool::Trace => "diag_tool_trace",
                DiagnosticTool::PortScan => "diag_tool_port",
                DiagnosticTool::LinkQuality => "diag_tool_link",
                DiagnosticTool::NetSpeed => "diag_tool_speed_net",
                DiagnosticTool::LanSpeed => "diag_tool_speed_lan",
            };
            ListItem::new(i18n.t(name_key))
        })
        .collect();

    let menu_list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("diag_menu_title"))
                .border_style(Style::default().fg(menu_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    f.render_stateful_widget(menu_list, chunks[0], &mut diag.menu_state);

    // --- 2 & 3. 中栏与右栏：由具体工具渲染 ---
    match diag.current_tool {
        DiagnosticTool::Ping => {
            diag.ping_tool
                .draw(f, chunks[1], chunks[2], i18n, is_focused, diag.active_focus);
        }
        _ => {
            // 占位符分支
            let main_block = Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("diag_main_title"))
                .border_style(Style::default().fg(Color::DarkGray));
            let conf_block = Block::default()
                .borders(Borders::ALL)
                .title(i18n.t("diag_config_title"))
                .border_style(Style::default().fg(Color::DarkGray));

            f.render_widget(
                Paragraph::new("Coming Soon...").block(main_block),
                chunks[1],
            );

            // 修复：直接渲染 conf_block，去掉错误的 .block(...) 调用
            f.render_widget(conf_block, chunks[2]);
        }
    }
}
