use crate::config::Config;
use crate::keymap::{Action, KeyMap};
use crate::modules::adapter::AdapterModule;
use crate::modules::dashboard::Dashboard;
use crate::modules::diagnostics::DiagnosticsModule;
use crate::modules::scanner::ScannerModule;
use crate::modules::settings::SettingsModule;
use crate::modules::traffic::TrafficModule;
use crate::utils::i18n::I18n;
use crossterm::event::KeyEvent;

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

pub struct App {
    pub running: bool,
    pub current_tab: CurrentTab,
    pub diag_focused: bool,

    pub config: Config,
    pub i18n: I18n,
    pub keymap: KeyMap,

    pub dashboard: Dashboard,
    pub adapter: AdapterModule,
    pub scanner: ScannerModule,
    pub settings: SettingsModule,
    pub traffic: TrafficModule,
    pub diagnostics: DiagnosticsModule,
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
            dashboard: Dashboard::new(),
            adapter: AdapterModule::new(),
            scanner: ScannerModule::new(),
            settings: SettingsModule::new(),
            traffic: TrafficModule::new(),
            diagnostics: DiagnosticsModule::new(),
            config,
            i18n,
            keymap,
        };

        app.dashboard.fetch_public_ip(app.i18n.get_lang().as_str());

        app
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
        let action = self.keymap.action_for(key);

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
            CurrentTab::Adapter => self.adapter.on_key(action),
            CurrentTab::Scanner => {
                self.scanner
                    .on_key(key, action, self.config.scan_concurrency);
            }
            CurrentTab::Traffic => self.traffic.on_key(action),
            CurrentTab::Settings => {
                self.settings
                    .on_key(action, &mut self.config, &mut self.i18n, &mut self.dashboard);
            }
            CurrentTab::Diagnostics => {}
        }
    }

    pub fn on_mouse(&mut self, _mouse: crossterm::event::MouseEvent) {}

    pub fn on_resize(&mut self, _w: u16, _h: u16) {}
}
