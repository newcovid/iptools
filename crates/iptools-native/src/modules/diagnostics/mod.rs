use crate::app::App;
use crate::history::HistoryStore;
use crate::keymap::Action;
use crate::runtime::NativeRuntime;
use crate::session::SessionState;
use crate::utils::textinput::TextInput;
use crossterm::event::KeyEvent;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};
use std::cell::RefCell;
use std::rc::Rc;

/// 诊断各工具「参数配置」栏的统一字段渲染：标签行 + 取值行（带光标块）。
/// 选中且可编辑时在右侧追加提示（直接输入 / 仅数字 / ←→ 调整 等），
/// 让「该字段是输入还是切换」一目了然。返回两行的 ListItem。
pub(super) fn config_field_item(
    label: &str,
    is_sel: bool,
    is_active: bool,
    input: &TextInput,
    cursor_active: bool,
    hint: Option<String>,
) -> ListItem<'static> {
    let label_line = Line::from(Span::styled(
        format!("{}:", label),
        Style::default().fg(if is_sel { Color::Yellow } else { Color::Gray }),
    ));
    let val_base = if is_sel && is_active {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let marker = if is_sel { ">> " } else { "   " };
    let mut spans = vec![Span::styled(marker.to_string(), val_base)];
    spans.extend(input.render_spans(cursor_active, val_base));
    if let Some(h) = hint {
        spans.push(Span::styled(
            format!("  ({})", h),
            Style::default().fg(Color::DarkGray),
        ));
    }
    ListItem::new(vec![label_line, Line::from(spans)])
}

pub mod icmp;
pub mod lan_speed;
pub mod link_quality;
pub mod ping;
pub mod port_scan;
pub mod public_speed;
pub mod trace;

use lan_speed::LanSpeedTool;
use link_quality::LinkQualityTool;
use ping::PingTool;
use port_scan::PortScanTool;
use public_speed::PublicSpeedTool;
use trace::TraceTool;

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
    pub port_scan_tool: PortScanTool,
    pub public_speed_tool: PublicSpeedTool,
    pub trace_tool: TraceTool,
    pub link_quality_tool: LinkQualityTool,
    pub lan_speed_tool: LanSpeedTool,
}

impl DiagnosticsModule {
    pub fn new(history: Rc<RefCell<HistoryStore>>) -> Self {
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
            ping_tool: PingTool::new(history.clone()),
            port_scan_tool: PortScanTool::new(history.clone()),
            public_speed_tool: PublicSpeedTool::new(),
            trace_tool: TraceTool::new(history.clone()),
            link_quality_tool: LinkQualityTool::new(history.clone()),
            lan_speed_tool: LanSpeedTool::new(history.clone()),
        }
    }

    /// 当前选中子工具在菜单中的索引（供持久化「上次所在子工具」）。
    pub fn current_tool_index(&self) -> u8 {
        self.tools
            .iter()
            .position(|t| *t == self.current_tool)
            .unwrap_or(0) as u8
    }

    /// 按持久化索引还原选中的子工具（同步菜单高亮）；越界夹紧。
    pub fn set_tool_by_index(&mut self, idx: u8) {
        if self.tools.is_empty() {
            return;
        }
        let i = (idx as usize).min(self.tools.len() - 1);
        self.current_tool = self.tools[i];
        self.menu_state.select(Some(i));
    }

    /// 把各子工具的会话参数写进 `SessionState`（供持久化）。
    pub fn export_into(&self, s: &mut SessionState) {
        s.ping = self.ping_tool.export_persist();
        s.port_scan = self.port_scan_tool.export_persist();
        s.trace = self.trace_tool.export_persist();
        s.lan_speed = self.lan_speed_tool.export_persist();
        s.link_quality = self.link_quality_tool.export_persist();
    }

    /// 从 `SessionState` 回灌各子工具的会话参数。
    pub fn apply_persist(&mut self, s: &SessionState) {
        self.ping_tool.apply_persist(&s.ping);
        self.port_scan_tool.apply_persist(&s.port_scan);
        self.trace_tool.apply_persist(&s.trace);
        self.lan_speed_tool.apply_persist(&s.lan_speed);
        self.link_quality_tool.apply_persist(&s.link_quality);
    }

    pub fn update(&mut self) {
        match self.current_tool {
            DiagnosticTool::Ping => self.ping_tool.update(),
            DiagnosticTool::PortScan => {}
            DiagnosticTool::NetSpeed => self.public_speed_tool.update(),
            DiagnosticTool::Trace => self.trace_tool.update(),
            DiagnosticTool::LinkQuality => self.link_quality_tool.update(),
            DiagnosticTool::LanSpeed => self.lan_speed_tool.update(),
        }
    }

    pub fn handle_runtime(&mut self, event: &iptools_core::RuntimeEvent) {
        self.port_scan_tool.handle_runtime(event);
    }

    pub fn on_key(
        &mut self,
        key: KeyEvent,
        action: Option<Action>,
        concurrency: usize,
        runtime: &mut NativeRuntime,
    ) {
        // 1. 诊断页内部用 NextTab(默认 Tab) 在 Menu/Main/Config 三栏间切换焦点
        if action == Some(Action::NextTab) {
            self.active_focus = self.active_focus.next();
            return;
        }

        // 2. 根据焦点区域分发事件
        match self.active_focus {
            FocusArea::Menu => self.handle_menu_key(action),
            _ => {
                // 将事件传递给当前选中的工具（需原始按键做文本输入的工具同时收到 key）
                match self.current_tool {
                    DiagnosticTool::Ping => self.ping_tool.on_key(key, action, self.active_focus),
                    DiagnosticTool::PortScan => self.port_scan_tool.on_key(
                        key,
                        action,
                        self.active_focus,
                        concurrency,
                        runtime,
                    ),
                    DiagnosticTool::NetSpeed => self.public_speed_tool.on_key(action),
                    DiagnosticTool::Trace => self.trace_tool.on_key(key, action, self.active_focus),
                    DiagnosticTool::LinkQuality => {
                        self.link_quality_tool
                            .on_key(key, action, self.active_focus)
                    }
                    DiagnosticTool::LanSpeed => {
                        self.lan_speed_tool.on_key(key, action, self.active_focus)
                    }
                }
            }
        }
    }

    /// 鼠标：直接设置当前焦点区域（点击中/右栏时用）。
    pub fn set_focus(&mut self, focus: FocusArea) {
        self.active_focus = focus;
    }

    /// 鼠标：点击左侧菜单第 `row` 项，聚焦菜单并切换到该工具。
    pub fn click_menu(&mut self, row: usize) {
        self.active_focus = FocusArea::Menu;
        if row < self.tools.len() {
            self.menu_state.select(Some(row));
            self.current_tool = self.tools[row];
        }
    }

    fn handle_menu_key(&mut self, action: Option<Action>) {
        match action {
            Some(Action::Down) => self.next_tool(),
            Some(Action::Up) => self.prev_tool(),
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

    // 登记鼠标区域：左栏菜单内容区（用于选工具）+ 中/右栏整列（用于切焦点）。
    app.mouse.diag_menu = Some(Block::default().borders(Borders::ALL).inner(chunks[0]));
    app.mouse.diag_main = Some(chunks[1]);
    app.mouse.diag_config = Some(chunks[2]);

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
        DiagnosticTool::PortScan => {
            diag.port_scan_tool
                .draw(f, chunks[1], chunks[2], i18n, is_focused, diag.active_focus);
        }
        DiagnosticTool::NetSpeed => {
            diag.public_speed_tool.draw(
                f,
                chunks[1],
                chunks[2],
                i18n,
                is_focused,
                diag.active_focus,
            );
        }
        DiagnosticTool::Trace => {
            diag.trace_tool
                .draw(f, chunks[1], chunks[2], i18n, is_focused, diag.active_focus);
        }
        DiagnosticTool::LinkQuality => {
            diag.link_quality_tool.draw(
                f,
                chunks[1],
                chunks[2],
                i18n,
                is_focused,
                diag.active_focus,
            );
        }
        DiagnosticTool::LanSpeed => {
            diag.lan_speed_tool
                .draw(f, chunks[1], chunks[2], i18n, is_focused, diag.active_focus);
        }
    }
}
