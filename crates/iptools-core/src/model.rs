use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::net::IpAddr;

use crate::{
    Action, AdapterEditParams, AdapterValidationError, Effect, InputEvent, JobId, KeyCode,
    Message::*, RuntimeEvent, ScanRequest, ToolKind,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Message {
    Input(InputEvent),
    Tick(u64),
    Clock(String),
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
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    #[default]
    Classic,
    Nord,
    CatppuccinMocha,
    Dracula,
}

impl ThemeId {
    pub const ALL: [Self; 4] = [
        Self::Classic,
        Self::Nord,
        Self::CatppuccinMocha,
        Self::Dracula,
    ];

    pub const fn next(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }

    pub const fn previous(self) -> Self {
        Self::ALL[(self as usize + Self::ALL.len() - 1) % Self::ALL.len()]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DiagnosticFocus {
    #[default]
    Menu,
    Main,
    Config,
}

impl DiagnosticFocus {
    fn next(self) -> Self {
        match self {
            Self::Menu => Self::Main,
            Self::Main => Self::Config,
            Self::Config => Self::Menu,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Menu => Self::Config,
            Self::Main => Self::Menu,
            Self::Config => Self::Main,
        }
    }
}

impl DiagnosticTool {
    pub const ALL: [Self; 6] = [
        Self::Ping,
        Self::Trace,
        Self::PortScan,
        Self::LinkQuality,
        Self::PublicSpeed,
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
    pub cidr: Option<String>,
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
    pub edit: Option<AdapterEditState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AdapterField {
    #[default]
    Mode,
    Ipv4,
    Mask,
    Gateway,
    Dns1,
    Dns2,
}

impl AdapterField {
    pub const ALL: [Self; 6] = [
        Self::Mode,
        Self::Ipv4,
        Self::Mask,
        Self::Gateway,
        Self::Dns1,
        Self::Dns2,
    ];

    fn index(self) -> usize {
        self as usize
    }

    fn from_index(index: usize) -> Self {
        Self::ALL[index.min(Self::ALL.len() - 1)]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdapterEditPhase {
    Editing,
    Confirming,
    Applying,
    Succeeded(crate::AdapterApplyOutcome),
    Failed(crate::RuntimeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterEditState {
    pub guid: String,
    pub name: String,
    pub params: AdapterEditParams,
    pub selected: AdapterField,
    pub cursor: usize,
    pub phase: AdapterEditPhase,
    pub validation_error: Option<AdapterValidationError>,
    pub history: Vec<String>,
    pub history_open: bool,
    pub history_selected: usize,
    pub job: Option<JobId>,
}

impl AdapterEditState {
    pub fn value(&self, field: AdapterField) -> &str {
        match field {
            AdapterField::Mode => {
                if self.params.use_dhcp {
                    "DHCP"
                } else {
                    "Static"
                }
            }
            AdapterField::Ipv4 => &self.params.ip,
            AdapterField::Mask => &self.params.mask,
            AdapterField::Gateway => &self.params.gateway,
            AdapterField::Dns1 => &self.params.dns1,
            AdapterField::Dns2 => &self.params.dns2,
        }
    }

    fn value_mut(&mut self, field: AdapterField) -> Option<&mut String> {
        match field {
            AdapterField::Mode => None,
            AdapterField::Ipv4 => Some(&mut self.params.ip),
            AdapterField::Mask => Some(&mut self.params.mask),
            AdapterField::Gateway => Some(&mut self.params.gateway),
            AdapterField::Dns1 => Some(&mut self.params.dns1),
            AdapterField::Dns2 => Some(&mut self.params.dns2),
        }
    }
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
    #[serde(default)]
    pub vendor: String,
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
    #[serde(skip, default = "scanner_auto_cidr_default")]
    pub auto_cidr: bool,
    pub editing: bool,
    pub cursor: usize,
    pub history: Vec<String>,
    pub history_open: bool,
    pub history_selected: usize,
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
            auto_cidr: true,
            editing: false,
            cursor: 0,
            history: Vec::new(),
            history_open: false,
            history_selected: 0,
            status: TaskStatus::Idle,
            current: 0,
            total: 0,
            results: Vec::new(),
            selected: 0,
            job: None,
        }
    }
}

const fn scanner_auto_cidr_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticCommonState {
    pub status: TaskStatus,
    pub error: Option<crate::RuntimeError>,
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
            error: None,
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
    pub config_selected: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceState {
    pub request: crate::TraceRequest,
    pub common: DiagnosticCommonState,
    pub hops: Vec<crate::TraceHop>,
    pub max_hops_input: String,
    pub timeout_input: String,
    pub config_selected: usize,
    pub selected: usize,
}

impl Default for TraceState {
    fn default() -> Self {
        let request = crate::TraceRequest::default();
        Self {
            max_hops_input: request.max_hops.to_string(),
            timeout_input: request.timeout_ms.to_string(),
            request,
            common: DiagnosticCommonState::default(),
            hops: Vec::new(),
            config_selected: 0,
            selected: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PortScanState {
    pub request: crate::PortScanRequest,
    pub persist: crate::PortScanPersist,
    pub config_selected: usize,
    pub selected: usize,
    pub common: DiagnosticCommonState,
    pub scanned: u64,
    pub total: u64,
    pub open_ports: Vec<crate::PortScanResult>,
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
    pub adapters: Vec<crate::LinkQualityAdapter>,
    pub selected_adapter: usize,
    pub params: crate::LinkParams,
    pub persist: crate::LinkQualityPersist,
    pub config_selected: usize,
    pub snapshot: Option<crate::LinkQualitySnapshot>,
    pub samples: Vec<crate::LinkQualitySample>,
    pub summary: Option<crate::LinkQualitySummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct LanSpeedState {
    pub request: crate::LanSpeedRequest,
    pub persist: crate::LanSpeedPersist,
    pub config_selected: usize,
    pub endpoint: String,
    pub phase: Option<crate::LanSpeedPhase>,
    pub common: DiagnosticCommonState,
    pub samples: Vec<crate::LanSpeedSample>,
    pub summary: Option<crate::LanSpeedSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiagnosticsState {
    pub tool: DiagnosticTool,
    pub ping: PingState,
    pub trace: TraceState,
    pub port_scan: PortScanState,
    pub public_speed: PublicSpeedState,
    pub link_quality: LinkQualityState,
    pub lan_speed: LanSpeedState,
    pub focused: bool,
    pub focus: DiagnosticFocus,
    pub cursor: usize,
    pub target_history: Vec<String>,
    pub history_open: bool,
    pub history_selected: usize,
}

impl Default for DiagnosticsState {
    fn default() -> Self {
        Self {
            tool: DiagnosticTool::default(),
            ping: PingState::default(),
            trace: TraceState::default(),
            port_scan: PortScanState::default(),
            public_speed: PublicSpeedState::default(),
            link_quality: LinkQualityState::default(),
            lan_speed: LanSpeedState::default(),
            focused: false,
            focus: DiagnosticFocus::Menu,
            cursor: 0,
            target_history: Vec::new(),
            history_open: false,
            history_selected: 0,
        }
    }
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
    pub theme: ThemeId,
    pub show_help: bool,
    pub dashboard: DashboardState,
    pub adapters: AdaptersState,
    pub scanner: ScannerState,
    pub traffic: TrafficState,
    pub diagnostics: DiagnosticsState,
    pub scan_concurrency: usize,
    #[serde(default)]
    pub settings_selected: usize,
    #[serde(default)]
    pub settings_just_reset: bool,
    #[serde(default)]
    pub keybindings: crate::PersistedKeymap,
    #[serde(default)]
    public_ip_config: crate::PublicIpConfig,
    adapter_edit_persist: crate::AdapterEditPersist,
    adapter_history: Vec<String>,
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
            theme: ThemeId::Classic,
            show_help: false,
            dashboard: DashboardState::default(),
            adapters: AdaptersState::default(),
            scanner: ScannerState::default(),
            traffic: TrafficState::default(),
            diagnostics: DiagnosticsState::default(),
            scan_concurrency: 50,
            settings_selected: 0,
            settings_just_reset: false,
            keybindings: crate::PersistedKeymap::new(),
            public_ip_config: crate::PublicIpConfig::default(),
            adapter_edit_persist: crate::AdapterEditPersist::default(),
            adapter_history: Vec::new(),
            generation: 0,
        }
    }
}

impl AppModel {
    pub fn apply_config(&mut self, config: &crate::ConfigData) {
        self.language = config.language;
        self.theme = config.theme;
        self.scan_concurrency = config.scan_concurrency.clamp(10, 500);
        self.keybindings = config.keybindings.clone();
        self.public_ip_config = config.public_ip.clone();
        self.adapter_edit_persist = config.session.adapter_edit.clone();
        self.adapter_history = config.session.history.adapter.clone();
        self.scanner.cidr = if config.session.scanner.cidr.trim().is_empty() {
            ScannerState::default().cidr
        } else {
            config.session.scanner.cidr.clone()
        };
        self.scanner.cursor = self.scanner.cidr.len();
        self.scanner.auto_cidr = true;
        self.scanner.history = config.session.history.cidrs.clone();
        self.diagnostics.ping.request = crate::PingRequest {
            target: config.session.ping.target.clone(),
            interval_ms: config.session.ping.interval_ms.clamp(100, 10_000),
            timeout_ms: config.session.ping.timeout_ms.clamp(100, 10_000),
            packet_size: config.session.ping.packet_size.min(65_500),
        };
        self.diagnostics.trace.request.target = config.session.trace.target.clone();
        self.diagnostics.trace.max_hops_input = config.session.trace.max_hops.clone();
        self.diagnostics.trace.timeout_input = config.session.trace.timeout_ms.clone();
        self.sync_trace_request();
        self.diagnostics.port_scan.persist = config.session.port_scan.clone();
        self.sync_port_scan_request();
        self.diagnostics.lan_speed.persist = config.session.lan_speed.clone();
        self.sync_lan_speed_request();
        self.diagnostics.link_quality.persist = config.session.link_quality.clone();
        self.diagnostics.link_quality.params = self
            .diagnostics
            .link_quality
            .persist
            .selected
            .as_ref()
            .and_then(|key| self.diagnostics.link_quality.persist.adapters.get(key))
            .cloned()
            .unwrap_or_default();
        self.sync_link_quality_request();
        self.diagnostics.target_history = config.session.history.targets.clone();
        self.page = Page::from_index(config.session.ui.last_tab);
        self.diagnostics.tool = DiagnosticTool::from_index(config.session.ui.last_diag_tool);
    }

    pub const fn preferences(&self) -> crate::Preferences {
        crate::Preferences {
            language: self.language,
            theme: self.theme,
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
            Clock(observed_at) => {
                self.dashboard.snapshot.observed_at = observed_at;
                Vec::new()
            }
            Runtime(event) => {
                self.handle_runtime(event);
                Vec::new()
            }
        }
    }

    fn handle_input(&mut self, input: InputEvent) -> Vec<Effect> {
        if self.page == Page::Adapters && self.adapters.edit.is_some() {
            let global = input.action();
            if matches!(
                global,
                Some(
                    Action::Quit
                        | Action::ToggleLanguage
                        | Action::Help
                        | Action::NextPage
                        | Action::PreviousPage
                        | Action::SelectPage(_)
                        | Action::ResetDemo
                )
            ) {
                return self.handle_action(global.expect("matched global action"));
            }
            return self.handle_adapter_edit_input(input);
        }

        if self.page == Page::Diagnostics {
            let action = input.action();
            if let Some(Action::SelectDiagnostic(index)) = action {
                return self.handle_action(Action::SelectDiagnostic(index));
            }
            if let Some(Action::SelectDiagnosticHistory(index)) = action {
                if let Some(value) = self.diagnostics.target_history.get(index).cloned() {
                    self.set_active_diagnostic_field(value);
                    self.diagnostics.cursor = self.active_diagnostic_field().len();
                    self.diagnostics.history_open = false;
                    return self.persist_active_diagnostic();
                }
                return Vec::new();
            }
            if let Some(Action::FocusDiagnostic(focus)) = action {
                self.diagnostics.focused = true;
                self.diagnostics.focus = focus;
                self.diagnostics.history_open = false;
                return Vec::new();
            }
            if let Some(Action::SelectDiagnosticField(index, cursor)) = action {
                self.diagnostics.focused = true;
                self.diagnostics.focus = DiagnosticFocus::Config;
                self.set_diagnostic_config_index(index);
                self.diagnostics.cursor = cursor.min(self.active_diagnostic_field().len());
                return Vec::new();
            }
            if self.diagnostics.focused {
                if matches!(
                    action,
                    Some(
                        Action::Quit
                            | Action::ToggleLanguage
                            | Action::Help
                            | Action::SelectPage(_)
                    )
                ) {
                    return self.handle_action(action.expect("matched global action"));
                }
                return self.handle_diagnostic_input(input);
            }
        }
        if self.page == Page::Scanner {
            let action = input.action();
            if self.scanner.editing
                || matches!(
                    action,
                    Some(
                        Action::SelectScannerInput(_)
                            | Action::ActivateScannerPanel
                            | Action::SelectScannerHistory(_)
                    )
                )
            {
                if matches!(
                    action,
                    Some(
                        Action::Quit
                            | Action::ToggleLanguage
                            | Action::Help
                            | Action::NextPage
                            | Action::PreviousPage
                            | Action::SelectPage(_)
                            | Action::ResetDemo
                    )
                ) {
                    return self.handle_action(action.expect("matched global action"));
                }
                return self.handle_scanner_input(input);
            }
        }

        let action = input.action();
        action.map_or_else(Vec::new, |action| self.handle_action(action))
    }

    fn handle_scanner_input(&mut self, input: InputEvent) -> Vec<Effect> {
        let action = input.action();

        if let Some(Action::SelectScannerInput(cursor)) = action {
            self.scanner.editing = true;
            self.scanner.cursor = cursor.min(self.scanner.cidr.len());
            self.scanner.history_open = false;
            return Vec::new();
        }

        if let Some(Action::ActivateScannerPanel) = action {
            if self.scanner.editing {
                self.scanner.editing = false;
                self.scanner.history_open = false;
                return self.persist_scanner();
            }
            return self.toggle_scan();
        }

        if let Some(Action::SelectScannerHistory(index)) = action {
            if let Some(value) = self.scanner.history.get(index).cloned() {
                self.scanner.cidr = value;
                self.scanner.auto_cidr = false;
                self.scanner.cursor = self.scanner.cidr.len();
                self.scanner.history_open = false;
                return self.persist_scanner();
            }
            return Vec::new();
        }

        if self.scanner.history_open {
            match action {
                Some(Action::Up) => {
                    self.scanner.history_selected = self.scanner.history_selected.saturating_sub(1)
                }
                Some(Action::Down) => {
                    self.scanner.history_selected = (self.scanner.history_selected + 1)
                        .min(self.scanner.history.len().min(8).saturating_sub(1));
                }
                Some(Action::Confirm | Action::Toggle) => {
                    if let Some(value) = self
                        .scanner
                        .history
                        .get(self.scanner.history_selected)
                        .cloned()
                    {
                        self.scanner.cidr = value;
                        self.scanner.auto_cidr = false;
                        self.scanner.cursor = self.scanner.cidr.len();
                        self.scanner.history_open = false;
                        return self.persist_scanner();
                    }
                }
                Some(Action::Back) => self.scanner.history_open = false,
                _ => self.scanner.history_open = false,
            }
            return Vec::new();
        }

        match action {
            Some(Action::History) => {
                self.scanner.history_open = true;
                self.scanner.history_selected = 0;
                return Vec::new();
            }
            Some(Action::Confirm | Action::Toggle | Action::Back) => {
                self.scanner.editing = false;
                return self.persist_scanner();
            }
            Some(Action::Right)
                if self.scanner.cursor == self.scanner.cidr.len()
                    && !self.scanner.cidr.is_empty() =>
            {
                if let Some(value) = self
                    .scanner
                    .history
                    .iter()
                    .find(|value| {
                        value.starts_with(&self.scanner.cidr)
                            && value.len() > self.scanner.cidr.len()
                    })
                    .cloned()
                {
                    self.scanner.cidr = value;
                    self.scanner.auto_cidr = false;
                    self.scanner.cursor = self.scanner.cidr.len();
                    return self.persist_scanner();
                }
            }
            _ => {}
        }

        if let Some(key) = input.key() {
            let mut value = self.scanner.cidr.clone();
            if edit_ascii(
                &mut value,
                &mut self.scanner.cursor,
                key.code,
                |character| character.is_ascii_digit() || matches!(character, '.' | '/'),
            ) && value.len() <= 32
            {
                self.scanner.cidr = value;
                self.scanner.auto_cidr = false;
                return self.persist_scanner();
            }
        }
        Vec::new()
    }

    fn persist_scanner(&self) -> Vec<Effect> {
        vec![Effect::PersistSession(crate::SessionUpdate::Scanner(
            crate::ScannerPersist {
                cidr: self.scanner.cidr.clone(),
            },
        ))]
    }

    fn begin_adapter_edit(&mut self) -> Vec<Effect> {
        let Some(adapter) = self.adapters.items.get(self.adapters.selected).cloned() else {
            return Vec::new();
        };
        if adapter.guid.is_empty() {
            return Vec::new();
        }

        let params = self
            .adapter_edit_persist
            .adapters
            .get(&adapter.guid)
            .cloned()
            .unwrap_or_else(|| adapter_defaults(&adapter));
        self.adapters.edit = Some(AdapterEditState {
            guid: adapter.guid,
            name: adapter.name,
            params,
            selected: AdapterField::Mode,
            cursor: 0,
            phase: AdapterEditPhase::Editing,
            validation_error: None,
            history: self.adapter_history.clone(),
            history_open: false,
            history_selected: 0,
            job: None,
        });
        Vec::new()
    }

    fn handle_adapter_edit_input(&mut self, input: InputEvent) -> Vec<Effect> {
        let phase = self.adapters.edit.as_ref().map(|edit| edit.phase.clone());
        match phase {
            Some(AdapterEditPhase::Applying) => return Vec::new(),
            Some(AdapterEditPhase::Succeeded(_)) => {
                self.adapters.edit = None;
                return self.refresh_adapters();
            }
            Some(AdapterEditPhase::Failed(_)) => {
                if let Some(edit) = self.adapters.edit.as_mut() {
                    edit.phase = AdapterEditPhase::Editing;
                    edit.validation_error = None;
                }
                return Vec::new();
            }
            _ => {}
        }

        let mapped_action = input.action();
        if self
            .adapters
            .edit
            .as_ref()
            .is_some_and(|edit| edit.history_open)
        {
            match mapped_action {
                Some(Action::Up) => {
                    let edit = self.adapters.edit.as_mut().expect("history is open");
                    edit.history_selected = edit.history_selected.saturating_sub(1);
                }
                Some(Action::Down) => {
                    let edit = self.adapters.edit.as_mut().expect("history is open");
                    edit.history_selected = (edit.history_selected + 1)
                        .min(edit.history.len().min(8).saturating_sub(1));
                }
                Some(Action::Confirm | Action::Toggle | Action::Right) => {
                    let index = self
                        .adapters
                        .edit
                        .as_ref()
                        .expect("history is open")
                        .history_selected;
                    return self.select_adapter_history(index);
                }
                Some(Action::SelectAdapterHistory(index)) => {
                    return self.select_adapter_history(index);
                }
                _ => {
                    self.adapters
                        .edit
                        .as_mut()
                        .expect("history is open")
                        .history_open = false;
                }
            }
            return Vec::new();
        }
        let action = match input.key() {
            Some(key) => {
                if self.handle_adapter_edit_key(key.code, key.modifiers.control) {
                    return self.persist_adapter_edit();
                }
                mapped_action
            }
            None => mapped_action,
        };

        match action {
            Some(Action::Back) => {
                if let Some(edit) = self.adapters.edit.as_mut()
                    && edit.phase == AdapterEditPhase::Confirming
                {
                    edit.phase = AdapterEditPhase::Editing;
                } else {
                    self.adapters.edit = None;
                }
                Vec::new()
            }
            Some(Action::Confirm | Action::Toggle) => self.confirm_adapter_edit(),
            Some(Action::Up) => {
                self.navigate_adapter_edit(-1);
                Vec::new()
            }
            Some(Action::Down) => {
                self.navigate_adapter_edit(1);
                Vec::new()
            }
            Some(Action::Left | Action::Right) => {
                if let Some(edit) = self.adapters.edit.as_mut()
                    && edit.selected == AdapterField::Mode
                {
                    edit.params.use_dhcp = !edit.params.use_dhcp;
                    return self.persist_adapter_edit();
                }
                Vec::new()
            }
            Some(Action::History) => {
                if let Some(edit) = self.adapters.edit.as_mut()
                    && edit.selected != AdapterField::Mode
                    && !edit.params.use_dhcp
                {
                    edit.history_open = !edit.history_open;
                    edit.history_selected = 0;
                }
                Vec::new()
            }
            Some(Action::SelectAdapterHistory(index)) => self.select_adapter_history(index),
            Some(Action::SelectAdapterField(field, cursor)) => {
                if let Some(edit) = self.adapters.edit.as_mut() {
                    edit.selected = field;
                    edit.cursor = cursor.min(edit.value(field).len());
                    edit.history_open = false;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn select_adapter_history(&mut self, index: usize) -> Vec<Effect> {
        let Some(edit) = self.adapters.edit.as_mut() else {
            return Vec::new();
        };
        let Some(value) = edit.history.get(index).cloned() else {
            edit.history_open = false;
            return Vec::new();
        };
        let field = edit.selected;
        edit.cursor = value.len();
        if let Some(target) = edit.value_mut(field) {
            *target = value;
        }
        edit.history_open = false;
        edit.validation_error = None;
        self.persist_adapter_edit()
    }

    /// Returns true when persistent form data changed.
    fn handle_adapter_edit_key(&mut self, code: KeyCode, control: bool) -> bool {
        let Some(edit) = self.adapters.edit.as_mut() else {
            return false;
        };
        if edit.phase != AdapterEditPhase::Editing || control {
            return false;
        }

        if edit.history_open {
            match code {
                KeyCode::Up => {
                    edit.history_selected = edit.history_selected.saturating_sub(1);
                    return false;
                }
                KeyCode::Down => {
                    edit.history_selected = (edit.history_selected + 1)
                        .min(edit.history.len().min(8).saturating_sub(1));
                    return false;
                }
                KeyCode::Enter | KeyCode::Right => {
                    if let Some(value) = edit.history.get(edit.history_selected).cloned() {
                        let field = edit.selected;
                        edit.cursor = value.len();
                        if let Some(target) = edit.value_mut(field) {
                            *target = value;
                        }
                        edit.history_open = false;
                        edit.validation_error = None;
                        return true;
                    }
                }
                KeyCode::Esc => {
                    edit.history_open = false;
                    return false;
                }
                _ => {}
            }
        }

        let field = edit.selected;
        if field == AdapterField::Mode {
            if matches!(code, KeyCode::Left | KeyCode::Right) {
                edit.params.use_dhcp = !edit.params.use_dhcp;
                return true;
            }
            return false;
        }
        if edit.params.use_dhcp {
            return false;
        }

        let len = edit.value(field).len();
        if code == KeyCode::Right
            && len > 0
            && edit.cursor == len
            && let Some(value) = edit
                .history
                .iter()
                .find(|candidate| candidate.starts_with(edit.value(field)) && candidate.len() > len)
                .cloned()
        {
            edit.cursor = value.len();
            *edit.value_mut(field).expect("text field") = value;
            edit.validation_error = None;
            return true;
        }
        match code {
            KeyCode::Left => edit.cursor = edit.cursor.saturating_sub(1),
            KeyCode::Right => edit.cursor = (edit.cursor + 1).min(len),
            KeyCode::Home => edit.cursor = 0,
            KeyCode::End => edit.cursor = len,
            KeyCode::Backspace if edit.cursor > 0 => {
                edit.cursor -= 1;
                let cursor = edit.cursor;
                edit.value_mut(field).expect("text field").remove(cursor);
                edit.validation_error = None;
                return true;
            }
            KeyCode::Delete if edit.cursor < len => {
                let cursor = edit.cursor;
                edit.value_mut(field).expect("text field").remove(cursor);
                edit.validation_error = None;
                return true;
            }
            KeyCode::Char(c) if (c.is_ascii_digit() || c == '.') && len < 15 => {
                let cursor = edit.cursor;
                edit.value_mut(field).expect("text field").insert(cursor, c);
                edit.cursor += 1;
                edit.validation_error = None;
                return true;
            }
            _ => {}
        }
        false
    }

    fn navigate_adapter_edit(&mut self, delta: isize) {
        let Some(edit) = self.adapters.edit.as_mut() else {
            return;
        };
        if edit.phase != AdapterEditPhase::Editing {
            return;
        }
        let index = wrap(edit.selected.index(), AdapterField::ALL.len(), delta);
        edit.selected = AdapterField::from_index(index);
        edit.cursor = edit.value(edit.selected).len();
        edit.history_open = false;
    }

    fn confirm_adapter_edit(&mut self) -> Vec<Effect> {
        let Some(edit) = self.adapters.edit.as_mut() else {
            return Vec::new();
        };
        if edit.phase == AdapterEditPhase::Confirming {
            let request = crate::AdapterConfigRequest {
                guid: edit.guid.clone(),
                name: edit.name.clone(),
                use_dhcp: edit.params.use_dhcp,
                ip: edit.params.ip.clone(),
                mask: edit.params.mask.clone(),
                gateway: non_empty(&edit.params.gateway),
                dns: [&edit.params.dns1, &edit.params.dns2]
                    .into_iter()
                    .filter_map(|value| non_empty(value))
                    .collect(),
            };
            let params = edit.params.clone();
            let guid = edit.guid.clone();
            if !params.use_dhcp {
                for value in [
                    &params.ip,
                    &params.mask,
                    &params.gateway,
                    &params.dns1,
                    &params.dns2,
                ] {
                    if !value.is_empty() {
                        self.adapter_history.retain(|old| old != value);
                        self.adapter_history.insert(0, value.clone());
                    }
                }
            }
            self.adapter_history.truncate(20);
            let history = self.adapter_history.clone();
            self.adapter_edit_persist
                .adapters
                .insert(guid.clone(), params.clone());
            let job = self.next_job(ToolKind::AdapterEdit);
            let edit = self.adapters.edit.as_mut().expect("edit remains open");
            edit.phase = AdapterEditPhase::Applying;
            edit.job = Some(job);
            edit.history = history.clone();
            return vec![
                Effect::PersistAdapterEdit {
                    guid,
                    params,
                    history,
                },
                Effect::ApplyAdapterConfig { job, request },
            ];
        }

        match validate_adapter_params(&edit.params) {
            Ok(()) => {
                edit.validation_error = None;
                edit.phase = AdapterEditPhase::Confirming;
            }
            Err(error) => edit.validation_error = Some(error),
        }
        Vec::new()
    }

    fn persist_adapter_edit(&mut self) -> Vec<Effect> {
        let Some(edit) = self.adapters.edit.as_ref() else {
            return Vec::new();
        };
        let guid = edit.guid.clone();
        let params = edit.params.clone();
        self.adapter_edit_persist
            .adapters
            .insert(guid.clone(), params.clone());
        vec![Effect::PersistAdapterEdit {
            guid,
            params,
            history: self.adapter_history.clone(),
        }]
    }

    fn handle_diagnostic_input(&mut self, input: InputEvent) -> Vec<Effect> {
        let key = input.key();
        let action = input.action();

        if key.is_some_and(|key| key.code == KeyCode::Tab && !key.modifiers.shift)
            || action == Some(Action::NextPage)
        {
            self.diagnostics.focus = self.diagnostics.focus.next();
            self.diagnostics.history_open = false;
            return Vec::new();
        }
        if key.is_some_and(|key| {
            key.code == KeyCode::BackTab || (key.code == KeyCode::Tab && key.modifiers.shift)
        }) || action == Some(Action::PreviousPage)
        {
            self.diagnostics.focus = self.diagnostics.focus.previous();
            self.diagnostics.history_open = false;
            return Vec::new();
        }
        if action == Some(Action::Back) {
            if self.diagnostics.history_open {
                self.diagnostics.history_open = false;
            } else {
                self.diagnostics.focused = false;
                self.diagnostics.focus = DiagnosticFocus::Menu;
            }
            return Vec::new();
        }

        match self.diagnostics.focus {
            DiagnosticFocus::Menu => match action {
                Some(Action::Up) => {
                    self.navigate_diagnostic_tool(-1);
                    return vec![self.persist_ui_effect()];
                }
                Some(Action::Down) => {
                    self.navigate_diagnostic_tool(1);
                    return vec![self.persist_ui_effect()];
                }
                _ => {}
            },
            DiagnosticFocus::Main => match action {
                Some(Action::Confirm | Action::Toggle) => return self.toggle_diagnostic(),
                Some(Action::Up) if self.diagnostics.tool == DiagnosticTool::Trace => {
                    let len = self.diagnostics.trace.hops.len();
                    if len > 0 {
                        self.diagnostics.trace.selected =
                            wrap(self.diagnostics.trace.selected, len, -1);
                    }
                }
                Some(Action::Down) if self.diagnostics.tool == DiagnosticTool::Trace => {
                    let len = self.diagnostics.trace.hops.len();
                    if len > 0 {
                        self.diagnostics.trace.selected =
                            wrap(self.diagnostics.trace.selected, len, 1);
                    }
                }
                Some(Action::Up) if self.diagnostics.tool == DiagnosticTool::PortScan => {
                    let len = self.diagnostics.port_scan.open_ports.len();
                    if len > 0 {
                        self.diagnostics.port_scan.selected =
                            wrap(self.diagnostics.port_scan.selected, len, -1);
                    }
                }
                Some(Action::Down) if self.diagnostics.tool == DiagnosticTool::PortScan => {
                    let len = self.diagnostics.port_scan.open_ports.len();
                    if len > 0 {
                        self.diagnostics.port_scan.selected =
                            wrap(self.diagnostics.port_scan.selected, len, 1);
                    }
                }
                _ => {}
            },
            DiagnosticFocus::Config => return self.handle_diagnostic_config(key, action),
        }
        Vec::new()
    }

    fn handle_diagnostic_config(
        &mut self,
        key: Option<crate::KeyEvent>,
        action: Option<Action>,
    ) -> Vec<Effect> {
        let running = self.diagnostics.active_common().job.is_some();
        let selected = self.active_diagnostic_config_index();
        let target_field = matches!(
            (self.diagnostics.tool, selected),
            (
                DiagnosticTool::Ping | DiagnosticTool::Trace | DiagnosticTool::PortScan,
                0
            ) | (DiagnosticTool::LinkQuality, 1)
                | (DiagnosticTool::LanSpeed, 4)
        );

        if action == Some(Action::History) && target_field && !running {
            self.diagnostics.history_open = !self.diagnostics.history_open;
            self.diagnostics.history_selected = 0;
            return Vec::new();
        }
        if self.diagnostics.history_open {
            match action {
                Some(Action::Up) => {
                    self.diagnostics.history_selected =
                        self.diagnostics.history_selected.saturating_sub(1);
                }
                Some(Action::Down) => {
                    self.diagnostics.history_selected = (self.diagnostics.history_selected + 1)
                        .min(
                            self.diagnostics
                                .target_history
                                .len()
                                .min(8)
                                .saturating_sub(1),
                        );
                }
                Some(Action::Confirm | Action::Toggle | Action::Right) => {
                    if let Some(value) = self
                        .diagnostics
                        .target_history
                        .get(self.diagnostics.history_selected)
                        .cloned()
                    {
                        self.set_active_diagnostic_field(value);
                        self.diagnostics.cursor = self.active_diagnostic_field().len();
                        self.diagnostics.history_open = false;
                        return self.persist_active_diagnostic();
                    }
                }
                _ => self.diagnostics.history_open = false,
            }
            return Vec::new();
        }

        if !running && target_field {
            if let Some(key) = key {
                let current = self.active_diagnostic_field().to_string();
                if key.code == KeyCode::Right
                    && !current.is_empty()
                    && self.diagnostics.cursor == current.len()
                    && let Some(value) = self
                        .diagnostics
                        .target_history
                        .iter()
                        .find(|candidate| {
                            candidate.starts_with(&current) && candidate.len() > current.len()
                        })
                        .cloned()
                {
                    self.set_active_diagnostic_field(value);
                    self.diagnostics.cursor = self.active_diagnostic_field().len();
                    return self.persist_active_diagnostic();
                }
                let mut value = current;
                if edit_ascii(&mut value, &mut self.diagnostics.cursor, key.code, |c| {
                    c.is_ascii() && !c.is_control() && c != ' '
                }) {
                    self.set_active_diagnostic_field(value);
                    return self.persist_active_diagnostic();
                }
            }
        } else if !running
            && matches!(
                (self.diagnostics.tool, selected),
                (DiagnosticTool::Trace, 1..)
                    | (DiagnosticTool::PortScan, 1..)
                    | (DiagnosticTool::LinkQuality, 2..)
                    | (DiagnosticTool::LanSpeed, 1 | 5..)
            )
            && let Some(key) = key
        {
            let mut value = self.active_diagnostic_field().to_string();
            if edit_ascii(&mut value, &mut self.diagnostics.cursor, key.code, |c| {
                c.is_ascii_digit()
            }) {
                self.set_active_diagnostic_field(value);
                self.sync_active_diagnostic_request();
                return self.persist_active_diagnostic();
            }
        }

        match action {
            Some(Action::Up) => self.move_diagnostic_config(-1),
            Some(Action::Down) => self.move_diagnostic_config(1),
            Some(Action::Left | Action::Right)
                if !running && self.diagnostics.tool == DiagnosticTool::Ping && selected > 0 =>
            {
                let dir = if action == Some(Action::Left) { -1 } else { 1 };
                let request = &mut self.diagnostics.ping.request;
                match selected {
                    1 => {
                        request.interval_ms =
                            ((request.interval_ms as i64) + dir * 100).clamp(100, 10_000) as u64
                    }
                    2 => {
                        request.timeout_ms =
                            ((request.timeout_ms as i64) + dir * 100).clamp(100, 10_000) as u64
                    }
                    3 => {
                        request.packet_size =
                            ((request.packet_size as i64) + dir * 8).clamp(0, 65_500) as u64
                    }
                    _ => {}
                }
                return self.persist_active_diagnostic();
            }
            Some(Action::Left | Action::Right)
                if !running
                    && self.diagnostics.tool == DiagnosticTool::LinkQuality
                    && selected == 0 =>
            {
                self.switch_link_quality_adapter(if action == Some(Action::Left) { -1 } else { 1 });
                return self.persist_active_diagnostic();
            }
            Some(Action::Left | Action::Right)
                if !running && self.diagnostics.tool == DiagnosticTool::LanSpeed =>
            {
                let forward = action == Some(Action::Right);
                let persist = &mut self.diagnostics.lan_speed.persist;
                match selected {
                    0 => {
                        persist.mode = if persist.mode == "client" {
                            "server"
                        } else {
                            "client"
                        }
                        .into();
                    }
                    2 => {
                        persist.proto = if persist.proto == "udp" { "tcp" } else { "udp" }.into();
                    }
                    3 => {
                        persist.direction = match (persist.direction.as_str(), forward) {
                            ("up", true) => "down",
                            ("down", true) => "bidir",
                            ("bidir", true) => "up",
                            ("up", false) => "bidir",
                            ("down", false) => "up",
                            _ => "down",
                        }
                        .into();
                    }
                    _ => return Vec::new(),
                }
                self.sync_lan_speed_request();
                let count = self.diagnostic_config_count();
                self.diagnostics.lan_speed.config_selected = selected.min(count.saturating_sub(1));
                return self.persist_active_diagnostic();
            }
            _ => {}
        }
        Vec::new()
    }

    fn navigate_diagnostic_tool(&mut self, delta: isize) {
        let current = DiagnosticTool::ALL
            .iter()
            .position(|tool| *tool == self.diagnostics.tool)
            .unwrap_or(0);
        let index = wrap(current, DiagnosticTool::ALL.len(), delta);
        self.diagnostics.tool = DiagnosticTool::from_index(index as u8);
        self.diagnostics.history_open = false;
        self.diagnostics.cursor = self.active_diagnostic_field().len();
    }

    fn diagnostic_config_count(&self) -> usize {
        match self.diagnostics.tool {
            DiagnosticTool::Ping => 4,
            DiagnosticTool::Trace => 3,
            DiagnosticTool::PortScan => 4,
            DiagnosticTool::LinkQuality => 6,
            DiagnosticTool::LanSpeed => self.lan_speed_config_count(),
            DiagnosticTool::PublicSpeed => 1,
        }
    }

    fn active_diagnostic_config_index(&self) -> usize {
        match self.diagnostics.tool {
            DiagnosticTool::Ping => self.diagnostics.ping.config_selected,
            DiagnosticTool::Trace => self.diagnostics.trace.config_selected,
            DiagnosticTool::PortScan => self.diagnostics.port_scan.config_selected,
            DiagnosticTool::LinkQuality => self.diagnostics.link_quality.config_selected,
            DiagnosticTool::LanSpeed => self.diagnostics.lan_speed.config_selected,
            DiagnosticTool::PublicSpeed => 0,
        }
    }

    fn set_diagnostic_config_index(&mut self, index: usize) {
        let index = index.min(self.diagnostic_config_count().saturating_sub(1));
        match self.diagnostics.tool {
            DiagnosticTool::Ping => self.diagnostics.ping.config_selected = index,
            DiagnosticTool::Trace => self.diagnostics.trace.config_selected = index,
            DiagnosticTool::PortScan => self.diagnostics.port_scan.config_selected = index,
            DiagnosticTool::LinkQuality => self.diagnostics.link_quality.config_selected = index,
            DiagnosticTool::LanSpeed => self.diagnostics.lan_speed.config_selected = index,
            DiagnosticTool::PublicSpeed => {}
        }
        self.diagnostics.cursor = self.active_diagnostic_field().len();
        self.diagnostics.history_open = false;
    }

    fn move_diagnostic_config(&mut self, delta: isize) {
        let index = wrap(
            self.active_diagnostic_config_index(),
            self.diagnostic_config_count(),
            delta,
        );
        self.set_diagnostic_config_index(index);
    }

    fn active_diagnostic_field(&self) -> &str {
        match self.diagnostics.tool {
            DiagnosticTool::Ping => match self.diagnostics.ping.config_selected {
                0 => &self.diagnostics.ping.request.target,
                _ => "",
            },
            DiagnosticTool::Trace => match self.diagnostics.trace.config_selected {
                0 => &self.diagnostics.trace.request.target,
                1 => &self.diagnostics.trace.max_hops_input,
                _ => &self.diagnostics.trace.timeout_input,
            },
            DiagnosticTool::PortScan => match self.diagnostics.port_scan.config_selected {
                0 => &self.diagnostics.port_scan.persist.target,
                1 => &self.diagnostics.port_scan.persist.start_port,
                2 => &self.diagnostics.port_scan.persist.end_port,
                _ => &self.diagnostics.port_scan.persist.timeout_ms,
            },
            DiagnosticTool::LinkQuality => match self.diagnostics.link_quality.config_selected {
                0 => "",
                1 => &self.diagnostics.link_quality.params.target,
                2 => &self.diagnostics.link_quality.params.count,
                3 => &self.diagnostics.link_quality.params.interval_ms,
                4 => &self.diagnostics.link_quality.params.timeout_ms,
                _ => &self.diagnostics.link_quality.params.packet_size,
            },
            DiagnosticTool::LanSpeed => match self.diagnostics.lan_speed.config_selected {
                1 => &self.diagnostics.lan_speed.persist.port,
                4 => &self.diagnostics.lan_speed.persist.peer,
                5 => &self.diagnostics.lan_speed.persist.duration,
                6 => &self.diagnostics.lan_speed.persist.streams,
                7 => &self.diagnostics.lan_speed.persist.payload,
                8 => &self.diagnostics.lan_speed.persist.rate,
                _ => "",
            },
            DiagnosticTool::PublicSpeed => "",
        }
    }

    fn set_active_diagnostic_field(&mut self, value: String) {
        match self.diagnostics.tool {
            DiagnosticTool::Ping if self.diagnostics.ping.config_selected == 0 => {
                self.diagnostics.ping.request.target = value;
            }
            DiagnosticTool::Ping => {}
            DiagnosticTool::Trace => match self.diagnostics.trace.config_selected {
                0 => self.diagnostics.trace.request.target = value,
                1 => self.diagnostics.trace.max_hops_input = value,
                _ => self.diagnostics.trace.timeout_input = value,
            },
            DiagnosticTool::PortScan => match self.diagnostics.port_scan.config_selected {
                0 => self.diagnostics.port_scan.persist.target = value,
                1 => self.diagnostics.port_scan.persist.start_port = value,
                2 => self.diagnostics.port_scan.persist.end_port = value,
                _ => self.diagnostics.port_scan.persist.timeout_ms = value,
            },
            DiagnosticTool::LinkQuality => match self.diagnostics.link_quality.config_selected {
                1 => self.diagnostics.link_quality.params.target = value,
                2 => self.diagnostics.link_quality.params.count = value,
                3 => self.diagnostics.link_quality.params.interval_ms = value,
                4 => self.diagnostics.link_quality.params.timeout_ms = value,
                5 => self.diagnostics.link_quality.params.packet_size = value,
                _ => {}
            },
            DiagnosticTool::LanSpeed => match self.diagnostics.lan_speed.config_selected {
                1 => self.diagnostics.lan_speed.persist.port = value,
                4 => self.diagnostics.lan_speed.persist.peer = value,
                5 => self.diagnostics.lan_speed.persist.duration = value,
                6 => self.diagnostics.lan_speed.persist.streams = value,
                7 => self.diagnostics.lan_speed.persist.payload = value,
                8 => self.diagnostics.lan_speed.persist.rate = value,
                _ => {}
            },
            DiagnosticTool::PublicSpeed => {}
        }
        self.sync_active_diagnostic_request();
    }

    fn sync_trace_request(&mut self) {
        self.diagnostics.trace.request.max_hops = self
            .diagnostics
            .trace
            .max_hops_input
            .parse::<u8>()
            .unwrap_or(30)
            .clamp(1, 64);
        self.diagnostics.trace.request.timeout_ms = self
            .diagnostics
            .trace
            .timeout_input
            .parse::<u64>()
            .unwrap_or(1_000)
            .clamp(100, 10_000);
    }

    fn sync_port_scan_request(&mut self) {
        let persist = &self.diagnostics.port_scan.persist;
        self.diagnostics.port_scan.request = crate::PortScanRequest {
            target: persist.target.trim().to_string(),
            start_port: persist.start_port.parse::<u16>().unwrap_or_default(),
            end_port: persist.end_port.parse::<u16>().unwrap_or_default(),
            timeout_ms: persist
                .timeout_ms
                .parse::<u64>()
                .unwrap_or(300)
                .clamp(20, 10_000),
            concurrency: self.scan_concurrency.clamp(1, 1_024),
        };
    }

    fn lan_speed_config_count(&self) -> usize {
        if self.diagnostics.lan_speed.persist.mode != "client" {
            2
        } else if self.diagnostics.lan_speed.persist.proto == "udp" {
            9
        } else {
            8
        }
    }

    fn sync_lan_speed_request(&mut self) {
        let persist = &self.diagnostics.lan_speed.persist;
        let protocol = if persist.proto == "udp" {
            crate::LanProtocol::Udp
        } else {
            crate::LanProtocol::Tcp
        };
        let payload_max = if protocol == crate::LanProtocol::Udp {
            65_507
        } else {
            1_048_576
        };
        self.diagnostics.lan_speed.request = crate::LanSpeedRequest {
            mode: if persist.mode == "client" {
                crate::LanSpeedMode::Client
            } else {
                crate::LanSpeedMode::Server
            },
            peer: persist.peer.trim().to_string(),
            port: persist.port.parse::<u16>().unwrap_or_default(),
            protocol,
            direction: match persist.direction.as_str() {
                "down" => crate::LanDirection::Download,
                "bidir" => crate::LanDirection::Bidirectional,
                _ => crate::LanDirection::Upload,
            },
            duration_secs: persist.duration.parse::<u64>().unwrap_or(10).clamp(1, 600),
            streams: persist.streams.parse::<u16>().unwrap_or(1).clamp(1, 32),
            payload_size: persist
                .payload
                .parse::<u32>()
                .unwrap_or(65_536)
                .clamp(64, payload_max),
            rate_mbps: persist.rate.parse::<u32>().unwrap_or_default().min(100_000),
        };
    }

    fn sync_link_quality_adapters(&mut self) {
        self.stash_link_quality_params();
        let selected_key = self
            .diagnostics
            .link_quality
            .persist
            .selected
            .clone()
            .or_else(|| {
                self.diagnostics
                    .link_quality
                    .request
                    .adapter
                    .as_ref()
                    .map(|adapter| adapter.key.clone())
            });
        let adapters = self
            .adapters
            .items
            .iter()
            .filter(|adapter| {
                let status = adapter.status.to_ascii_lowercase();
                adapter.is_physical
                    && !adapter.ipv4.is_empty()
                    && adapter.ipv4.parse::<std::net::Ipv4Addr>().is_ok()
                    && !status.contains("down")
                    && !status.contains("disconnected")
                    && !status.contains("standby")
            })
            .map(link_quality_adapter)
            .collect::<Vec<_>>();
        let selected = selected_key
            .as_ref()
            .and_then(|key| adapters.iter().position(|adapter| &adapter.key == key))
            .unwrap_or(0)
            .min(adapters.len().saturating_sub(1));
        self.diagnostics.link_quality.adapters = adapters;
        self.diagnostics.link_quality.selected_adapter = selected;
        self.load_selected_link_quality_params();
    }

    fn sync_scanner_cidr(&mut self, adapters: &[AdapterInfo]) {
        if !self.scanner.auto_cidr {
            return;
        }
        let usable = |adapter: &&AdapterInfo| {
            !adapter.ipv4.is_empty()
                && adapter.ipv4 != "—"
                && !adapter.ipv4.starts_with("127.")
                && adapter.cidr.is_some()
                && !adapter.status.to_ascii_lowercase().contains("down")
                && !adapter.status.to_ascii_lowercase().contains("disconnected")
        };
        let adapter = adapters
            .iter()
            .filter(usable)
            .find(|adapter| adapter.is_physical)
            .or_else(|| adapters.iter().find(usable));
        let Some(cidr) = adapter.and_then(active_network_cidr) else {
            return;
        };
        self.scanner.cidr = cidr;
        self.scanner.cursor = self.scanner.cidr.len();
    }

    fn switch_link_quality_adapter(&mut self, delta: isize) {
        self.stash_link_quality_params();
        let len = self.diagnostics.link_quality.adapters.len();
        if len > 0 {
            self.diagnostics.link_quality.selected_adapter =
                wrap(self.diagnostics.link_quality.selected_adapter, len, delta);
        }
        self.load_selected_link_quality_params();
    }

    fn stash_link_quality_params(&mut self) {
        let Some(adapter) = self
            .diagnostics
            .link_quality
            .request
            .adapter
            .as_ref()
            .or_else(|| {
                self.diagnostics
                    .link_quality
                    .adapters
                    .get(self.diagnostics.link_quality.selected_adapter)
            })
            .cloned()
        else {
            return;
        };
        self.diagnostics.link_quality.persist.selected = Some(adapter.key.clone());
        self.diagnostics
            .link_quality
            .persist
            .adapters
            .insert(adapter.key, self.diagnostics.link_quality.params.clone());
    }

    fn load_selected_link_quality_params(&mut self) {
        let adapter = self
            .diagnostics
            .link_quality
            .adapters
            .get(self.diagnostics.link_quality.selected_adapter)
            .cloned();
        if let Some(adapter) = &adapter {
            self.diagnostics.link_quality.params = self
                .diagnostics
                .link_quality
                .persist
                .adapters
                .get(&adapter.key)
                .cloned()
                .unwrap_or_default();
            self.diagnostics.link_quality.persist.selected = Some(adapter.key.clone());
        }
        self.diagnostics.link_quality.request.adapter = adapter;
        self.sync_link_quality_request();
    }

    fn sync_link_quality_request(&mut self) {
        let params = &self.diagnostics.link_quality.params;
        self.diagnostics.link_quality.request.target = params.target.clone();
        self.diagnostics.link_quality.request.count =
            params.count.parse::<u32>().unwrap_or(20).clamp(5, 100);
        self.diagnostics.link_quality.request.interval_ms = params
            .interval_ms
            .parse::<u64>()
            .unwrap_or(200)
            .clamp(50, 5_000);
        self.diagnostics.link_quality.request.timeout_ms = params
            .timeout_ms
            .parse::<u64>()
            .unwrap_or(1_000)
            .clamp(100, 10_000);
        self.diagnostics.link_quality.request.packet_size = params
            .packet_size
            .parse::<u64>()
            .unwrap_or(32)
            .clamp(0, 1_472);
    }

    fn link_quality_persist(&self) -> crate::LinkQualityPersist {
        let mut persist = self.diagnostics.link_quality.persist.clone();
        if let Some(adapter) = self
            .diagnostics
            .link_quality
            .adapters
            .get(self.diagnostics.link_quality.selected_adapter)
        {
            persist.selected = Some(adapter.key.clone());
            persist.adapters.insert(
                adapter.key.clone(),
                self.diagnostics.link_quality.params.clone(),
            );
        }
        persist
    }

    fn sync_active_diagnostic_request(&mut self) {
        match self.diagnostics.tool {
            DiagnosticTool::Trace => self.sync_trace_request(),
            DiagnosticTool::PortScan => self.sync_port_scan_request(),
            DiagnosticTool::LinkQuality => self.sync_link_quality_request(),
            DiagnosticTool::LanSpeed => self.sync_lan_speed_request(),
            _ => {}
        }
    }

    fn persist_active_diagnostic(&self) -> Vec<Effect> {
        match self.diagnostics.tool {
            DiagnosticTool::Ping => vec![Effect::PersistSession(crate::SessionUpdate::Ping(
                crate::PingPersist {
                    target: self.diagnostics.ping.request.target.clone(),
                    interval_ms: self.diagnostics.ping.request.interval_ms,
                    timeout_ms: self.diagnostics.ping.request.timeout_ms,
                    packet_size: self.diagnostics.ping.request.packet_size,
                },
            ))],
            DiagnosticTool::Trace => vec![Effect::PersistSession(crate::SessionUpdate::Trace(
                crate::TracePersist {
                    target: self.diagnostics.trace.request.target.clone(),
                    max_hops: self.diagnostics.trace.max_hops_input.clone(),
                    timeout_ms: self.diagnostics.trace.timeout_input.clone(),
                },
            ))],
            DiagnosticTool::PortScan => vec![Effect::PersistSession(
                crate::SessionUpdate::PortScan(self.diagnostics.port_scan.persist.clone()),
            )],
            DiagnosticTool::LinkQuality => vec![Effect::PersistSession(
                crate::SessionUpdate::LinkQuality(self.link_quality_persist()),
            )],
            DiagnosticTool::LanSpeed => vec![Effect::PersistSession(
                crate::SessionUpdate::LanSpeed(self.diagnostics.lan_speed.persist.clone()),
            )],
            _ => Vec::new(),
        }
    }

    fn handle_action(&mut self, action: Action) -> Vec<Effect> {
        use Action::*;
        match action {
            Quit => self.running = false,
            ToggleLanguage => {
                self.language = self.language.toggle();
                return vec![Effect::PersistPreferences(self.preferences())];
            }
            NextPage => {
                self.page = self.page.next();
                return vec![self.persist_ui_effect()];
            }
            PreviousPage => {
                self.page = self.page.previous();
                return vec![self.persist_ui_effect()];
            }
            SelectPage(index) => {
                self.page = Page::from_index(index);
                return vec![self.persist_ui_effect()];
            }
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
            Edit if self.page == Page::Scanner => {
                self.scanner.editing = true;
                self.scanner.cursor = self.scanner.cidr.len();
                self.scanner.history_open = false;
            }
            Confirm | Toggle if self.page == Page::Scanner => return self.toggle_scan(),
            Confirm if self.page == Page::Diagnostics => {
                self.diagnostics.focused = true;
                self.diagnostics.focus = DiagnosticFocus::Menu;
            }
            SelectDiagnostic(index) => {
                self.diagnostics.tool = DiagnosticTool::from_index(index);
                self.diagnostics.focused = true;
                self.diagnostics.focus = DiagnosticFocus::Menu;
                self.diagnostics.history_open = false;
                return vec![self.persist_ui_effect()];
            }
            SelectAdapter(index) if !self.adapters.items.is_empty() => {
                self.adapters.selected = index.min(self.adapters.items.len() - 1)
            }
            SelectSetting(index) if self.page == Page::Settings => {
                self.settings_selected = index.min(3);
                self.settings_just_reset = false;
            }
            Edit | Confirm | Toggle if self.page == Page::Adapters => {
                return self.begin_adapter_edit();
            }
            Up => {
                self.navigate(-1);
                if self.page == Page::Diagnostics {
                    return vec![self.persist_ui_effect()];
                }
            }
            Down => {
                self.navigate(1);
                if self.page == Page::Diagnostics {
                    return vec![self.persist_ui_effect()];
                }
            }
            Left if self.page == Page::Settings => {
                return self.change_setting(-1, false);
            }
            Right if self.page == Page::Settings => {
                return self.change_setting(1, false);
            }
            Confirm | Toggle if self.page == Page::Settings => {
                return self.change_setting(1, true);
            }
            Left
            | Right
            | Edit
            | Confirm
            | Toggle
            | History
            | SelectAdapter(_)
            | SelectAdapterField(_, _)
            | SelectAdapterHistory(_)
            | SelectScannerInput(_)
            | ActivateScannerPanel
            | SelectScannerHistory(_)
            | SelectSetting(_)
            | FocusDiagnostic(_)
            | SelectDiagnosticField(_, _)
            | SelectDiagnosticHistory(_) => {}
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
                let current = DiagnosticTool::ALL
                    .iter()
                    .position(|tool| *tool == self.diagnostics.tool)
                    .unwrap_or(0);
                let index = wrap(current, DiagnosticTool::ALL.len(), delta);
                self.diagnostics.tool = DiagnosticTool::from_index(index as u8);
            }
            Page::Settings => {
                self.settings_selected = wrap(self.settings_selected, 4, delta);
                self.settings_just_reset = false;
            }
            _ => {}
        }
    }

    fn change_setting(&mut self, direction: isize, activate: bool) -> Vec<Effect> {
        self.settings_just_reset = false;
        match self.settings_selected {
            0 => {
                self.language = self.language.toggle();
                vec![Effect::PersistPreferences(self.preferences())]
            }
            1 => {
                if direction < 0 {
                    self.scan_concurrency = self.scan_concurrency.saturating_sub(10).max(10);
                } else {
                    self.scan_concurrency = (self.scan_concurrency + 10).min(500);
                }
                vec![Effect::PersistPreferences(self.preferences())]
            }
            2 => {
                self.theme = if direction < 0 {
                    self.theme.previous()
                } else {
                    self.theme.next()
                };
                vec![Effect::PersistPreferences(self.preferences())]
            }
            3 if activate => {
                self.reset_session_memory();
                self.settings_just_reset = true;
                vec![Effect::PersistSession(crate::SessionUpdate::Reset(
                    crate::UiPersist {
                        last_tab: self.page as u8,
                        last_diag_tool: DiagnosticTool::ALL
                            .iter()
                            .position(|tool| *tool == self.diagnostics.tool)
                            .unwrap_or(0) as u8,
                    },
                ))]
            }
            _ => Vec::new(),
        }
    }

    fn reset_session_memory(&mut self) {
        self.scanner = ScannerState::default();
        self.diagnostics.ping.request = crate::PingRequest::default();
        let trace = crate::TracePersist::default();
        self.diagnostics.trace.request = crate::TraceRequest::default();
        self.diagnostics.trace.max_hops_input = trace.max_hops;
        self.diagnostics.trace.timeout_input = trace.timeout_ms;
        self.diagnostics.port_scan.persist = crate::PortScanPersist::default();
        self.sync_port_scan_request();
        self.diagnostics.lan_speed.persist = crate::LanSpeedPersist::default();
        self.sync_lan_speed_request();
        self.diagnostics.link_quality.persist = crate::LinkQualityPersist::default();
        self.diagnostics.link_quality.params = crate::LinkParams::default();
        self.sync_link_quality_request();
        self.diagnostics.target_history.clear();
        self.diagnostics.history_open = false;
        self.adapter_edit_persist = crate::AdapterEditPersist::default();
        self.adapter_history.clear();
    }

    fn persist_ui_effect(&self) -> Effect {
        let diagnostic_index = DiagnosticTool::ALL
            .iter()
            .position(|tool| *tool == self.diagnostics.tool)
            .unwrap_or(0) as u8;
        Effect::PersistSession(crate::SessionUpdate::Ui(crate::UiPersist {
            last_tab: self.page as u8,
            last_diag_tool: diagnostic_index,
        }))
    }

    fn next_job(&mut self, tool: ToolKind) -> JobId {
        self.generation = self.generation.saturating_add(1);
        JobId {
            tool,
            generation: self.generation,
        }
    }

    pub fn bootstrap_effects(&mut self) -> Vec<Effect> {
        let mut effects = self.refresh_dashboard();
        effects.extend(self.refresh_adapters());
        effects.extend(self.refresh_traffic());
        effects
    }

    pub fn refresh_adapters(&mut self) -> Vec<Effect> {
        self.refresh_adapters_inner()
    }

    pub fn refresh_traffic(&mut self) -> Vec<Effect> {
        self.refresh_traffic_inner()
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

    fn refresh_adapters_inner(&mut self) -> Vec<Effect> {
        let job = self.next_job(ToolKind::Adapters);
        self.adapters.job = Some(job);
        self.adapters.status = TaskStatus::Running;
        self.adapters.error = None;
        vec![Effect::RefreshAdapters { job }]
    }

    fn refresh_traffic_inner(&mut self) -> Vec<Effect> {
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
        let cidr = self.scanner.cidr.trim().to_string();
        if !cidr.is_empty() {
            self.scanner.history.retain(|old| old != &cidr);
            self.scanner.history.insert(0, cidr.clone());
            self.scanner.history.truncate(15);
        }
        self.scanner.editing = false;
        self.scanner.history_open = false;
        self.scanner.job = Some(job);
        self.scanner.status = TaskStatus::Running;
        self.scanner.current = 0;
        self.scanner.total = 0;
        self.scanner.results.clear();
        vec![
            Effect::PersistSession(crate::SessionUpdate::Scanner(crate::ScannerPersist {
                cidr: self.scanner.cidr.clone(),
            })),
            Effect::PersistSession(crate::SessionUpdate::CidrHistory(
                self.scanner.history.clone(),
            )),
            Effect::StartScan {
                job,
                request: ScanRequest {
                    cidr: self.scanner.cidr.clone(),
                    concurrency: self.scan_concurrency,
                },
            },
        ]
    }

    fn toggle_diagnostic(&mut self) -> Vec<Effect> {
        if let Some(job) = self.diagnostics.active_common().job {
            let common = self.diagnostics.active_common_mut();
            common.job = None;
            common.status = TaskStatus::Done;
            return vec![stop_effect(job)];
        }

        self.sync_active_diagnostic_request();
        let target = match self.diagnostics.tool {
            DiagnosticTool::Ping => Some(self.diagnostics.ping.request.target.trim().to_string()),
            DiagnosticTool::Trace => Some(self.diagnostics.trace.request.target.trim().to_string()),
            DiagnosticTool::PortScan => {
                Some(self.diagnostics.port_scan.request.target.trim().to_string())
            }
            DiagnosticTool::LinkQuality => Some(
                self.diagnostics
                    .link_quality
                    .request
                    .target
                    .trim()
                    .to_string(),
            ),
            DiagnosticTool::LanSpeed
                if self.diagnostics.lan_speed.request.mode == crate::LanSpeedMode::Client =>
            {
                Some(self.diagnostics.lan_speed.request.peer.trim().to_string())
            }
            _ => None,
        };
        if target.as_ref().is_some_and(String::is_empty) {
            let common = self.diagnostics.active_common_mut();
            let error = crate::RuntimeError::new(
                crate::RuntimeErrorCode::InvalidRequest,
                "target cannot be empty",
            );
            common.status = TaskStatus::Failed(error.message.clone());
            common.detail = error.message.clone();
            common.error = Some(error);
            return Vec::new();
        }
        if self.diagnostics.tool == DiagnosticTool::LinkQuality
            && self.diagnostics.link_quality.request.adapter.is_none()
        {
            let common = self.diagnostics.active_common_mut();
            let error = crate::RuntimeError::new(
                crate::RuntimeErrorCode::InvalidRequest,
                "no active physical IPv4 adapter is available",
            );
            common.status = TaskStatus::Failed(error.message.clone());
            common.detail = error.message.clone();
            common.error = Some(error);
            return Vec::new();
        }
        if self.diagnostics.tool == DiagnosticTool::PortScan {
            let request = &self.diagnostics.port_scan.request;
            if request.start_port == 0
                || request.end_port == 0
                || request.start_port > request.end_port
            {
                let common = self.diagnostics.active_common_mut();
                let error = crate::RuntimeError::new(
                    crate::RuntimeErrorCode::InvalidRequest,
                    "port range must be between 1 and 65535 and start must not exceed end",
                );
                common.status = TaskStatus::Failed(error.message.clone());
                common.detail = error.message.clone();
                common.error = Some(error);
                return Vec::new();
            }
        }
        if self.diagnostics.tool == DiagnosticTool::LanSpeed
            && self.diagnostics.lan_speed.request.port == 0
        {
            let common = self.diagnostics.active_common_mut();
            let error = crate::RuntimeError::new(
                crate::RuntimeErrorCode::InvalidRequest,
                "LAN speed port must be between 1 and 65535",
            );
            common.status = TaskStatus::Failed(error.message.clone());
            common.detail = error.message.clone();
            common.error = Some(error);
            return Vec::new();
        }

        let tool = ToolKind::from(self.diagnostics.tool);
        let job = self.next_job(tool);
        let common = self.diagnostics.active_common_mut();
        common.job = Some(job);
        common.status = TaskStatus::Running;
        common.error = None;
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
                self.diagnostics.link_quality.snapshot = None;
                Effect::StartLinkQuality {
                    job,
                    request: self.diagnostics.link_quality.request.clone(),
                }
            }
            DiagnosticTool::LanSpeed => {
                self.diagnostics.lan_speed.samples.clear();
                self.diagnostics.lan_speed.summary = None;
                self.diagnostics.lan_speed.endpoint.clear();
                self.diagnostics.lan_speed.phase = None;
                Effect::StartLanSpeed {
                    job,
                    request: self.diagnostics.lan_speed.request.clone(),
                }
            }
        };
        let mut effects = vec![effect];
        if let Some(target) = target {
            self.diagnostics
                .target_history
                .retain(|value| value != &target);
            self.diagnostics.target_history.insert(0, target);
            self.diagnostics.target_history.truncate(15);
            effects.push(Effect::PersistSession(crate::SessionUpdate::TargetHistory(
                self.diagnostics.target_history.clone(),
            )));
        }
        effects
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
            RuntimeEvent::AdaptersUpdated(adapters) => {
                self.sync_scanner_cidr(&adapters);
                self.adapters.items = adapters;
                self.sync_link_quality_adapters();
            }
            RuntimeEvent::TrafficUpdated(rows) => {
                self.sync_dashboard_traffic(&rows);
                self.traffic.rows = rows;
            }
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
                self.sync_scanner_cidr(&adapters);
                self.adapters.items = adapters;
                self.sync_link_quality_adapters();
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
                self.sync_dashboard_traffic(&rows);
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
            RuntimeEvent::AdapterConfigStarted { job }
                if self.adapters.edit.as_ref().and_then(|edit| edit.job) == Some(job) =>
            {
                if let Some(edit) = self.adapters.edit.as_mut() {
                    edit.phase = AdapterEditPhase::Applying;
                }
            }
            RuntimeEvent::AdapterConfigFinished { job, outcome }
                if self.adapters.edit.as_ref().and_then(|edit| edit.job) == Some(job) =>
            {
                if let Some(edit) = self.adapters.edit.as_mut() {
                    edit.phase = AdapterEditPhase::Succeeded(outcome);
                    edit.job = None;
                }
            }
            RuntimeEvent::AdapterConfigFailed { job, error }
                if self.adapters.edit.as_ref().and_then(|edit| edit.job) == Some(job) =>
            {
                if let Some(edit) = self.adapters.edit.as_mut() {
                    edit.phase = AdapterEditPhase::Failed(error);
                    edit.job = None;
                }
            }
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
                let selected_ip = self
                    .scanner
                    .results
                    .get(self.scanner.selected)
                    .map(|host| host.ip.clone());
                self.scanner.results.push(host);
                self.scanner.results.sort_by(scan_host_ip_order);
                if let Some(selected_ip) = selected_ip {
                    self.scanner.selected = self
                        .scanner
                        .results
                        .iter()
                        .position(|host| host.ip == selected_ip)
                        .unwrap_or(0);
                }
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
                    || {
                        format!(
                            "seq={} {}",
                            sample.sequence,
                            if self.language == Language::Zh {
                                "请求超时"
                            } else {
                                "request timed out"
                            }
                        )
                    },
                    |latency| {
                        format!(
                            "{} seq={} bytes={} ttl={} time={}ms",
                            if self.language == Language::Zh {
                                "回复"
                            } else {
                                "Reply"
                            },
                            sample.sequence,
                            sample.size,
                            sample.ttl.map_or_else(|| "—".into(), |ttl| ttl.to_string()),
                            latency
                        )
                    },
                );
                let common = &mut self.diagnostics.ping.common;
                common.progress = (sample.sequence.saturating_add(1) * 12).min(99) as u8;
                common.primary = primary.clone();
                common.detail = format!(
                    "{} / {} received · {:.1}% loss · avg {:?} ms",
                    sample.received, sample.sent, sample.loss_percent, sample.average_ms
                );
                common.log.push(primary);
                self.diagnostics.ping.summary = Some(crate::PingSummary {
                    sent: sample.sent,
                    received: sample.received,
                    min_ms: sample.min_ms,
                    average_ms: sample.average_ms,
                    max_ms: sample.max_ms,
                    loss_percent: sample.loss_percent,
                });
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
            RuntimeEvent::PortScanOpen { job, result }
                if self.diagnostics.port_scan.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.port_scan;
                if let Err(index) = state
                    .open_ports
                    .binary_search_by_key(&result.port, |entry| entry.port)
                {
                    state.open_ports.insert(index, result.clone());
                }
                let line = format!("open: {} ({})", result.port, result.service);
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
                state.common.primary = format!("{} B/s", sample.bytes_per_second);
                state.common.detail = format!("{} bytes", sample.bytes);
                state.samples.push(sample);
            }
            RuntimeEvent::PublicSpeedFinished { job, summary }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                finish_common(
                    &mut self.diagnostics.public_speed.common,
                    format!(
                        "average {} B/s · peak {} B/s",
                        summary.average_bytes_per_second, summary.peak_bytes_per_second
                    ),
                );
                self.diagnostics.public_speed.summary = Some(summary);
            }
            RuntimeEvent::PublicSpeedFailed { job, error }
                if self.diagnostics.public_speed.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.public_speed.common, error);
            }
            RuntimeEvent::LinkQualityStarted { job, snapshot }
                if self.diagnostics.link_quality.common.job == Some(job) =>
            {
                self.diagnostics.link_quality.common.status = TaskStatus::Running;
                self.diagnostics.link_quality.snapshot = Some(*snapshot);
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
                if let Some(snapshot) = &state.snapshot {
                    state.summary =
                        Some(crate::link_quality::summary_from_sample(snapshot, &sample));
                }
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
            RuntimeEvent::LanSpeedStarted { job, endpoint }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                let state = &mut self.diagnostics.lan_speed;
                state.common.status = TaskStatus::Running;
                state.endpoint = endpoint;
            }
            RuntimeEvent::LanSpeedStatus { job, phase }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                self.diagnostics.lan_speed.phase = Some(phase);
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
                self.diagnostics.lan_speed.phase = None;
            }
            RuntimeEvent::LanSpeedFailed { job, error }
                if self.diagnostics.lan_speed.common.job == Some(job) =>
            {
                fail_common(&mut self.diagnostics.lan_speed.common, error);
            }
            _ => {}
        }
    }

    fn sync_dashboard_traffic(&mut self, rows: &[TrafficRow]) {
        let Some(interface) = self.dashboard.snapshot.active_interface.as_ref() else {
            return;
        };
        let Some(row) = rows.iter().find(|row| row.name == interface.name) else {
            return;
        };
        self.dashboard.snapshot.download_bps = row.download_bps;
        self.dashboard.snapshot.upload_bps = row.upload_bps;
        self.dashboard.snapshot.total_download = row.total_download;
        self.dashboard.snapshot.total_upload = row.total_upload;
    }
}

fn scan_host_ip_order(left: &ScanHost, right: &ScanHost) -> Ordering {
    match (left.ip.parse::<IpAddr>(), right.ip.parse::<IpAddr>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.ip.cmp(&right.ip),
    }
}

fn active_network_cidr(adapter: &AdapterInfo) -> Option<String> {
    let (_, prefix) = adapter.cidr.as_deref()?.rsplit_once('/')?;
    let prefix = prefix.parse::<u8>().ok().filter(|prefix| *prefix <= 32)?;
    let address = adapter.ipv4.parse::<std::net::Ipv4Addr>().ok()?;
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    let network = std::net::Ipv4Addr::from(u32::from(address) & mask);
    Some(format!("{network}/{prefix}"))
}

fn finish_common(common: &mut DiagnosticCommonState, summary: String) {
    common.status = TaskStatus::Done;
    common.error = None;
    common.progress = 100;
    common.detail = summary;
    common.job = None;
}

fn fail_common(common: &mut DiagnosticCommonState, error: crate::RuntimeError) {
    common.status = TaskStatus::Failed(error.message.clone());
    common.detail = error.message.clone();
    common.error = Some(error);
    common.job = None;
}

fn link_quality_adapter(adapter: &AdapterInfo) -> crate::LinkQualityAdapter {
    let key = if !adapter.guid.is_empty() {
        adapter.guid.clone()
    } else if !adapter.mac.is_empty() {
        adapter.mac.clone()
    } else {
        adapter.name.clone()
    };
    crate::LinkQualityAdapter {
        key,
        name: adapter.name.clone(),
        guid: adapter.guid.clone(),
        ipv4: adapter.ipv4.clone(),
        is_wifi: adapter.kind.to_ascii_lowercase().contains("ieee80211") || adapter.ssid.is_some(),
        link_speed_bps: adapter.link_speed_bps,
        mac: adapter.mac.clone(),
    }
}

fn wrap(current: usize, len: usize, delta: isize) -> usize {
    (current as isize + delta).rem_euclid(len as isize) as usize
}

fn stop_effect(job: JobId) -> Effect {
    match job.tool {
        ToolKind::Dashboard => unreachable!("dashboard refreshes are not diagnostic jobs"),
        ToolKind::Adapters | ToolKind::AdapterEdit | ToolKind::Traffic => {
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

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn edit_ascii(
    value: &mut String,
    cursor: &mut usize,
    code: KeyCode,
    allow: impl Fn(char) -> bool,
) -> bool {
    *cursor = (*cursor).min(value.len());
    match code {
        KeyCode::Left => *cursor = cursor.saturating_sub(1),
        KeyCode::Right => *cursor = (*cursor + 1).min(value.len()),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = value.len(),
        KeyCode::Backspace if *cursor > 0 => {
            *cursor -= 1;
            value.remove(*cursor);
            return true;
        }
        KeyCode::Delete if *cursor < value.len() => {
            value.remove(*cursor);
            return true;
        }
        KeyCode::Char(character) if allow(character) && value.len() < 64 => {
            value.insert(*cursor, character);
            *cursor += 1;
            return true;
        }
        _ => {}
    }
    false
}

fn adapter_defaults(adapter: &AdapterInfo) -> AdapterEditParams {
    let prefix = adapter
        .cidr
        .as_deref()
        .and_then(|cidr| cidr.rsplit_once('/'))
        .and_then(|(_, prefix)| prefix.parse::<u8>().ok());
    let mask = prefix.and_then(prefix_to_mask).unwrap_or_default();
    let gateway = adapter
        .ipv4
        .parse::<std::net::Ipv4Addr>()
        .ok()
        .map(|ip| {
            let mut octets = ip.octets();
            octets[3] = 1;
            std::net::Ipv4Addr::from(octets).to_string()
        })
        .unwrap_or_default();
    AdapterEditParams {
        use_dhcp: adapter.dhcp_enabled,
        ip: adapter.ipv4.clone(),
        mask,
        gateway,
        dns1: "8.8.8.8".into(),
        dns2: "8.8.4.4".into(),
    }
}

fn prefix_to_mask(prefix: u8) -> Option<String> {
    if prefix > 32 {
        return None;
    }
    let value = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    Some(std::net::Ipv4Addr::from(value).to_string())
}

fn validate_adapter_params(params: &AdapterEditParams) -> Result<(), AdapterValidationError> {
    crate::AdapterConfigRequest {
        guid: "validation-only".into(),
        name: String::new(),
        use_dhcp: params.use_dhcp,
        ip: params.ip.clone(),
        mask: params.mask.clone(),
        gateway: non_empty(&params.gateway),
        dns: [&params.dns1, &params.dns2]
            .into_iter()
            .filter_map(|value| non_empty(value))
            .collect(),
    }
    .validate()
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
        let Some(Effect::StartScan { job, .. }) = effects
            .iter()
            .find(|effect| matches!(effect, Effect::StartScan { .. }))
            .cloned()
        else {
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
                ip: "192.168.1.10".into(),
                ..ScanHost::default()
            },
        }));
        assert_eq!(app.scanner.results.len(), 1);
        app.update(Runtime(RuntimeEvent::ScanHostFound {
            job,
            host: ScanHost {
                ip: "192.168.1.2".into(),
                ..ScanHost::default()
            },
        }));
        assert_eq!(
            app.scanner
                .results
                .iter()
                .map(|host| host.ip.as_str())
                .collect::<Vec<_>>(),
            ["192.168.1.2", "192.168.1.10"]
        );
        assert_eq!(app.scanner.selected, 1);
    }

    #[test]
    fn scanner_restores_edit_mru_completion_and_panel_click_semantics() {
        let mut config = crate::ConfigData::default();
        config.session.scanner.cidr = "10.0.0.0/24".into();
        config.session.history.cidrs = vec!["192.168.50.0/24".into(), "10.0.0.0/24".into()];
        let mut app = AppModel::default();
        app.apply_config(&config);
        app.page = Page::Scanner;

        app.update(Input(InputEvent::Action(Action::SelectScannerInput(3))));
        assert!(app.scanner.editing);
        assert_eq!(app.scanner.cursor, 3);
        app.update(Input(InputEvent::Action(Action::History)));
        assert!(app.scanner.history_open);
        let effects = app.update(Input(InputEvent::Action(Action::SelectScannerHistory(0))));
        assert_eq!(app.scanner.cidr, "192.168.50.0/24");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::Scanner(_))]
        ));

        app.scanner.cidr = "10.".into();
        app.scanner.cursor = app.scanner.cidr.len();
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Right))));
        assert_eq!(app.scanner.cidr, "10.0.0.0/24");

        let effects = app.update(Input(InputEvent::Action(Action::ActivateScannerPanel)));
        assert!(!app.scanner.editing);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::StartScan { .. }))
        );
        let effects = app.update(Input(InputEvent::Action(Action::ActivateScannerPanel)));
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::StartScan { .. }))
        );
        assert!(effects.iter().any(|effect| matches!(
            effect,
            Effect::PersistSession(crate::SessionUpdate::CidrHistory(_))
        )));
    }

    #[test]
    fn scanner_defaults_to_the_active_adapter_network_until_user_edits_it() {
        let mut config = crate::ConfigData::default();
        config.session.scanner.cidr = "10.20.30.0/24".into();
        let mut app = AppModel::default();
        app.apply_config(&config);
        app.update(Runtime(RuntimeEvent::AdaptersUpdated(vec![AdapterInfo {
            name: "WLAN".into(),
            ipv4: "192.168.50.35".into(),
            cidr: Some("192.168.50.35/24".into()),
            status: "up".into(),
            is_physical: true,
            ..AdapterInfo::default()
        }])));
        assert_eq!(app.scanner.cidr, "192.168.50.0/24");

        app.page = Page::Scanner;
        app.scanner.editing = true;
        app.scanner.cursor = app.scanner.cidr.len();
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Backspace))));
        assert!(!app.scanner.auto_cidr);
        let edited = app.scanner.cidr.clone();
        app.update(Runtime(RuntimeEvent::AdaptersUpdated(vec![AdapterInfo {
            ipv4: "172.16.8.9".into(),
            cidr: Some("172.16.8.9/16".into()),
            status: "up".into(),
            is_physical: true,
            ..AdapterInfo::default()
        }])));
        assert_eq!(app.scanner.cidr, edited);
    }

    #[test]
    fn typed_ping_events_ignore_stale_and_post_cancel_samples() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
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
            sent: 2,
            received: 2,
            min_ms: Some(12),
            average_ms: Some(12.0),
            max_ms: Some(12),
            loss_percent: 0.0,
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
            app.diagnostics.ping.common.log.last().map(String::as_str),
            Some("Reply seq=1 bytes=32 ttl=64 time=12ms")
        );

        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopPing(job)]
        );
        app.update(Runtime(RuntimeEvent::PingSample { job, sample }));
        assert_eq!(app.diagnostics.ping.samples.len(), 1);

        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPing { job: restarted, .. } = effects[0] else {
            panic!("expected restarted ping effect");
        };
        assert!(restarted.generation > job.generation);
        let summary = crate::PingSummary {
            sent: 4,
            received: 4,
            min_ms: Some(10),
            average_ms: Some(12.5),
            max_ms: Some(15),
            loss_percent: 0.0,
        };
        app.update(Runtime(RuntimeEvent::PingFinished {
            job,
            summary: summary.clone(),
        }));
        assert_eq!(app.diagnostics.ping.common.job, Some(restarted));
        app.update(Runtime(RuntimeEvent::PingFinished {
            job: restarted,
            summary: summary.clone(),
        }));
        assert_eq!(app.diagnostics.ping.summary, Some(summary));
        assert_eq!(app.diagnostics.ping.common.status, TaskStatus::Done);
    }

    #[test]
    fn trace_start_cancel_restart_ignores_stale_and_finishes_current_job() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
        app.diagnostics.tool = DiagnosticTool::Trace;

        let first = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartTrace { job: first, .. } = first[0] else {
            panic!("expected trace effect");
        };
        let hop = crate::TraceHop {
            ttl: 1,
            address: Some("192.0.2.1".into()),
            hostname: None,
            latency_ms: Some(12),
        };
        app.update(Runtime(RuntimeEvent::TraceHop {
            job: first,
            hop: hop.clone(),
        }));
        assert_eq!(app.diagnostics.trace.hops, [hop]);
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopTrace(first)]
        );

        let restarted = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartTrace { job: restarted, .. } = restarted[0] else {
            panic!("expected restarted trace effect");
        };
        app.update(Runtime(RuntimeEvent::TraceFinished {
            job: first,
            hops: 1,
        }));
        assert_eq!(app.diagnostics.trace.common.job, Some(restarted));
        app.update(Runtime(RuntimeEvent::TraceFinished {
            job: restarted,
            hops: 8,
        }));
        assert_eq!(app.diagnostics.trace.common.status, TaskStatus::Done);
        assert_eq!(app.diagnostics.trace.common.job, None);
    }

    #[test]
    fn typed_failure_only_mutates_its_current_tool_generation() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
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
        assert_eq!(
            app.diagnostics
                .trace
                .common
                .error
                .as_ref()
                .map(|error| error.code),
            Some(crate::RuntimeErrorCode::Timeout)
        );
        assert_eq!(app.diagnostics.ping.common.status, TaskStatus::Idle);
    }

    #[test]
    fn port_scan_effect_carries_validated_shape_not_generic_text() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
        app.diagnostics.tool = DiagnosticTool::PortScan;
        app.scan_concurrency = 64;
        app.diagnostics.port_scan.persist = crate::PortScanPersist {
            target: "192.0.2.10".into(),
            start_port: "20".into(),
            end_port: "443".into(),
            timeout_ms: "250".into(),
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
        assert!(
            app.update(Input(InputEvent::Action(Action::Down)))
                .is_empty()
        );
        assert_eq!(app.settings_selected, 1);
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Right))),
            [Effect::PersistPreferences(crate::Preferences {
                language: Language::En,
                theme: ThemeId::Classic,
                scan_concurrency: 60,
            })]
        );
        assert_eq!(app.scan_concurrency, 60);

        assert_eq!(
            app.update(Input(InputEvent::Action(Action::ToggleLanguage))),
            [Effect::PersistPreferences(crate::Preferences {
                language: Language::Zh,
                theme: ThemeId::Classic,
                scan_concurrency: 60,
            })]
        );

        app.diagnostics.ping.request.target = "remembered.example".into();
        app.diagnostics.target_history = vec!["remembered.example".into()];
        app.update(Input(InputEvent::Action(Action::Down)));
        assert_eq!(app.settings_selected, 2);
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Confirm))),
            [Effect::PersistPreferences(crate::Preferences {
                language: Language::Zh,
                theme: ThemeId::Nord,
                scan_concurrency: 60,
            })]
        );
        app.update(Input(InputEvent::Action(Action::Down)));
        assert_eq!(app.settings_selected, 3);
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Confirm))),
            [Effect::PersistSession(crate::SessionUpdate::Reset(
                crate::UiPersist {
                    last_tab: Page::Settings as u8,
                    last_diag_tool: 0,
                }
            ))]
        );
        assert_eq!(app.diagnostics.ping.request, crate::PingRequest::default());
        assert!(app.diagnostics.target_history.is_empty());
        assert!(app.settings_just_reset);
        assert_eq!(app.page, Page::Settings);
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
    fn clock_messages_refresh_the_dashboard_without_network_io() {
        let mut app = AppModel::default();
        let effects = app.update(Clock("2026-07-12 20:30:45".into()));
        assert!(effects.is_empty());
        assert_eq!(app.dashboard.snapshot.observed_at, "2026-07-12 20:30:45");
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
    fn shared_model_loads_preferences_from_legacy_config_data() {
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

    fn adapter_app() -> AppModel {
        let mut app = AppModel {
            page: Page::Adapters,
            ..AppModel::default()
        };
        app.adapters.items.push(AdapterInfo {
            name: "Ethernet".into(),
            guid: "adapter-guid".into(),
            ipv4: "192.168.50.20".into(),
            cidr: Some("192.168.50.20/24".into()),
            dhcp_enabled: false,
            status: "up".into(),
            ..AdapterInfo::default()
        });
        app
    }

    #[test]
    fn adapter_edit_preserves_defaults_and_validates_static_fields() {
        let mut app = adapter_app();
        assert!(
            app.update(Input(InputEvent::Action(Action::Edit)))
                .is_empty()
        );
        let edit = app.adapters.edit.as_ref().unwrap();
        assert_eq!(edit.params.ip, "192.168.50.20");
        assert_eq!(edit.params.mask, "255.255.255.0");
        assert_eq!(edit.params.gateway, "192.168.50.1");
        assert_eq!(edit.params.dns1, "8.8.8.8");
        assert_eq!(edit.selected, AdapterField::Mode);

        app.adapters.edit.as_mut().unwrap().params.mask = "255.0.255.0".into();
        app.update(Input(InputEvent::Action(Action::Confirm)));
        let edit = app.adapters.edit.as_ref().unwrap();
        assert_eq!(edit.phase, AdapterEditPhase::Editing);
        assert_eq!(edit.validation_error, Some(AdapterValidationError::Mask));

        app.adapters.edit.as_mut().unwrap().params.mask = "255.255.255.0".into();
        app.update(Input(InputEvent::Action(Action::Confirm)));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Confirming
        );
    }

    #[test]
    fn adapter_enter_and_space_keep_unambiguous_edit_entry() {
        for action in [Action::Confirm, Action::Toggle] {
            let mut app = adapter_app();
            app.update(Input(InputEvent::Action(action)));
            assert!(app.adapters.edit.is_some(), "{action:?}");
        }
    }

    #[test]
    fn adapter_apply_is_job_scoped_and_runtime_only_is_not_reported_as_failure() {
        let mut app = adapter_app();
        app.update(Input(InputEvent::Action(Action::Edit)));
        app.update(Input(InputEvent::Action(Action::Confirm)));
        let effects = app.update(Input(InputEvent::Action(Action::Confirm)));
        assert!(matches!(effects[0], Effect::PersistAdapterEdit { .. }));
        let Effect::ApplyAdapterConfig { job, ref request } = effects[1] else {
            panic!("expected typed adapter apply effect");
        };
        assert_eq!(request.guid, "adapter-guid");
        assert_eq!(request.gateway.as_deref(), Some("192.168.50.1"));

        app.update(Input(InputEvent::Action(Action::Back)));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Applying
        );
        app.update(Runtime(RuntimeEvent::AdapterConfigFinished {
            job: JobId {
                generation: job.generation + 1,
                ..job
            },
            outcome: crate::AdapterApplyOutcome::Persistent,
        }));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Applying
        );
        app.update(Runtime(RuntimeEvent::AdapterConfigFinished {
            job,
            outcome: crate::AdapterApplyOutcome::RuntimeOnly,
        }));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Succeeded(crate::AdapterApplyOutcome::RuntimeOnly)
        );
        let effects = app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Enter))));
        assert!(app.adapters.edit.is_none());
        assert!(matches!(
            effects.as_slice(),
            [Effect::RefreshAdapters { .. }]
        ));
    }

    #[test]
    fn adapter_failure_returns_to_form_and_cursor_edits_emit_persistence() {
        let mut app = adapter_app();
        app.update(Input(InputEvent::Action(Action::Edit)));
        app.update(Input(InputEvent::Action(Action::Confirm)));
        let effects = app.update(Input(InputEvent::Action(Action::Confirm)));
        let Effect::ApplyAdapterConfig { job, .. } = effects[1] else {
            panic!()
        };
        app.update(Runtime(RuntimeEvent::AdapterConfigFailed {
            job,
            error: crate::RuntimeError::new(
                crate::RuntimeErrorCode::PermissionDenied,
                "administrator required",
            ),
        }));
        assert!(matches!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Failed(_)
        ));
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Enter))));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().phase,
            AdapterEditPhase::Editing
        );

        app.update(Input(InputEvent::Action(Action::SelectAdapterField(
            AdapterField::Ipv4,
            0,
        ))));
        let effects = app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Char('1')))));
        assert_eq!(
            app.adapters.edit.as_ref().unwrap().params.ip,
            "1192.168.50.20"
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistAdapterEdit { .. }]
        ));
    }

    #[test]
    fn adapter_edit_loads_guid_scoped_persistence_and_history_escape_is_local() {
        let mut app = adapter_app();
        let params = AdapterEditParams {
            use_dhcp: false,
            ip: "10.0.0.8".into(),
            mask: "255.255.255.0".into(),
            gateway: "10.0.0.1".into(),
            dns1: "1.1.1.1".into(),
            dns2: String::new(),
        };
        let mut config = crate::ConfigData::default();
        config
            .session
            .adapter_edit
            .adapters
            .insert("adapter-guid".into(), params.clone());
        config.session.history.adapter = vec!["9.9.9.9".into()];
        app.apply_config(&config);
        app.page = Page::Adapters;
        app.update(Input(InputEvent::Action(Action::Edit)));
        assert_eq!(app.adapters.edit.as_ref().unwrap().params, params);
        app.update(Input(InputEvent::Action(Action::SelectAdapterField(
            AdapterField::Ipv4,
            0,
        ))));
        app.update(Input(InputEvent::Action(Action::History)));
        assert!(app.adapters.edit.as_ref().unwrap().history_open);
        let effects = app.update(Input(InputEvent::Action(Action::SelectAdapterHistory(0))));
        assert_eq!(app.adapters.edit.as_ref().unwrap().params.ip, "9.9.9.9");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistAdapterEdit { .. }]
        ));
        app.update(Input(InputEvent::Action(Action::History)));
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Esc))));
        assert!(!app.adapters.edit.as_ref().unwrap().history_open);
        assert!(app.adapters.edit.is_some());

        let edit = app.adapters.edit.as_mut().unwrap();
        edit.params.ip = "9.".into();
        edit.selected = AdapterField::Ipv4;
        edit.cursor = 2;
        let effects = app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Right))));
        assert_eq!(app.adapters.edit.as_ref().unwrap().params.ip, "9.9.9.9");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistAdapterEdit { .. }]
        ));
    }

    #[test]
    fn adapter_edit_keeps_global_shortcuts_available() {
        let mut app = adapter_app();
        app.update(Input(InputEvent::Action(Action::Edit)));
        assert!(app.adapters.edit.is_some());

        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Tab))));
        assert_eq!(app.page, Page::Scanner);
        app.page = Page::Adapters;
        let effects = app.update(Input(InputEvent::Action(Action::ToggleLanguage)));
        assert_eq!(app.language, Language::Zh);
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistPreferences(_)]
        ));
        app.update(Input(InputEvent::Action(Action::Quit)));
        assert!(!app.running);
    }

    #[test]
    fn diagnostics_focus_and_ping_config_keep_established_interaction() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Enter))));
        assert!(app.diagnostics.focused);
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Menu);
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Tab))));
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Main);
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::BackTab))));
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Menu);
        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Tab))));

        let effects = app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Char(' ')))));
        assert!(matches!(effects.first(), Some(Effect::StartPing { .. })));
        assert!(matches!(
            effects.get(1),
            Some(Effect::PersistSession(crate::SessionUpdate::TargetHistory(
                _
            )))
        ));
        let Effect::StartPing { job, .. } = effects[0] else {
            panic!()
        };
        assert_eq!(
            app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Char(' '))))),
            [Effect::StopPing(job)]
        );

        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Tab))));
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Config);
        app.diagnostics.cursor = 0;
        let effects = app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Char('x')))));
        assert_eq!(app.diagnostics.ping.request.target, "x8.8.8.8");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::Ping(_))]
        ));
        app.update(Input(InputEvent::Action(Action::SelectDiagnosticField(
            1, 0,
        ))));
        let effects = app.update(Input(InputEvent::Action(Action::Right)));
        assert_eq!(app.diagnostics.ping.request.interval_ms, 1_100);
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::Ping(_))]
        ));

        app.update(Input(InputEvent::Key(KeyEvent::plain(KeyCode::Esc))));
        assert!(!app.diagnostics.focused);
    }

    #[test]
    fn clicking_a_diagnostic_row_switches_tool_even_when_already_focused() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Config;

        let effects = app.update(Input(InputEvent::Action(Action::SelectDiagnostic(
            DiagnosticTool::Trace as u8,
        ))));

        assert_eq!(app.diagnostics.tool, DiagnosticTool::Trace);
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Menu);
        assert!(matches!(effects.as_slice(), [Effect::PersistSession(_)]));
    }

    #[test]
    fn diagnostic_history_mouse_selection_fills_the_active_target() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Config;
        app.diagnostics.target_history = vec!["1.1.1.1".into(), "8.8.8.8".into()];
        app.diagnostics.history_open = true;
        let effects = app.update(Input(InputEvent::Action(Action::SelectDiagnosticHistory(
            1,
        ))));
        assert_eq!(app.diagnostics.ping.request.target, "8.8.8.8");
        assert!(!app.diagnostics.history_open);
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::Ping(_))]
        ));
    }

    #[test]
    fn trace_raw_config_loads_persists_and_clamps_at_execution() {
        let mut app = AppModel::default();
        let mut config = crate::ConfigData::default();
        config.session.trace = crate::TracePersist {
            target: "trace.example".into(),
            max_hops: "999".into(),
            timeout_ms: "1".into(),
        };
        config.session.history.targets = vec!["trace.example".into()];
        app.apply_config(&config);
        assert_eq!(app.diagnostics.trace.max_hops_input, "999");
        assert_eq!(app.diagnostics.trace.request.max_hops, 30);
        assert_eq!(app.diagnostics.trace.request.timeout_ms, 100);

        app.page = Page::Diagnostics;
        app.update(Input(InputEvent::Action(Action::SelectDiagnostic(1))));
        app.update(Input(InputEvent::Action(Action::FocusDiagnostic(
            DiagnosticFocus::Main,
        ))));
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartTrace { request, .. } = &effects[0] else {
            panic!()
        };
        assert_eq!(request.max_hops, 30);
        assert_eq!(request.timeout_ms, 100);
        assert_eq!(request.target, "trace.example");
    }

    fn shared_link_adapter(guid: &str, name: &str, wifi: bool) -> AdapterInfo {
        AdapterInfo {
            name: name.into(),
            guid: guid.into(),
            kind: if wifi { "wireless" } else { "wired" }.into(),
            ipv4: if wifi { "192.168.1.21" } else { "192.168.1.20" }.into(),
            mac: if wifi {
                "02:00:00:00:00:21"
            } else {
                "02:00:00:00:00:20"
            }
            .into(),
            status: "up".into(),
            ssid: wifi.then(|| "Lab".into()),
            is_physical: true,
            link_speed_bps: Some(if wifi { 866_000_000 } else { 1_000_000_000 }),
            ..AdapterInfo::default()
        }
    }

    #[test]
    fn link_quality_restores_per_adapter_params_and_validates_request() {
        let mut app = AppModel::default();
        let mut config = crate::ConfigData::default();
        config.session.link_quality.adapters.insert(
            "eth-guid".into(),
            crate::LinkParams {
                target: "1.1.1.1".into(),
                count: "4".into(),
                interval_ms: "10".into(),
                timeout_ms: "99999".into(),
                packet_size: "9999".into(),
            },
        );
        config.session.link_quality.adapters.insert(
            "wifi-guid".into(),
            crate::LinkParams {
                target: "8.8.4.4".into(),
                count: "40".into(),
                interval_ms: "300".into(),
                timeout_ms: "1200".into(),
                packet_size: "64".into(),
            },
        );
        config.session.link_quality.selected = Some("wifi-guid".into());
        app.apply_config(&config);
        app.update(Runtime(RuntimeEvent::AdaptersUpdated(vec![
            shared_link_adapter("eth-guid", "Ethernet", false),
            shared_link_adapter("wifi-guid", "Wi-Fi", true),
        ])));
        assert_eq!(
            app.diagnostics
                .link_quality
                .request
                .adapter
                .as_ref()
                .map(|adapter| adapter.key.as_str()),
            Some("wifi-guid")
        );
        assert_eq!(app.diagnostics.link_quality.params.target, "8.8.4.4");

        app.page = Page::Diagnostics;
        app.diagnostics.tool = DiagnosticTool::LinkQuality;
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Config;
        app.update(Input(InputEvent::Action(Action::SelectDiagnosticField(
            0, 0,
        ))));
        let effects = app.update(Input(InputEvent::Action(Action::Left)));
        assert_eq!(
            app.diagnostics
                .link_quality
                .request
                .adapter
                .as_ref()
                .map(|adapter| adapter.key.as_str()),
            Some("eth-guid")
        );
        assert_eq!(app.diagnostics.link_quality.params.target, "1.1.1.1");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::LinkQuality(_))]
        ));

        app.diagnostics.focus = DiagnosticFocus::Main;
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLinkQuality { request, .. } = &effects[0] else {
            panic!("expected link-quality start");
        };
        assert_eq!(request.count, 5);
        assert_eq!(request.interval_ms, 50);
        assert_eq!(request.timeout_ms, 10_000);
        assert_eq!(request.packet_size, 1_472);
        assert!(matches!(
            effects.get(1),
            Some(Effect::PersistSession(crate::SessionUpdate::TargetHistory(
                _
            )))
        ));
    }

    #[test]
    fn public_speed_and_link_quality_lifecycles_are_job_scoped() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
        app.diagnostics.tool = DiagnosticTool::PublicSpeed;
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPublicSpeed { job: first, .. } = effects[0] else {
            panic!("expected public speed start");
        };
        app.update(Runtime(RuntimeEvent::PublicSpeedStarted {
            job: first,
            server: Some("demo.invalid".into()),
        }));
        app.update(Runtime(RuntimeEvent::PublicSpeedSample {
            job: first,
            sample: crate::SpeedSample {
                elapsed_ms: 500,
                bytes: 2_000_000,
                bytes_per_second: 4_000_000,
            },
        }));
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopPublicSpeed(first)]
        );
        let restarted = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPublicSpeed { job: restarted, .. } = restarted[0] else {
            panic!("expected public speed restart");
        };
        app.update(Runtime(RuntimeEvent::PublicSpeedFailed {
            job: first,
            error: crate::RuntimeError::new(crate::RuntimeErrorCode::Network, "stale"),
        }));
        assert_eq!(app.diagnostics.public_speed.common.job, Some(restarted));
        app.update(Runtime(RuntimeEvent::PublicSpeedFinished {
            job: restarted,
            summary: crate::SpeedSummary {
                average_bytes_per_second: 4_000_000,
                peak_bytes_per_second: 5_000_000,
                total_bytes: 8_000_000,
            },
        }));
        assert_eq!(app.diagnostics.public_speed.common.status, TaskStatus::Done);
        let failed = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartPublicSpeed { job: failed, .. } = failed[0] else {
            panic!("expected public speed retry");
        };
        app.update(Runtime(RuntimeEvent::PublicSpeedFailed {
            job: failed,
            error: crate::RuntimeError::new(crate::RuntimeErrorCode::Network, "offline"),
        }));
        assert_eq!(
            app.diagnostics.public_speed.common.status,
            TaskStatus::Failed("offline".into())
        );

        app.update(Runtime(RuntimeEvent::AdaptersUpdated(vec![
            shared_link_adapter("wifi-guid", "Wi-Fi", true),
        ])));
        app.diagnostics.tool = DiagnosticTool::LinkQuality;
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLinkQuality { job, request } = effects[0].clone() else {
            panic!("expected link-quality start");
        };
        let snapshot = crate::LinkQualitySnapshot {
            adapter: request.adapter.expect("adapter selected"),
            wireless: None,
        };
        app.update(Runtime(RuntimeEvent::LinkQualityStarted {
            job,
            snapshot: Box::new(snapshot.clone()),
        }));
        let sample = crate::LinkQualitySample {
            sequence: 1,
            latency_ms: Some(20),
            sent: 1,
            received: 1,
            min_latency_ms: Some(20),
            average_latency_ms: Some(20.0),
            max_latency_ms: Some(20),
            jitter_ms: None,
            loss_percent: 0.0,
            rssi_dbm: None,
            min_rssi_dbm: None,
            average_rssi_dbm: None,
            max_rssi_dbm: None,
            signal_quality: None,
            min_signal_quality: None,
            average_signal_quality: None,
            max_signal_quality: None,
            link_speed_bps: Some(866_000_000),
        };
        app.update(Runtime(RuntimeEvent::LinkQualitySample {
            job,
            sample: sample.clone(),
        }));
        assert_eq!(app.diagnostics.link_quality.samples.first(), Some(&sample));
        let summary = app
            .diagnostics
            .link_quality
            .summary
            .clone()
            .expect("live summary");
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopLinkQuality(job)]
        );
        app.update(Runtime(RuntimeEvent::LinkQualitySample { job, sample }));
        assert_eq!(app.diagnostics.link_quality.samples.len(), 1);
        let restarted = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLinkQuality { job: restarted, .. } = restarted[0] else {
            panic!("expected link-quality restart");
        };
        app.update(Runtime(RuntimeEvent::LinkQualityFinished {
            job,
            summary: summary.clone(),
        }));
        assert_eq!(app.diagnostics.link_quality.common.job, Some(restarted));
        app.update(Runtime(RuntimeEvent::LinkQualityFinished {
            job: restarted,
            summary,
        }));
        assert_eq!(app.diagnostics.link_quality.common.status, TaskStatus::Done);
        let failed = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLinkQuality { job: failed, .. } = failed[0] else {
            panic!("expected link-quality retry");
        };
        app.update(Runtime(RuntimeEvent::LinkQualityFailed {
            job: failed,
            error: crate::RuntimeError::new(
                crate::RuntimeErrorCode::PermissionDenied,
                "permission denied",
            ),
        }));
        assert_eq!(
            app.diagnostics
                .link_quality
                .common
                .error
                .as_ref()
                .map(|error| error.code),
            Some(crate::RuntimeErrorCode::PermissionDenied)
        );
    }

    #[test]
    fn port_scan_and_lan_speed_keep_raw_config_and_validate_at_execution() {
        let mut config = crate::ConfigData::default();
        config.session.port_scan = crate::PortScanPersist {
            target: "scan.example".into(),
            start_port: "200".into(),
            end_port: "100".into(),
            timeout_ms: "1".into(),
        };
        config.session.lan_speed = crate::LanSpeedPersist {
            mode: "client".into(),
            peer: "peer.example".into(),
            port: "50505".into(),
            proto: "udp".into(),
            direction: "bidir".into(),
            duration: "999".into(),
            streams: "99".into(),
            payload: "999999".into(),
            rate: "999999".into(),
        };
        let mut app = AppModel::default();
        app.apply_config(&config);
        assert_eq!(app.diagnostics.port_scan.persist.end_port, "100");
        assert_eq!(app.diagnostics.port_scan.request.timeout_ms, 20);
        assert_eq!(app.diagnostics.lan_speed.persist.duration, "999");
        assert_eq!(app.diagnostics.lan_speed.request.duration_secs, 600);
        assert_eq!(app.diagnostics.lan_speed.request.streams, 32);
        assert_eq!(app.diagnostics.lan_speed.request.payload_size, 65_507);
        assert_eq!(app.diagnostics.lan_speed.request.rate_mbps, 100_000);

        app.page = Page::Diagnostics;
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
        app.diagnostics.tool = DiagnosticTool::PortScan;
        assert!(
            app.update(Input(InputEvent::Action(Action::Toggle)))
                .is_empty()
        );
        assert!(matches!(
            app.diagnostics
                .port_scan
                .common
                .error
                .as_ref()
                .map(|error| error.code),
            Some(crate::RuntimeErrorCode::InvalidRequest)
        ));
        app.diagnostics.port_scan.persist.end_port = "220".into();
        app.sync_port_scan_request();
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        assert!(
            matches!(effects.first(), Some(Effect::StartPortScan { request, .. }) if request.start_port == 200 && request.end_port == 220)
        );

        app.diagnostics.tool = DiagnosticTool::LanSpeed;
        app.diagnostics.focus = DiagnosticFocus::Config;
        app.diagnostics.lan_speed.config_selected = 2;
        let effects = app.update(Input(InputEvent::Action(Action::Left)));
        assert_eq!(app.diagnostics.lan_speed.persist.proto, "tcp");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::LanSpeed(_))]
        ));
    }

    #[test]
    fn lan_speed_lifecycle_is_typed_cancellable_and_generation_scoped() {
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Main;
        app.diagnostics.tool = DiagnosticTool::LanSpeed;
        app.diagnostics.lan_speed.persist = crate::LanSpeedPersist {
            mode: "client".into(),
            peer: "127.0.0.1".into(),
            duration: "1".into(),
            ..crate::LanSpeedPersist::default()
        };
        app.sync_lan_speed_request();
        let effects = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLanSpeed { job, request } = effects[0].clone() else {
            panic!("expected LAN speed start")
        };
        assert_eq!(request.mode, crate::LanSpeedMode::Client);
        app.update(Runtime(RuntimeEvent::LanSpeedStarted {
            job,
            endpoint: "127.0.0.1:50505".into(),
        }));
        app.update(Runtime(RuntimeEvent::LanSpeedStatus {
            job,
            phase: crate::LanSpeedPhase::Connected,
        }));
        let sample = crate::LanSpeedSample {
            elapsed_ms: 250,
            tx_bps: 10_000_000,
            rx_bps: 9_000_000,
            tx_bytes: 2_500_000,
            rx_bytes: 2_250_000,
            loss_percent: None,
            jitter_ms: None,
        };
        app.update(Runtime(RuntimeEvent::LanSpeedSample {
            job,
            sample: sample.clone(),
        }));
        assert_eq!(app.diagnostics.lan_speed.samples, [sample]);
        assert_eq!(
            app.update(Input(InputEvent::Action(Action::Toggle))),
            [Effect::StopLanSpeed(job)]
        );
        let restarted = app.update(Input(InputEvent::Action(Action::Toggle)));
        let Effect::StartLanSpeed { job: restarted, .. } = restarted[0] else {
            panic!("expected LAN speed restart")
        };
        let summary = crate::LanSpeedSummary {
            tx_bytes: 10_000_000,
            rx_bytes: 9_000_000,
            elapsed_ms: 1_000,
            loss_percent: Some(0.1),
            jitter_ms: Some(0.5),
            out_of_order: Some(1),
        };
        app.update(Runtime(RuntimeEvent::LanSpeedFinished {
            job,
            summary: summary.clone(),
        }));
        assert_eq!(app.diagnostics.lan_speed.common.job, Some(restarted));
        app.update(Runtime(RuntimeEvent::LanSpeedFinished {
            job: restarted,
            summary: summary.clone(),
        }));
        assert_eq!(app.diagnostics.lan_speed.summary, Some(summary));
        assert_eq!(app.diagnostics.lan_speed.common.status, TaskStatus::Done);
    }

    #[test]
    fn mapped_native_keys_preserve_text_input_and_custom_navigation() {
        let mapped = |code, action| InputEvent::MappedKey {
            key: KeyEvent::plain(code),
            action,
        };
        let mut app = AppModel {
            page: Page::Diagnostics,
            ..AppModel::default()
        };
        app.diagnostics.focused = true;
        app.diagnostics.focus = DiagnosticFocus::Menu;
        app.update(Input(mapped(KeyCode::Char('j'), Some(Action::Down))));
        assert_eq!(app.diagnostics.tool, DiagnosticTool::Trace);

        app.diagnostics.focus = DiagnosticFocus::Config;
        app.diagnostics.trace.config_selected = 0;
        app.diagnostics.trace.request.target.clear();
        app.diagnostics.cursor = 0;
        let effects = app.update(Input(mapped(KeyCode::Char('j'), Some(Action::Down))));
        assert_eq!(app.diagnostics.trace.request.target, "j");
        assert!(matches!(
            effects.as_slice(),
            [Effect::PersistSession(crate::SessionUpdate::Trace(_))]
        ));

        app.diagnostics.focus = DiagnosticFocus::Main;
        app.update(Input(mapped(KeyCode::Char('n'), Some(Action::NextPage))));
        assert_eq!(app.diagnostics.focus, DiagnosticFocus::Config);
    }

    #[test]
    fn bootstrap_and_background_traffic_keep_native_read_models_live() {
        let mut app = AppModel::default();
        let effects = app.bootstrap_effects();
        assert_eq!(effects.len(), 3);
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::RefreshDashboard { .. }))
        );
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::RefreshAdapters { .. }))
        );
        let traffic_job = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::RefreshTraffic { job } => Some(*job),
                _ => None,
            })
            .unwrap();
        app.dashboard.snapshot.active_interface = Some(DashboardInterface {
            name: "Ethernet".into(),
            ..DashboardInterface::default()
        });
        app.update(Runtime(RuntimeEvent::TrafficRefreshFinished {
            job: traffic_job,
            rows: vec![TrafficRow {
                name: "Ethernet".into(),
                download_bps: 1_000,
                upload_bps: 500,
                total_download: 10_000,
                total_upload: 5_000,
                ..TrafficRow::default()
            }],
        }));
        assert_eq!(app.dashboard.snapshot.download_bps, 1_000);
        assert_eq!(app.dashboard.snapshot.total_upload, 5_000);
    }
}
