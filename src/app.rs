use crate::config::Config;
use crate::modules::adapter::AdapterModule;
use crate::modules::dashboard::Dashboard;
use crate::modules::diagnostics::DiagnosticsModule;
use crate::modules::scanner::ScannerModule;
use crate::modules::settings::SettingsModule;
use crate::modules::traffic::TrafficModule;
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
    pub diag_focused: bool,

    pub config: Config,
    pub i18n: I18n,

    pub dashboard: Dashboard,
    pub adapter: AdapterModule,
    pub scanner: ScannerModule,
    pub settings: SettingsModule,
    pub traffic: TrafficModule,
    pub diagnostics: DiagnosticsModule,
}

impl App {
    pub fn new() -> Self {
        let config = Config::load();
        let i18n = I18n::new(config.language);

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
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.running = false;
                return;
            }
            KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_language();
                return;
            }
            _ => {}
        }

        if self.current_tab == CurrentTab::Diagnostics {
            if self.diag_focused {
                if key.code == KeyCode::Esc {
                    self.diag_focused = false;
                    return;
                }
                self.diagnostics.on_key(key);
                return;
            } else {
                // 修复：使用 || 而不是 |
                if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
                    self.diag_focused = true;
                    return;
                }
            }
        }

        match key.code {
            KeyCode::Tab => {
                self.current_tab = self.current_tab.next();
                self.diag_focused = false;
                return;
            }
            KeyCode::BackTab => {
                self.current_tab = self.current_tab.previous();
                self.diag_focused = false;
                return;
            }
            _ => {}
        }

        match self.current_tab {
            CurrentTab::Dashboard => self.dashboard.on_key(key, self.i18n.get_lang().as_str()),
            CurrentTab::Adapter => self.adapter.on_key(key),
            CurrentTab::Scanner => {
                self.scanner.on_key(key, self.config.scan_concurrency);
            }
            CurrentTab::Traffic => self.traffic.on_key(key),
            CurrentTab::Settings => {
                self.settings
                    .on_key(key, &mut self.config, &mut self.i18n, &mut self.dashboard);
            }
            CurrentTab::Diagnostics => {}
        }
    }

    pub fn on_mouse(&mut self, _mouse: crossterm::event::MouseEvent) {}

    pub fn on_resize(&mut self, _w: u16, _h: u16) {}
}
