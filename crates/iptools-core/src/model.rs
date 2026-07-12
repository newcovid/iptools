use serde::{Deserialize, Serialize};

use crate::{
    Action, Effect, InputEvent, JobId, KeyCode, Message::*, RuntimeEvent, ScanRequest, ToolKind,
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
    En,
    Zh,
}

impl Language {
    pub const fn toggle(self) -> Self {
        match self {
            Self::En => Self::Zh,
            Self::Zh => Self::En,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en-US",
            Self::Zh => "zh-CN",
        }
    }

    pub const fn next(self) -> Self {
        self.toggle()
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
#[serde(default)]
pub struct AdapterInfo {
    pub name: String,
    pub description: String,
    pub guid: String,
    pub kind: String,
    pub ipv4: String,
    pub ipv6: Vec<String>,
    pub mac: String,
    pub status: String,
    pub ssid: Option<String>,
    pub dhcp_enabled: bool,
    pub is_physical: bool,
    pub link_speed_bps: Option<u64>,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub total_download: u64,
    pub total_upload: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AdaptersState {
    pub items: Vec<AdapterInfo>,
    pub selected: usize,
    pub status: TaskStatus,
    pub error: Option<crate::RuntimeError>,
    pub job: Option<JobId>,
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
#[serde(default)]
pub struct TrafficRow {
    pub name: String,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub total_download: u64,
    pub total_upload: u64,
    pub session_download: u64,
    pub session_upload: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TrafficState {
    pub rows: Vec<TrafficRow>,
    pub selected: usize,
    pub status: TaskStatus,
    pub error: Option<crate::RuntimeError>,
    pub job: Option<JobId>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DashboardInterface {
    pub name: String,
    pub description: String,
    pub ipv4: String,
    pub ssid: Option<String>,
    pub is_physical: bool,
    pub dhcp_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PublicIpInfo {
    pub ip: String,
    pub city: String,
    pub region: String,
    pub country: String,
    pub isp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DashboardSnapshot {
    pub observed_at: String,
    pub hostname: String,
    pub os_name: String,
    pub os_version: String,
    pub active_interface: Option<DashboardInterface>,
    pub proxy: Option<String>,
    pub public_info: Option<PublicIpInfo>,
    pub download_bps: u64,
    pub upload_bps: u64,
    pub total_download: u64,
    pub total_upload: u64,
}

impl Default for DashboardSnapshot {
    fn default() -> Self {
        Self {
            observed_at: "—".into(),
            hostname: "loading…".into(),
            os_name: String::new(),
            os_version: String::new(),
            active_interface: None,
            proxy: None,
            public_info: None,
            download_bps: 0,
            upload_bps: 0,
            total_download: 0,
            total_upload: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardState {
    pub snapshot: DashboardSnapshot,
    pub status: TaskStatus,
    pub error: Option<crate::RuntimeError>,
    pub job: Option<JobId>,
}

impl Default for DashboardState {
    fn default() -> Self {
        Self {
            snapshot: DashboardSnapshot::default(),
            status: TaskStatus::Idle,
            error: None,
            job: None,
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
pub struct DiagnosticCommonState {
    pub status: TaskStatus,
    pub progress: u8,
    pub primary: String,
    pub detail: String,
    pub log: Vec<String>,
    pub job: Option<JobId>,
}

impl Default for DiagnosticCommonState {
    fn default() -> Self {
        Self {
            status: TaskStatus::Idle,
            progress: 0,
            primary: String::new(),
            detail: String::new(),
            log: Vec::new(),
            job: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PingState {
    pub request: crate::PingRequest,
    pub common: DiagnosticCommonState,
    pub samples: Vec<crate::PingSample>,
    pub summary: Option<crate::PingSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TraceState {
    pub request: crate::TraceRequest,
    pub common: DiagnosticCommonState,
    pub hops: Vec<crate::TraceHop>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PortScanState {
    pub request: crate::PortScanRequest,
    pub common: DiagnosticCommonState,
    pub scanned: u64,
    pub total: u64,
    pub open_ports: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PublicSpeedState {
    pub request: crate::PublicSpeedRequest,
    pub common: DiagnosticCommonState,
    pub server: Option<String>,
    pub samples: Vec<crate::SpeedSample>,
    pub summary: Option<crate::SpeedSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LinkQualityState {
    pub request: crate::LinkQualityRequest,
    pub common: DiagnosticCommonState,
    pub samples: Vec<crate::LinkQualitySample>,
    pub summary: Option<crate::LinkQualitySummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LanSpeedState {
    pub request: crate::LanSpeedRequest,
    pub common: DiagnosticCommonState,
    pub samples: Vec<crate::LanSpeedSample>,
    pub summary: Option<crate::LanSpeedSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiagnosticsState {
    pub tool: DiagnosticTool,
    pub ping: PingState,
    pub trace: TraceState,
    pub port_scan: PortScanState,
    pub public_speed: PublicSpeedState,
    pub link_quality: LinkQualityState,
    pub lan_speed: LanSpeedState,
}

impl DiagnosticsState {
    pub fn active_common(&self) -> &DiagnosticCommonState {
        match self.tool {
            DiagnosticTool::Ping => &self.ping.common,
            DiagnosticTool::Trace => &self.trace.common,
            DiagnosticTool::PortScan => &self.port_scan.common,
            DiagnosticTool::PublicSpeed => &self.public_speed.common,
            DiagnosticTool::LinkQuality => &self.link_quality.common,
            DiagnosticTool::LanSpeed => &self.lan_speed.common,
        }
    }

    pub fn active_common_mut(&mut self) -> &mut DiagnosticCommonState {
        match self.tool {
            DiagnosticTool::Ping => &mut self.ping.common,
            DiagnosticTool::Trace => &mut self.trace.common,
            DiagnosticTool::PortScan => &mut self.port_scan.common,
            DiagnosticTool::PublicSpeed => &mut self.public_speed.common,
            DiagnosticTool::LinkQuality => &mut self.link_quality.common,
            DiagnosticTool::LanSpeed => &mut self.lan_speed.common,
        }
    }

    pub fn active_target(&self) -> &str {
        match self.tool {
            DiagnosticTool::Ping => &self.ping.request.target,
            DiagnosticTool::Trace => &self.trace.request.target,
            DiagnosticTool::PortScan => &self.port_scan.request.target,
            DiagnosticTool::PublicSpeed => "automatic endpoint",
            DiagnosticTool::LinkQuality => &self.link_quality.request.target,
            DiagnosticTool::LanSpeed => &self.lan_speed.request.peer,
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
    pub adapters: AdaptersState,
    pub scanner: ScannerState,
    pub traffic: TrafficState,
    pub diagnostics: DiagnosticsState,
    pub scan_concurrency: usize,
    #[serde(default)]
    public_ip_config: crate::PublicIpConfig,
    generation: u64,
}

impl Default for AppModel {
    fn default() -> Self {
        Self {
            running: true,
            demo: true,
            elapsed_ms: 0,
            page: Page::Dashboard,
            language: Language::En,
            show_help: false,
            dashboard: DashboardState::default(),
            adapters: AdaptersState::default(),
            scanner: ScannerState::default(),
            traffic: TrafficState::default(),
            diagnostics: DiagnosticsState::default(),
            scan_concurrency: 50,
            public_ip_config: crate::PublicIpConfig::default(),
            generation: 0,
        }
    }
}

impl AppModel {
    pub fn apply_config(&mut self, config: &crate::ConfigData) {
        self.language = config.language;
        self.scan_concurrency = config.scan_concurrency.clamp(10, 500);
        self.public_ip_config = config.public_ip.clone();
    }

    pub const fn preferences(&self) -> crate::Preferences {
        crate::Preferences {
            language: self.language,
            scan_concurrency: self.scan_concurrency,
        }
    }

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
            ToggleLanguage => {
                self.language = self.language.toggle();
                return vec![Effect::PersistPreferences(self.preferences())];
            }
            NextPage => self.page = self.page.next(),
            PreviousPage => self.page = self.page.previous(),
            SelectPage(index) => self.page = Page::from_index(index),
            Help => self.show_help = !self.show_help,
            Back => self.show_help = false,
            ResetDemo => {
                *self = Self::default();
                return vec![Effect::PersistPreferences(self.preferences())];
            }
            Refresh => {
                return match self.page {
                    Page::Dashboard => self.refresh_dashboard(),
                    Page::Adapters => self.refresh_adapters(),
                    Page::Traffic => self.refresh_traffic(),
                    Page::Scanner | Page::Diagnostics | Page::Settings => Vec::new(),
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
                self.scan_concurrency = self.scan_concurrency.saturating_sub(10).max(10);
                return vec![Effect::PersistPreferences(self.preferences())];
            }
            Right if self.page == Page::Settings => {
                self.scan_concurrency = (self.scan_concurrency + 10).min(500);
                return vec![Effect::PersistPreferences(self.preferences())];
            }
            Left | Right | Edit | Confirm | Toggle => {}
        }
        Vec::new()
    }

    fn navigate(&mut self, delta: isize) {
        match self.page {
            Page::Adapters if !self.adapters.items.is_empty() => {
                self.adapters.selected =
                    wrap(self.adapters.selected, self.adapters.items.len(), delta)
            }
            Page::Traffic if !self.traffic.rows.is_empty() => {
                self.traffic.selected = wrap(self.traffic.selected, self.traffic.rows.len(), delta)
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

    fn refresh_dashboard(&mut self) -> Vec<Effect> {
        let job = self.next_job(ToolKind::Dashboard);
        self.dashboard.job = Some(job);
        self.dashboard.status = TaskStatus::Running;
        self.dashboard.error = None;
        vec![Effect::RefreshDashboard {
            job,
            request: crate::DashboardRequest {
                public_ip: self.public_ip_config.clone(),
            },
        }]
    }

    fn refresh_adapters(&mut self) -> Vec<Effect> {
        let job = self.next_job(ToolKind::Adapters);
        self.adapters.job = Some(job);
        self.adapters.status = TaskStatus::Running;
        self.adapters.error = None;
        vec![Effect::RefreshAdapters { job }]
    }

    fn refresh_traffic(&mut self) -> Vec<Effect> {
        let job = self.next_job(ToolKind::Traffic);
        self.traffic.job = Some(job);
        self.traffic.status = TaskStatus::Running;
        self.traffic.error = None;
        vec![Effect::RefreshTraffic { job }]
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
        if let Some(job) = self.diagnostics.active_common().job {
            let common = self.diagnostics.active_common_mut();
            common.job = None;
            common.status = TaskStatus::Done;
            return vec![stop_effect(job)];
        }

        let tool = ToolKind::from(self.diagnostics.tool);
        let job = self.next_job(tool);
        let common = self.diagnostics.active_common_mut();
        common.job = Some(job);
        common.status = TaskStatus::Running;
        common.progress = 0;
        common.primary.clear();
        common.detail.clear();
        common.log.clear();

        let effect = match self.diagnostics.tool {
            DiagnosticTool::Ping => {
                self.diagnostics.ping.samples.clear();
                self.diagnostics.ping.summary = None;
                Effect::StartPing {
                    job,
                    request: self.diagnostics.ping.request.clone(),
                }
            }
            DiagnosticTool::Trace => {
                self.diagnostics.trace.hops.clear();
                Effect::StartTrace {
                    job,
                    request: self.diagnostics.trace.request.clone(),
                }
            }
            DiagnosticTool::PortScan => {
                self.diagnostics.port_scan.scanned = 0;
                self.diagnostics.port_scan.total = 0;
                self.diagnostics.port_scan.open_ports.clear();
                self.diagnostics.port_scan.request.concurrency = self.scan_concurrency;
                Effect::StartPortScan {
                    job,
                    request: self.diagnostics.port_scan.request.clone(),
                }
            }
            DiagnosticTool::PublicSpeed => {
                self.diagnostics.public_speed.samples.clear();
                self.diagnostics.public_speed.summary = None;
                Effect::StartPublicSpeed {
                    job,
                    request: self.diagnostics.public_speed.request.clone(),
                }
            }
            DiagnosticTool::LinkQuality => {
                self.diagnostics.link_quality.samples.clear();
                self.diagnostics.link_quality.summary = None;
                Effect::StartLinkQuality {
                    job,
                    request: self.diagnostics.link_quality.request.clone(),
                }
            }
            DiagnosticTool::LanSpeed => {
                self.diagnostics.lan_speed.samples.clear();
                self.diagnostics.lan_speed.summary = None;
                Effect::StartLanSpeed {
                    job,
                    request: self.diagnostics.lan_speed.request.clone(),
                }
            }
        };
        vec![effect]
    }

    fn handle_runtime(&mut self, event: RuntimeEvent) {
        match event {
            RuntimeEvent::DashboardUpdated(snapshot) => self.dashboard.snapshot = *snapshot,
            RuntimeEvent::DashboardRefreshFinished { job, snapshot }
                if self.dashboard.job == Some(job) =>
            {
                self.dashboard.snapshot = *snapshot;
                self.dashboard.status = TaskStatus::Done;
                self.dashboard.error = None;
                self.dashboard.job = None;
            }
            RuntimeEvent::DashboardRefreshFailed {
                job,
                snapshot,
                error,
            } if self.dashboard.job == Some(job) => {
                self.dashboard.snapshot = *snapshot;
                self.dashboard.status = TaskStatus::Failed(error.message.clone());
                self.dashboard.error = Some(error);
                self.dashboard.job = None;
            }
            RuntimeEvent::DashboardRefreshCancelled { job } if self.dashboard.job == Some(job) => {
                self.dashboard.status = TaskStatus::Done;
                self.dashboard.job = None;
            }
            RuntimeEvent::AdaptersUpdated(adapters) => self.adapters.items = adapters,
            RuntimeEvent::TrafficUpdated(rows) => self.traffic.rows = rows,
            RuntimeEvent::AdaptersRefreshFinished { job, adapters }
                if self.adapters.job == Some(job) =>
            {
                let selected_name = self
                    .adapters
                    .items
                    .get(self.adapters.selected)
                    .map(|adapter| adapter.name.as_str());
                self.adapters.selected = selected_name
                    .and_then(|name| adapters.iter().position(|adapter| adapter.name == name))
                    .unwrap_or(0)
                    .min(adapters.len().saturating_sub(1));
                self.adapters.items = adapters;
                self.adapters.status = TaskStatus::Done;
                self.adapters.error = None;
                self.adapters.job = None;
            }
            RuntimeEvent::AdaptersRefreshFailed { job, error }
                if self.adapters.job == Some(job) =>
            {
                self.adapters.status = TaskStatus::Failed(error.message.clone());
                self.adapters.error = Some(error);
                self.adapters.job = None;
            }
            RuntimeEvent::AdaptersRefreshCancelled { job } if self.adapters.job == Some(job) => {
                self.adapters.status = TaskStatus::Done;
                self.adapters.job = None;
            }
            RuntimeEvent::TrafficRefreshFinished { job, rows } if self.traffic.job == Some(job) => {
                let selected_name = self
                    .traffic
                    .rows
                    .get(self.traffic.selected)
                    .map(|row| row.name.as_str());
                self.traffic.selected = selected_name
                    .and_then(|name| rows.iter().position(|row| row.name == name))
                    .unwrap_or(0)
                    .min(rows.len().saturating_sub(1));
                self.traffic.rows = rows;
                self.traffic.status = TaskStatus::Done;
                self.traffic.error = None;
                self.traffic.job = None;
            }
            RuntimeEvent::TrafficRefreshFailed { job, error } if self.traffic.job == Some(job) => {
                self.traffic.status = TaskStatus::Failed(error.message.clone());
                self.traffic.error = Some(error);
                self.traffic.job = None;
            }
            RuntimeEvent::TrafficRefreshCancelled { job } if self.traffic.job == Some(job) => {
                self.traffic.status = TaskStatus::Done;
                self.traffic.job = None;
            }
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
            RuntimeEvent::PingStarted { job } if self.diagnostics.ping.common.job == Some(job) => {
                self.diagnostics.ping.common.status = TaskStatus::Running;
            }
            RuntimeEvent::PingSample { job, sample }
                if self.diagnostics.ping.common.job == Some(job) =>
            {
                let primary = sample.latency_ms.map_or_else(
                    || format!("sequence {} timed out", sample.sequence),
                    |latency| format!("reply {}: {latency} ms", sample.sequence),
                );
                let common = &mut self.diagnostics.ping.common;
                common.progress = (sample.sequence.saturating_add(1) * 12).min(99) as u8;
                common.primary = primary.clone();
                common.detail = format!("ttl={:?} size={}", sample.ttl, sample.size);
                common.log.push(primary);
                self.diagnostics.ping.samples.push(sample);
            }
            RuntimeEvent::PingFinished { job, summary }
                if self.diagnostics.ping.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.ping.common,
                    format!(
                        "{} received / {} sent · {:.1}% loss",
                        summary.received, summary.sent, summary.loss_percent
                    ),
                );
                self.diagnostics.ping.summary = Some(summary);
            }
            RuntimeEvent::PingFailed { job, error }
                if self.diagnostics.ping.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.ping.common, error);
            }
            RuntimeEvent::TraceStarted { job }
                if self.diagnostics.trace.common.job == Some(job) =>
            {
                self.diagnostics.trace.common.status = TaskStatus::Running;
            }
            RuntimeEvent::TraceHop { job, hop }
                if self.diagnostics.trace.common.job == Some(job) =>
            {
                let primary = format!("hop {}: {}", hop.ttl, hop.address.as_deref().unwrap_or("*"));
                let common = &mut self.diagnostics.trace.common;
                common.progress = hop
                    .ttl
                    .saturating_mul(100)
                    .checked_div(self.diagnostics.trace.request.max_hops.max(1))
                    .unwrap_or(0)
                    .min(99);
                common.primary = primary.clone();
                common.detail = hop
                    .latency_ms
                    .map_or_else(|| "timeout".into(), |latency| format!("{latency} ms"));
                common.log.push(primary);
                self.diagnostics.trace.hops.push(hop);
            }
            RuntimeEvent::TraceFinished { job, hops }
                if self.diagnostics.trace.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.trace.common,
                    format!("route completed in {hops} hops"),
                );
            }
            RuntimeEvent::TraceFailed { job, error }
                if self.diagnostics.trace.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.trace.common, error);
            }
            RuntimeEvent::PortScanStarted { job, total }
                if self.diagnostics.port_scan.common.job == Some(job) =>
            {
                self.diagnostics.port_scan.total = total;
                self.diagnostics.port_scan.common.status = TaskStatus::Running;
            }
            RuntimeEvent::PortScanProgress {
                job,
                scanned,
                total,
            } if self.diagnostics.port_scan.common.job == Some(job) => {
                let state = &mut self.diagnostics.port_scan;
                state.scanned = scanned;
                state.total = total;
                state.common.progress = scanned
                    .saturating_mul(100)
                    .checked_div(total.max(1))
                    .unwrap_or(0)
                    .min(100) as u8;
                state.common.primary = format!("scanned {scanned} ports");
                state.common.detail = format!("{} open", state.open_ports.len());
            }
            RuntimeEvent::PortScanOpen { job, port }
                if self.diagnostics.port_scan.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.port_scan;
                if let Err(index) = state.open_ports.binary_search(&port) {
                    state.open_ports.insert(index, port);
                }
                let line = format!("open: {port}");
                state.common.primary = line.clone();
                state.common.log.push(line);
            }
            RuntimeEvent::PortScanFinished {
                job,
                scanned,
                total,
                cancelled,
            } if self.diagnostics.port_scan.common.job == Some(job) => {
                let state = &mut self.diagnostics.port_scan;
                state.scanned = scanned;
                state.total = total;
                finish_common(
                    &mut state.common,
                    format!(
                        "{} · {} open ports",
                        if cancelled { "cancelled" } else { "completed" },
                        state.open_ports.len()
                    ),
                );
            }
            RuntimeEvent::PortScanFailed { job, error }
                if self.diagnostics.port_scan.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.port_scan.common, error);
            }
            RuntimeEvent::PublicSpeedStarted { job, server }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                self.diagnostics.public_speed.server = server;
                self.diagnostics.public_speed.common.status = TaskStatus::Running;
            }
            RuntimeEvent::PublicSpeedSample { job, sample }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.public_speed;
                state.common.progress = sample
                    .elapsed_ms
                    .saturating_mul(100)
                    .checked_div(state.request.max_duration_ms.max(1))
                    .unwrap_or(0)
                    .min(99) as u8;
                state.common.primary = format!("{} bps", sample.bits_per_second);
                state.common.detail = format!("{} bytes", sample.bytes);
                state.samples.push(sample);
            }
            RuntimeEvent::PublicSpeedFinished { job, summary }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.public_speed.common,
                    format!(
                        "average {} bps · peak {} bps",
                        summary.average_bps, summary.peak_bps
                    ),
                );
                self.diagnostics.public_speed.summary = Some(summary);
            }
            RuntimeEvent::PublicSpeedFailed { job, error }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.public_speed.common, error);
            }
            RuntimeEvent::LinkQualityStarted { job }
                if self.diagnostics.link_quality.common.job == Some(job) =>
            {
                self.diagnostics.link_quality.common.status = TaskStatus::Running;
            }
            RuntimeEvent::LinkQualitySample { job, sample }
                if self.diagnostics.link_quality.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.link_quality;
                state.common.progress = sample
                    .sequence
                    .saturating_mul(100)
                    .checked_div(state.request.count.max(1))
                    .unwrap_or(0)
                    .min(99) as u8;
                state.common.primary = format!("latency={:?} ms", sample.latency_ms);
                state.common.detail = format!(
                    "loss={:.1}% · rssi={:?}",
                    sample.loss_percent, sample.rssi_dbm
                );
                state.samples.push(sample);
            }
            RuntimeEvent::LinkQualityFinished { job, summary }
                if self.diagnostics.link_quality.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.link_quality.common,
                    format!(
                        "score {:.0}/100 · loss {:.1}%",
                        summary.score, summary.loss_percent
                    ),
                );
                self.diagnostics.link_quality.summary = Some(summary);
            }
            RuntimeEvent::LinkQualityFailed { job, error }
                if self.diagnostics.link_quality.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.link_quality.common, error);
            }
            RuntimeEvent::LanSpeedStarted { job }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                self.diagnostics.lan_speed.common.status = TaskStatus::Running;
            }
            RuntimeEvent::LanSpeedSample { job, sample }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.lan_speed;
                state.common.progress = sample
                    .elapsed_ms
                    .saturating_mul(100)
                    .checked_div(state.request.duration_secs.saturating_mul(1_000).max(1))
                    .unwrap_or(0)
                    .min(99) as u8;
                state.common.primary =
                    format!("tx={} bps · rx={} bps", sample.tx_bps, sample.rx_bps);
                state.common.detail = format!(
                    "loss={:?} · jitter={:?}",
                    sample.loss_percent, sample.jitter_ms
                );
                state.samples.push(sample);
            }
            RuntimeEvent::LanSpeedFinished { job, summary }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.lan_speed.common,
                    format!(
                        "tx={} bytes · rx={} bytes",
                        summary.tx_bytes, summary.rx_bytes
                    ),
                );
                self.diagnostics.lan_speed.summary = Some(summary);
            }
            RuntimeEvent::LanSpeedFailed { job, error }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.lan_speed.common, error);
            }
            _ => {}
        }
    }
}

fn finish_common(common: &mut DiagnosticCommonState, summary: String) {
    common.status = TaskStatus::Done;
    common.progress = 100;
    common.detail = summary;
    common.job = None;
}

fn fail_common(common: &mut DiagnosticCommonState, error: crate::RuntimeError) {
    common.status = TaskStatus::Failed(error.message);
    common.job = None;
}

fn wrap(current: usize, len: usize, delta: isize) -> usize {
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn stop_effect(job: JobId) -> Effect {
    match job.tool {
        ToolKind::Dashboard => unreachable!("dashboard refreshes are not diagnostic jobs"),
        ToolKind::Adapters | ToolKind::Traffic => {
            unreachable!("read-only refreshes are not diagnostic jobs")
        }
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

    #[test]
    fn typed_ping_events_ignore_stale_and_post_cancel_samples() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPing { job, request } = effects[0].clone() else {
            panic!("expected typed ping effect");
        };
        assert_eq!(request, crate::PingRequest::default());

        let sample = crate::PingSample {
            sequence: 1,
            latency_ms: Some(12),
            ttl: Some(64),
            size: 32,
        };
        app.update(Runtime(RuntimeEvent::PingSample {
            job: JobId {
                generation: job.generation + 1,
                ..job
            },
            sample: sample.clone(),
        }));
        assert!(app.diagnostics.ping.samples.is_empty());

        app.update(Runtime(RuntimeEvent::PingSample {
            job,
            sample: sample.clone(),
        }));
        assert_eq!(app.diagnostics.ping.samples.first(), Some(&sample));

        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopPing(job)]
        );
        app.update(Runtime(RuntimeEvent::PingSample { job, sample }));
        assert_eq!(app.diagnostics.ping.samples.len(), 1);
    }

    #[test]
    fn typed_failure_only_mutates_its_current_tool_generation() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.tool = DiagnosticTool::Trace;
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartTrace { job, request } = effects[0].clone() else {
            panic!("expected typed trace effect");
        };
        assert_eq!(request, crate::TraceRequest::default());

        app.update(Runtime(RuntimeEvent::TraceFailed {
            job,
            error: crate::RuntimeError::new(crate::RuntimeErrorCode::Timeout, "trace timeout"),
        }));
        assert_eq!(
            app.diagnostics.trace.common.status,
            TaskStatus::Failed("trace timeout".into())
        );
        assert_eq!(app.diagnostics.trace.common.job, None);
        assert_eq!(app.diagnostics.ping.common.status, TaskStatus::Idle);
    }

    #[test]
    fn port_scan_effect_carries_validated_shape_not_generic_text() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.tool = DiagnosticTool::PortScan;
        app.scan_concurrency = 64;
        app.diagnostics.port_scan.request = crate::PortScanRequest {
            target: "192.0.2.10".into(),
            start_port: 20,
            end_port: 443,
            timeout_ms: 250,
            concurrency: 64,
        };
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPortScan { request, .. } = &effects[0] else {
            panic!("expected typed port scan effect");
        };
        assert_eq!(request.start_port, 20);
        assert_eq!(request.end_port, 443);
        assert_eq!(request.concurrency, 64);
    }

    #[test]
    fn settings_emit_explicit_persistence_effects() {
        let mut app = AppModel {
            page: Page::Settings,
            ..AppModel::default()
        };
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Right))),
            [Effect::PersistPreferences(crate::Preferences {
                language: Language::En,
                scan_concurrency: 60,
            })]
        );
        assert_eq!(app.scan_concurrency, 60);

        assert_eq!(
            app.update(Input(InputEvent::Action(Action::ToggleLanguage))),
            [Effect::PersistPreferences(crate::Preferences {
                language: Language::Zh,
                scan_concurrency: 60,
            })]
        );
    }

    #[test]
    fn dashboard_refresh_uses_config_and_ignores_stale_generations() {
        let mut app = AppModel::default();
        app.apply_config(&crate::ConfigData {
            public_ip: crate::PublicIpConfig {
                endpoints: vec![crate::Endpoint {
                    url: "http://127.0.0.1:9876/ip".into(),
                    kind: "plaintext".into(),
                }],
                use_system_proxy: false,
            },
            ..crate::ConfigData::default()
        });

        let [first_effect] = app
            .update(Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let first = match first_effect {
            Effect::RefreshDashboard { job, request } => {
                assert_eq!(request.public_ip.endpoints[0].kind, "plaintext");
                assert!(!request.public_ip.use_system_proxy);
                job
            }
            other => panic!("unexpected effect: {other:?}"),
        };
        let [second_effect] = app
            .update(Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let second = match second_effect {
            Effect::RefreshDashboard { job, .. } => job,
            other => panic!("unexpected effect: {other:?}"),
        };
        assert!(second.generation > first.generation);

        let stale = DashboardSnapshot {
            hostname: "stale-host".into(),
            ..DashboardSnapshot::default()
        };
        app.update(Runtime(RuntimeEvent::DashboardRefreshFinished {
            job: first,
            snapshot: Box::new(stale),
        }));
        assert_eq!(app.dashboard.job, Some(second));
        assert_ne!(app.dashboard.snapshot.hostname, "stale-host");

        let current = DashboardSnapshot {
            hostname: "current-host".into(),
            ..DashboardSnapshot::default()
        };
        app.update(Runtime(RuntimeEvent::DashboardRefreshFailed {
            job: second,
            snapshot: Box::new(current),
            error: crate::RuntimeError::new(crate::RuntimeErrorCode::Network, "offline"),
        }));
        assert_eq!(app.dashboard.job, None);
        assert_eq!(app.dashboard.snapshot.hostname, "current-host");
        assert!(matches!(app.dashboard.status, TaskStatus::Failed(_)));
        assert_eq!(
            app.dashboard.error.as_ref().unwrap().code,
            crate::RuntimeErrorCode::Network
        );
    }

    #[test]
    fn adapter_and_traffic_refreshes_are_job_scoped_and_preserve_selection() {
        let mut app = AppModel {
            page: Page::Adapters,
            ..AppModel::default()
        };
        app.adapters.items = vec![
            AdapterInfo {
                name: "Ethernet".into(),
                ..AdapterInfo::default()
            },
            AdapterInfo {
                name: "Wi-Fi".into(),
                ..AdapterInfo::default()
            },
        ];
        app.adapters.selected = 1;
        let [first_effect] = app
            .update(Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let first = match first_effect {
            Effect::RefreshAdapters { job } => job,
            other => panic!("unexpected effect: {other:?}"),
        };
        let [second_effect] = app
            .update(Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let second = match second_effect {
            Effect::RefreshAdapters { job } => job,
            other => panic!("unexpected effect: {other:?}"),
        };
        app.update(Runtime(RuntimeEvent::AdaptersRefreshFinished {
            job: first,
            adapters: vec![AdapterInfo {
                name: "stale".into(),
                ..AdapterInfo::default()
            }],
        }));
        assert_eq!(app.adapters.job, Some(second));
        assert_eq!(app.adapters.items[1].name, "Wi-Fi");

        app.update(Runtime(RuntimeEvent::AdaptersRefreshFinished {
            job: second,
            adapters: vec![
                AdapterInfo {
                    name: "Wi-Fi".into(),
                    ..AdapterInfo::default()
                },
                AdapterInfo {
                    name: "Ethernet".into(),
                    ..AdapterInfo::default()
                },
            ],
        }));
        assert_eq!(app.adapters.selected, 0);
        assert_eq!(app.adapters.status, TaskStatus::Done);

        app.page = Page::Traffic;
        let [effect] = app
            .update(Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let job = match effect {
            Effect::RefreshTraffic { job } => job,
            other => panic!("unexpected effect: {other:?}"),
        };
        app.update(Runtime(RuntimeEvent::TrafficRefreshFinished {
            job,
            rows: vec![TrafficRow {
                name: "Wi-Fi".into(),
                download_bps: 1_024,
                ..TrafficRow::default()
            }],
        }));
        assert_eq!(app.traffic.rows[0].download_bps, 1_024);
        assert_eq!(app.traffic.status, TaskStatus::Done);
    }

    #[test]
    fn shared_model_loads_preferences_from_v031_config_data() {
        let config = crate::ConfigData {
            language: Language::Zh,
            scan_concurrency: 120,
            ..crate::ConfigData::default()
        };
        let mut app = AppModel::default();
        app.apply_config(&config);
        assert_eq!(app.language, Language::Zh);
        assert_eq!(app.scan_concurrency, 120);
    }
}
