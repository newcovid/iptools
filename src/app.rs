use crate::config::Config;
use crate::keymap::{Action, KeyMap};
use crate::modules::adapter::AdapterModule;
use crate::modules::dashboard::Dashboard;
use crate::modules::diagnostics::DiagnosticsModule;
use crate::modules::scanner::ScannerModule;
use crate::modules::settings::SettingsModule;
use crate::modules::traffic::TrafficModule;
use crate::session::{SessionState, UiPersist};
use crate::utils::i18n::I18n;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurrentTab {
    Dashboard = 0,
    Adapter = 1,
    Scanner = 2,
    Traffic = 3,
    Diagnostics = 4,
    Settings = 5,
}

impl CurrentTab {
    /// 由持久化的索引（判别值 0..5）还原标签页；越界回退概览。
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Adapter,
            2 => Self::Scanner,
            3 => Self::Traffic,
            4 => Self::Diagnostics,
            5 => Self::Settings,
            _ => Self::Dashboard,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Dashboard => Self::Adapter,
            Self::Adapter => Self::Scanner,
            Self::Scanner => Self::Traffic,
            Self::Traffic => Self::Diagnostics,
            Self::Diagnostics => Self::Settings,
            Self::Settings => Self::Dashboard,
        }
    }

    pub fn previous(&self) -> Self {
        match self {
            Self::Dashboard => Self::Settings,
            Self::Adapter => Self::Dashboard,
            Self::Scanner => Self::Adapter,
            Self::Traffic => Self::Scanner,
            Self::Diagnostics => Self::Traffic,
            Self::Settings => Self::Diagnostics,
        }
    }
}

/// 一帧渲染时记录的可点击区域，供 `on_mouse` 命中测试。
/// 每次 `ui::draw` 重新填充——坐标随布局/窗口变化自动更新。
#[derive(Default)]
pub struct MouseRegions {
    /// 每个标签页标题的矩形 + 对应页。
    pub tabs: Vec<(Rect, CurrentTab)>,
    /// 适配器只读列表的内容区（每行一个网卡，顺序同 `interfaces`）。
    pub adapter_list: Option<Rect>,
    /// 适配器编辑表单的字段区（每行一个字段）。
    pub adapter_edit: Option<Rect>,
    /// 扫描结果表体（已扣除表头），每行一个结果。
    pub scanner_results: Option<Rect>,
    /// 扫描 CIDR 取值文本的起点 (x, y)，用于点击定位光标。
    pub scanner_cidr: Option<(u16, u16)>,
    /// 设置项列表内容区。
    pub settings_list: Option<Rect>,
    /// 诊断三栏的内容区。
    pub diag_menu: Option<Rect>,
    pub diag_main: Option<Rect>,
    pub diag_config: Option<Rect>,
}

/// 命中测试：点 (x,y) 是否落在矩形内。
fn hit(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

pub struct App {
    pub running: bool,
    pub current_tab: CurrentTab,
    pub diag_focused: bool,
    /// 是否显示快捷键帮助浮层（模态）。
    pub show_help: bool,
    /// 本帧的可点击区域（由各模块 draw 填充）。
    pub mouse: MouseRegions,

    pub config: Config,
    pub i18n: I18n,
    pub keymap: KeyMap,

    pub dashboard: Dashboard,
    pub adapter: AdapterModule,
    pub scanner: ScannerModule,
    pub settings: SettingsModule,
    pub traffic: TrafficModule,
    pub diagnostics: DiagnosticsModule,

    /// 上次落盘的会话参数快照。每次按键/鼠标后与最新快照对比，仅在变化时写盘，
    /// 既保证「重启不丢」又避免高频磁盘写入（见 `maybe_persist`）。
    last_session: SessionState,
}

impl App {
    pub fn new(config_path: Option<String>) -> Self {
        let config = Config::load(config_path.as_deref());
        let i18n = I18n::new(config.language);
        let keymap = config.keymap();

        let mut app = Self {
            running: true,
            current_tab: CurrentTab::Dashboard,
            diag_focused: false,
            show_help: false,
            mouse: MouseRegions::default(),
            dashboard: Dashboard::new(),
            adapter: AdapterModule::new(),
            scanner: ScannerModule::new(),
            settings: SettingsModule::new(),
            traffic: TrafficModule::new(),
            diagnostics: DiagnosticsModule::new(),
            config,
            i18n,
            keymap,
            last_session: SessionState::default(),
        };

        // 启动回灌：把上次保存的输入参数写回各模块，并记录基准快照。
        app.apply_session();
        app.last_session = app.snapshot_session();

        app.dashboard.fetch_public_ip(app.i18n.get_lang().as_str());

        app
    }

    /// 汇总各模块当前会话参数为一份快照。
    fn snapshot_session(&self) -> SessionState {
        let mut s = SessionState {
            scanner: self.scanner.export_persist(),
            ui: UiPersist {
                last_tab: self.current_tab as u8,
                last_diag_tool: self.diagnostics.current_tool_index(),
            },
            ..SessionState::default()
        };
        self.diagnostics.export_into(&mut s);
        s
    }

    /// 把 `config.session` 回灌到各模块（启动时调用一次）。
    fn apply_session(&mut self) {
        let s = self.config.session.clone();
        self.scanner.apply_persist(&s.scanner);
        self.diagnostics.apply_persist(&s);
        // 恢复上次所在标签页与诊断子工具。
        self.current_tab = CurrentTab::from_index(s.ui.last_tab);
        self.diagnostics.set_tool_by_index(s.ui.last_diag_tool);
    }

    /// 清空所有「参数记忆」：会话参数恢复默认并落盘、回灌到各模块，
    /// 但**保留当前界面位置**（不把用户从设置页弹走）。设置页「清空参数记忆」调用。
    fn reset_session(&mut self) {
        let keep_ui = self.snapshot_session().ui;
        self.config.session = SessionState {
            ui: keep_ui,
            ..SessionState::default()
        };
        self.config.save();
        self.apply_session();
        // apply 会按 keep_ui 还原位置；扫描 CIDR 单独重置为按网卡自动推断。
        self.scanner.reset_to_default();
        self.last_session = self.snapshot_session();
    }

    /// 脏检查持久化：仅当会话参数较上次落盘发生变化时，写回 `config.json`。
    fn maybe_persist(&mut self) {
        let cur = self.snapshot_session();
        if cur != self.last_session {
            self.config.session = cur.clone();
            self.config.save();
            self.last_session = cur;
        }
    }

    pub fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    pub fn toggle_language(&mut self) {
        let new_lang = self.i18n.get_lang().next();
        self.i18n.set_lang(new_lang);
        self.config.language = new_lang;
        self.config.save();

        self.dashboard.fetch_public_ip(new_lang.as_str());
    }

    pub fn on_tick(&mut self) {
        self.diagnostics.update();
        match self.current_tab {
            CurrentTab::Dashboard => self.dashboard.update(),
            CurrentTab::Adapter => self.adapter.update(),
            CurrentTab::Scanner => self.scanner.update(),
            CurrentTab::Traffic => self.traffic.update(),
            CurrentTab::Diagnostics => {}
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        self.handle_key(key);
        // 处理完按键后做一次脏检查持久化，保证任何参数改动都即时落盘、重启不丢。
        self.maybe_persist();
    }

    fn handle_key(&mut self, key: KeyEvent) {
        let action = self.keymap.action_for(key);

        // 0. 帮助浮层是模态的：显示时除退出外的任意键都用于关闭它
        if self.show_help {
            if action == Some(Action::Quit) {
                self.running = false;
            } else {
                self.show_help = false;
            }
            return;
        }

        // 1. 全局动作：任何标签页 / 任何模式下都生效
        match action {
            Some(Action::Quit) => {
                self.running = false;
                return;
            }
            Some(Action::ToggleLanguage) => {
                self.toggle_language();
                return;
            }
            Some(Action::Help) => {
                self.show_help = true;
                return;
            }
            _ => {}
        }

        // 2. 诊断页的两级焦点模型（进入交互后按键交给子模块）
        if self.current_tab == CurrentTab::Diagnostics {
            if self.diag_focused {
                if action == Some(Action::Back) {
                    self.diag_focused = false;
                    return;
                }
                self.diagnostics
                    .on_key(key, action, self.config.scan_concurrency);
                return;
            } else if action == Some(Action::Confirm) {
                self.diag_focused = true;
                return;
            }
        }

        // 3. 主标签页切换
        match action {
            Some(Action::NextTab) => {
                self.current_tab = self.current_tab.next();
                self.diag_focused = false;
                return;
            }
            Some(Action::PrevTab) => {
                self.current_tab = self.current_tab.previous();
                self.diag_focused = false;
                return;
            }
            _ => {}
        }

        // 4. 分发给当前模块。需要原始按键做文本输入的模块（scanner）同时收到 key。
        match self.current_tab {
            CurrentTab::Dashboard => self.dashboard.on_key(action, self.i18n.get_lang().as_str()),
            CurrentTab::Adapter => self.adapter.on_key(key, action),
            CurrentTab::Scanner => {
                self.scanner
                    .on_key(key, action, self.config.scan_concurrency);
            }
            CurrentTab::Traffic => self.traffic.on_key(action),
            CurrentTab::Settings => {
                self.settings
                    .on_key(action, &mut self.config, &mut self.i18n, &mut self.dashboard);
                // 设置页够不到其他模块：清空请求在此由 App 统一执行。
                if self.settings.take_reset() {
                    self.reset_session();
                    self.settings.mark_reset_done();
                }
            }
            CurrentTab::Diagnostics => {}
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        self.handle_mouse(m);
        self.maybe_persist();
    }

    fn handle_mouse(&mut self, m: MouseEvent) {
        // 帮助浮层打开时，任意点击关闭它。
        if self.show_help {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.show_help = false;
            }
            return;
        }
        match m.kind {
            // 滚轮上下：等价键盘上下导航（按当前页/焦点分发）。
            MouseEventKind::ScrollDown => self.route_nav(Action::Down),
            MouseEventKind::ScrollUp => self.route_nav(Action::Up),
            MouseEventKind::Down(MouseButton::Left) => self.handle_click(m.column, m.row),
            _ => {}
        }
    }

    /// 把一个上下导航动作分发到当前页/焦点（供滚轮复用键盘导航逻辑）。
    fn route_nav(&mut self, action: Action) {
        let dummy = KeyEvent::new(KeyCode::Null, KeyModifiers::NONE);
        if self.current_tab == CurrentTab::Diagnostics {
            if self.diag_focused {
                self.diagnostics
                    .on_key(dummy, Some(action), self.config.scan_concurrency);
            }
            return;
        }
        match self.current_tab {
            CurrentTab::Adapter => self.adapter.on_key(dummy, Some(action)),
            CurrentTab::Scanner => {
                self.scanner
                    .on_key(dummy, Some(action), self.config.scan_concurrency)
            }
            CurrentTab::Traffic => self.traffic.on_key(Some(action)),
            CurrentTab::Settings => {
                self.settings
                    .on_key(Some(action), &mut self.config, &mut self.i18n, &mut self.dashboard)
            }
            _ => {}
        }
    }

    /// 左键点击：先判标签页，再按当前页命中列表/字段/诊断三栏。
    fn handle_click(&mut self, x: u16, y: u16) {
        // 1. 标签页切换
        if let Some(tab) = self
            .mouse
            .tabs
            .iter()
            .find(|(r, _)| hit(*r, x, y))
            .map(|(_, t)| *t)
        {
            self.current_tab = tab;
            self.diag_focused = false;
            return;
        }

        // 2. 当前页内的点击
        match self.current_tab {
            CurrentTab::Adapter => {
                if self.adapter.edit.is_some() {
                    if let Some(area) = self.mouse.adapter_edit {
                        self.adapter.click_edit(x, y, area);
                    }
                } else if let Some(area) = self.mouse.adapter_list {
                    if hit(area, x, y) {
                        self.adapter.click_list((y - area.y) as usize);
                    }
                }
            }
            CurrentTab::Scanner => {
                if let Some((cx, cy)) = self.mouse.scanner_cidr {
                    if y == cy {
                        self.scanner.click_cidr(x.saturating_sub(cx) as usize);
                        return;
                    }
                }
                if let Some(area) = self.mouse.scanner_results {
                    if hit(area, x, y) {
                        self.scanner.click_result((y - area.y) as usize);
                    }
                }
            }
            CurrentTab::Settings => {
                if let Some(area) = self.mouse.settings_list {
                    if hit(area, x, y) {
                        self.settings.click_row((y - area.y) as usize);
                    }
                }
            }
            CurrentTab::Diagnostics => {
                if let Some(area) = self.mouse.diag_menu {
                    if hit(area, x, y) {
                        self.diag_focused = true;
                        self.diagnostics.click_menu((y - area.y) as usize);
                        return;
                    }
                }
                if let Some(area) = self.mouse.diag_main {
                    if hit(area, x, y) {
                        self.diag_focused = true;
                        self.diagnostics.set_focus(crate::modules::diagnostics::FocusArea::Main);
                        return;
                    }
                }
                if let Some(area) = self.mouse.diag_config {
                    if hit(area, x, y) {
                        self.diag_focused = true;
                        self.diagnostics
                            .set_focus(crate::modules::diagnostics::FocusArea::Config);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_resize(&mut self, _w: u16, _h: u16) {}
}
