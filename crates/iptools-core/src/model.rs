use serde::{Deserialize, Serialize};

use crate::{
    Action, DiagnosticRequest, Effect, InputEvent, JobId, KeyCode, Message::*, RuntimeEvent,
    ScanRequest, ToolKind,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    Input(InputEvent),
    Tick(u64),
    Runtime(RuntimeEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    English,
    Chinese,
}

impl Language {
    pub fn toggle(self) -> Self {
        match self {
            Self::English => Self::Chinese,
            Self::Chinese => Self::English,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Page {
    #[default]
    Dashboard,
    Adapters,
    Scanner,
    Traffic,
    Diagnostics,
    Settings,
}

impl Page {
    pub const ALL: [Self; 6] = [
        Self::Dashboard,
        Self::Adapters,
        Self::Scanner,
        Self::Traffic,
        Self::Diagnostics,
        Self::Settings,
    ];

    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }

    fn next(self) -> Self {
        Self::from_index((self as u8 + 1) % Self::ALL.len() as u8)
    }

    fn previous(self) -> Self {
        Self::from_index((self as u8 + Self::ALL.len() as u8 - 1) % Self::ALL.len() as u8)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiagnosticTool {
    #[default]
    Ping,
    Trace,
    PortScan,
    PublicSpeed,
    LinkQuality,
    LanSpeed,
}

impl DiagnosticTool {
    pub const ALL: [Self; 6] = [
        Self::Ping,
        Self::Trace,
        Self::PortScan,
        Self::PublicSpeed,
        Self::LinkQuality,
        Self::LanSpeed,
    ];

    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdapterInfo {
    pub name: String,
    pub kind: String,
    pub ipv4: String,
    pub mac: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdapterConfig {
    pub name: String,
    pub ipv4: String,
    pub gateway: String,
    pub dns: String,
    pub dhcp: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrafficRow {
    pub name: String,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub total_download: u64,
    pub total_upload: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ScanHost {
    pub ip: String,
    pub mac: String,
    pub hostname: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TaskStatus {
    #[default]
    Idle,
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardState {
    pub hostname: String,
    pub public_ip: String,
    pub download_bps: u64,
    pub upload_bps: u64,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            hostname: "loading…".into(),
            public_ip: "loading…".into(),
            download_bps: 0,
            upload_bps: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScannerState {
    pub cidr: String,
    pub editing: bool,
    pub status: TaskStatus,
    pub current: u64,
    pub total: u64,
    pub results: Vec<ScanHost>,
    pub selected: usize,
    pub job: Option<JobId>,
}

impl Default for ScannerState {
    fn default() -> Self {
        Self {
            cidr: "192.168.1.0/24".into(),
            editing: false,
            status: TaskStatus::Idle,
            current: 0,
            total: 0,
            results: Vec::new(),
            selected: 0,
            job: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticState {
    pub tool: DiagnosticTool,
    pub target: String,
    pub status: TaskStatus,
    pub progress: u8,
    pub primary: String,
    pub detail: String,
    pub log: Vec<String>,
    pub job: Option<JobId>,
}

impl Default for DiagnosticState {
    fn default() -> Self {
        Self {
            tool: DiagnosticTool::Ping,
            target: "192.0.2.1".into(),
            status: TaskStatus::Idle,
            progress: 0,
            primary: String::new(),
            detail: String::new(),
            log: Vec::new(),
            job: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppModel {
    pub running: bool,
    pub demo: bool,
    pub elapsed_ms: u64,
    pub page: Page,
    pub language: Language,
    pub show_help: bool,
    pub dashboard: DashboardState,
    pub adapters: Vec<AdapterInfo>,
    pub adapter_selected: usize,
    pub scanner: ScannerState,
    pub traffic: Vec<TrafficRow>,
    pub diagnostics: DiagnosticState,
    pub scan_concurrency: usize,
    generation: u64,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            running: true,
            demo: true,
            elapsed_ms: 0,
            page: Page::Dashboard,
            language: Language::English,
            show_help: false,
            dashboard: DashboardState::default(),
            adapters: Vec::new(),
            adapter_selected: 0,
            scanner: ScannerState::default(),
            traffic: Vec::new(),
            diagnostics: DiagnosticState::default(),
            scan_concurrency: 50,
            generation: 0,
        }
    }
}

impl AppModel {
    pub fn update(&mut self, message: Message) -> Vec<Effect> {
        match message {
            Input(input) => self.handle_input(input),
            Tick(delta) => {
                self.elapsed_ms = self.elapsed_ms.saturating_add(delta);
                Vec::new()
            }
            Runtime(event) => {
                self.handle_runtime(event);
                Vec::new()
            }
        }
    }

    fn handle_input(&mut self, input: InputEvent) -> Vec<Effect> {
        if let InputEvent::Key(key) = input
            && self.page == Page::Scanner
            && self.scanner.editing
        {
            return self.handle_scanner_edit(key.code);
        }

        let action = match input {
            InputEvent::Action(action) => Some(action),
            InputEvent::Key(key) => key.action(),
            InputEvent::Mouse(_) => None,
        };
        action.map_or_else(Vec::new, |action| self.handle_action(action))
    }

    fn handle_scanner_edit(&mut self, code: KeyCode) -> Vec<Effect> {
        match code {
            KeyCode::Enter | KeyCode::Esc => self.scanner.editing = false,
            KeyCode::Backspace => {
                self.scanner.cidr.pop();
            }
            KeyCode::Char(c)
                if (c.is_ascii_digit() || matches!(c, '.' | '/'))
                    && self.scanner.cidr.len() < 32 =>
            {
                self.scanner.cidr.push(c);
            }
            _ => {}
        }
        Vec::new()
    }

    fn handle_action(&mut self, action: Action) -> Vec<Effect> {
        use Action::*;
        match action {
            Quit => self.running = false,
            ToggleLanguage => self.language = self.language.toggle(),
            NextPage => self.page = self.page.next(),
            PreviousPage => self.page = self.page.previous(),
            SelectPage(index) => self.page = Page::from_index(index),
            Help => self.show_help = !self.show_help,
            Back => self.show_help = false,
            ResetDemo => *self = Self::default(),
            Refresh => {
                return match self.page {
                    Page::Dashboard => vec![Effect::RefreshDashboard],
                    Page::Adapters => vec![Effect::RefreshAdapters],
                    _ => Vec::new(),
                };
            }
            Edit if self.page == Page::Scanner => self.scanner.editing = true,
            Confirm | Toggle if self.page == Page::Scanner => return self.toggle_scan(),
            Confirm | Toggle if self.page == Page::Diagnostics => {
                return self.toggle_diagnostic();
            }
            SelectDiagnostic(index) => self.diagnostics.tool = DiagnosticTool::from_index(index),
            Up => self.navigate(-1),
            Down => self.navigate(1),
            Left if self.page == Page::Settings => {
                self.scan_concurrency = self.scan_concurrency.saturating_sub(10).max(10)
            }
            Right if self.page == Page::Settings => {
                self.scan_concurrency = (self.scan_concurrency + 10).min(500)
            }
            Left | Right | Edit | Confirm | Toggle => {}
        }
        Vec::new()
    }

    fn navigate(&mut self, delta: isize) {
        match self.page {
            Page::Adapters if !self.adapters.is_empty() => {
                self.adapter_selected = wrap(self.adapter_selected, self.adapters.len(), delta)
            }
            Page::Scanner if !self.scanner.results.is_empty() => {
                self.scanner.selected =
                    wrap(self.scanner.selected, self.scanner.results.len(), delta)
            }
            Page::Diagnostics => {
                let index = wrap(
                    self.diagnostics.tool as usize,
                    DiagnosticTool::ALL.len(),
                    delta,
                );
                self.diagnostics.tool = DiagnosticTool::from_index(index as u8);
            }
            _ => {}
        }
    }

    fn next_job(&mut self, tool: ToolKind) -> JobId {
        self.generation = self.generation.saturating_add(1);
        JobId {
            tool,
            generation: self.generation,
        }
    }

    fn toggle_scan(&mut self) -> Vec<Effect> {
        if let Some(job) = self.scanner.job {
            self.scanner.status = TaskStatus::Done;
            self.scanner.job = None;
            return vec![Effect::CancelScan(job)];
        }

        let job = self.next_job(ToolKind::Scanner);
        self.scanner.job = Some(job);
        self.scanner.status = TaskStatus::Running;
        self.scanner.current = 0;
        self.scanner.total = 0;
        self.scanner.results.clear();
        vec![Effect::StartScan {
            job,
            request: ScanRequest {
                cidr: self.scanner.cidr.clone(),
                concurrency: self.scan_concurrency,
            },
        }]
    }

    fn toggle_diagnostic(&mut self) -> Vec<Effect> {
        if let Some(job) = self.diagnostics.job {
            self.diagnostics.job = None;
            self.diagnostics.status = TaskStatus::Done;
            return vec![stop_effect(job)];
        }

        let tool = ToolKind::from(self.diagnostics.tool);
        let job = self.next_job(tool);
        self.diagnostics.job = Some(job);
        self.diagnostics.status = TaskStatus::Running;
        self.diagnostics.progress = 0;
        self.diagnostics.log.clear();
        let request = DiagnosticRequest {
            target: self.diagnostics.target.clone(),
        };
        vec![start_effect(job, request)]
    }

    fn handle_runtime(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::DashboardUpdated {
                hostname,
                public_ip,
                download_bps,
                upload_bps,
            } => {
                self.dashboard = DashboardState {
                    hostname,
                    public_ip,
                    download_bps,
                    upload_bps,
                };
            }
            RuntimeEvent::AdaptersUpdated(adapters) => self.adapters = adapters,
            RuntimeEvent::TrafficUpdated(rows) => self.traffic = rows,
            RuntimeEvent::AdapterConfigApplied(_) => {}
            RuntimeEvent::ScanStarted { job, total } if self.scanner.job == Some(job) => {
                self.scanner.total = total;
                self.scanner.status = TaskStatus::Running;
            }
            RuntimeEvent::ScanProgress {
                job,
                current,
                total,
            } if self.scanner.job == Some(job) => {
                self.scanner.current = current;
                self.scanner.total = total;
            }
            RuntimeEvent::ScanHostFound { job, host } if self.scanner.job == Some(job) => {
                self.scanner.results.push(host)
            }
            RuntimeEvent::ScanFinished { job } | RuntimeEvent::ScanCancelled { job }
                if self.scanner.job == Some(job) =>
            {
                self.scanner.status = TaskStatus::Done;
                self.scanner.job = None;
            }
            RuntimeEvent::DiagnosticStarted { job } if self.diagnostics.job == Some(job) => {
                self.diagnostics.status = TaskStatus::Running
            }
            RuntimeEvent::DiagnosticProgress {
                job,
                progress,
                primary,
                detail,
            } if self.diagnostics.job == Some(job) => {
                self.diagnostics.progress = progress;
                self.diagnostics.primary = primary.clone();
                self.diagnostics.detail = detail;
                self.diagnostics.log.push(primary);
            }
            RuntimeEvent::DiagnosticFinished { job, summary }
                if self.diagnostics.job == Some(job) =>
            {
                self.diagnostics.status = TaskStatus::Done;
                self.diagnostics.progress = 100;
                self.diagnostics.detail = summary;
                self.diagnostics.job = None;
            }
            RuntimeEvent::DiagnosticFailed { job, error } if self.diagnostics.job == Some(job) => {
                self.diagnostics.status = TaskStatus::Failed(error);
                self.diagnostics.job = None;
            }
            _ => {}
        }
    }
}

fn wrap(current: usize, len: usize, delta: isize) -> usize {
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn start_effect(job: JobId, request: DiagnosticRequest) -> Effect {
    match job.tool {
        ToolKind::Ping => Effect::StartPing { job, request },
        ToolKind::Trace => Effect::StartTrace { job, request },
        ToolKind::PortScan => Effect::StartPortScan { job, request },
        ToolKind::PublicSpeed => Effect::StartPublicSpeed { job, request },
        ToolKind::LinkQuality => Effect::StartLinkQuality { job, request },
        ToolKind::LanSpeed => Effect::StartLanSpeed { job, request },
        ToolKind::Scanner => unreachable!("scanner has its own effect"),
    }
}

fn stop_effect(job: JobId) -> Effect {
    match job.tool {
        ToolKind::Ping => Effect::StopPing(job),
        ToolKind::Trace => Effect::StopTrace(job),
        ToolKind::PortScan => Effect::StopPortScan(job),
        ToolKind::PublicSpeed => Effect::StopPublicSpeed(job),
        ToolKind::LinkQuality => Effect::StopLinkQuality(job),
        ToolKind::LanSpeed => Effect::StopLanSpeed(job),
        ToolKind::Scanner => Effect::CancelScan(job),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputEvent, KeyEvent};

    #[test]
    fn navigation_wraps() {
        let mut app = AppModel::default();
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::BackTab))));
        assert_eq!(app.page, Page::Settings);
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Tab))));
        assert_eq!(app.page, Page::Dashboard);
    }

    #[test]
    fn scanner_emits_effect_and_ignores_stale_events() {
        let mut app = AppModel {
            page: Page::Scanner,
            ..AppModel::default()
        };
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartScan { job, .. } = effects[0].clone() else {
            panic!("expected scan effect");
        };
        app.update(Runtime(RuntimeEvent::ScanHostFound {
            job: JobId {
                generation: job.generation + 1,
                ..job
            },
            host: ScanHost::default(),
        }));
        assert!(app.scanner.results.is_empty());
        app.update(Runtime(RuntimeEvent::ScanHostFound {
            job,
            host: ScanHost {
                ip: "192.168.1.1".into(),
                ..ScanHost::default()
            },
        }));
        assert_eq!(app.scanner.results.len(), 1);
    }
}
