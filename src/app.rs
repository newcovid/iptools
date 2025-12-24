use crate::config::Config;
use crate::modules::adapter::AdapterModule;
use crate::modules::dashboard::Dashboard;
use crate::modules::scanner::ScannerModule;
use crate::utils::i18n::I18n;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

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
    pub config: Config,
    pub i18n: I18n,

    // 业务模块
    pub dashboard: Dashboard,
    pub adapter: AdapterModule,
    pub scanner: ScannerModule,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let i18n = I18n::new(config.language);

        let mut app = Self {
            running: true,
            current_tab: CurrentTab::Dashboard,
            dashboard: Dashboard::new(),
            adapter: AdapterModule::new(),
            scanner: ScannerModule::new(),
            config,
            i18n,
        };

        // 初始化请求
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
        match self.current_tab {
            CurrentTab::Dashboard => self.dashboard.update(),
            CurrentTab::Adapter => self.adapter.update(),
            CurrentTab::Scanner => self.scanner.update(),
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.running = false;
                return;
            }
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
                return;
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.previous();
                return;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_language();
                return;
            }
            _ => {}
        }

        match self.current_tab {
            CurrentTab::Dashboard => self.dashboard.on_key(key, self.i18n.get_lang().as_str()),
            CurrentTab::Adapter => self.adapter.on_key(key),
            CurrentTab::Scanner => {
                // 传入配置中的扫描并发数
                self.scanner.on_key(key, self.config.scan_concurrency);
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, _mouse: crossterm::event::MouseEvent) {}

    pub fn on_resize(&mut self, _w: u16, _h: u16) {}
}
