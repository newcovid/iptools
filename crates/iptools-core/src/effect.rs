use serde::{Deserialize, Serialize};

use crate::{
    AdapterConfig, AdapterInfo, DashboardSnapshot, DiagnosticTool, PublicIpConfig, ScanHost,
    TrafficRow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    pub language: crate::Language,
    pub scan_concurrency: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId {
    pub tool: ToolKind,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Dashboard,
    Adapters,
    Traffic,
    Scanner,
    Ping,
    Trace,
    PortScan,
    PublicSpeed,
    LinkQuality,
    LanSpeed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DashboardRequest {
    pub public_ip: PublicIpConfig,
}

impl From<DiagnosticTool> for ToolKind {
    fn from(value: DiagnosticTool) -> Self {
        match value {
            DiagnosticTool::Ping => Self::Ping,
            DiagnosticTool::Trace => Self::Trace,
            DiagnosticTool::PortScan => Self::PortScan,
            DiagnosticTool::PublicSpeed => Self::PublicSpeed,
            DiagnosticTool::LinkQuality => Self::LinkQuality,
            DiagnosticTool::LanSpeed => Self::LanSpeed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRequest {
    pub cidr: String,
    pub concurrency: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingRequest {
    pub target: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub packet_size: u64,
}

impl Default for PingRequest {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".into(),
            interval_ms: 1_000,
            timeout_ms: 2_000,
            packet_size: 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceRequest {
    pub target: String,
    pub max_hops: u8,
    pub timeout_ms: u64,
}

impl Default for TraceRequest {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".into(),
            max_hops: 30,
            timeout_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortScanRequest {
    pub target: String,
    pub start_port: u16,
    pub end_port: u16,
    pub timeout_ms: u64,
    pub concurrency: usize,
}

impl Default for PortScanRequest {
    fn default() -> Self {
        Self {
            target: "127.0.0.1".into(),
            start_port: 1,
            end_port: 1_024,
            timeout_ms: 300,
            concurrency: 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicSpeedRequest {
    pub max_duration_ms: u64,
}

impl Default for PublicSpeedRequest {
    fn default() -> Self {
        Self {
            max_duration_ms: 15_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkQualityRequest {
    pub adapter_id: Option<String>,
    pub target: String,
    pub count: u32,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub packet_size: u64,
}

impl Default for LinkQualityRequest {
    fn default() -> Self {
        Self {
            adapter_id: None,
            target: "8.8.8.8".into(),
            count: 20,
            interval_ms: 200,
            timeout_ms: 1_000,
            packet_size: 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LanSpeedMode {
    #[default]
    Server,
    Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LanProtocol {
    #[default]
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LanDirection {
    #[default]
    Upload,
    Download,
    Bidirectional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanSpeedRequest {
    pub mode: LanSpeedMode,
    pub peer: String,
    pub port: u16,
    pub protocol: LanProtocol,
    pub direction: LanDirection,
    pub duration_secs: u64,
    pub streams: u16,
    pub payload_size: u32,
    pub rate_mbps: u32,
}

impl Default for LanSpeedRequest {
    fn default() -> Self {
        Self {
            mode: LanSpeedMode::Server,
            peer: String::new(),
            port: 50_505,
            protocol: LanProtocol::Tcp,
            direction: LanDirection::Upload,
            duration_secs: 10,
            streams: 1,
            payload_size: 65_536,
            rate_mbps: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    PersistPreferences(Preferences),
    RefreshDashboard {
        job: JobId,
        request: DashboardRequest,
    },
    RefreshAdapters {
        job: JobId,
    },
    RefreshTraffic {
        job: JobId,
    },
    ApplyAdapterConfig(AdapterConfig),
    StartScan {
        job: JobId,
        request: ScanRequest,
    },
    CancelScan(JobId),
    StartPing {
        job: JobId,
        request: PingRequest,
    },
    StopPing(JobId),
    StartTrace {
        job: JobId,
        request: TraceRequest,
    },
    StopTrace(JobId),
    StartPortScan {
        job: JobId,
        request: PortScanRequest,
    },
    StopPortScan(JobId),
    StartPublicSpeed {
        job: JobId,
        request: PublicSpeedRequest,
    },
    StopPublicSpeed(JobId),
    StartLinkQuality {
        job: JobId,
        request: LinkQualityRequest,
    },
    StopLinkQuality(JobId),
    StartLanSpeed {
        job: JobId,
        request: LanSpeedRequest,
    },
    StopLanSpeed(JobId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RuntimeErrorCode {
    InvalidRequest,
    ResolveTarget,
    PermissionDenied,
    Timeout,
    Network,
    Cancelled,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeError {
    pub code: RuntimeErrorCode,
    pub message: String,
}

impl RuntimeError {
    pub fn new(code: RuntimeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingSample {
    pub sequence: u64,
    pub latency_ms: Option<u64>,
    pub ttl: Option<u8>,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PingSummary {
    pub sent: u64,
    pub received: u64,
    pub min_ms: Option<u64>,
    pub average_ms: Option<f64>,
    pub max_ms: Option<u64>,
    pub loss_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceHop {
    pub ttl: u8,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedSample {
    pub elapsed_ms: u64,
    pub bytes: u64,
    pub bits_per_second: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedSummary {
    pub average_bps: u64,
    pub peak_bps: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkQualitySample {
    pub sequence: u32,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub loss_percent: f64,
    pub rssi_dbm: Option<i16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinkQualitySummary {
    pub score: f64,
    pub average_latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub loss_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanSpeedSample {
    pub elapsed_ms: u64,
    pub tx_bps: u64,
    pub rx_bps: u64,
    pub loss_percent: Option<f64>,
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LanSpeedSummary {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub elapsed_ms: u64,
    pub loss_percent: Option<f64>,
    pub jitter_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuntimeEvent {
    DashboardUpdated(Box<DashboardSnapshot>),
    DashboardRefreshFinished {
        job: JobId,
        snapshot: Box<DashboardSnapshot>,
    },
    DashboardRefreshFailed {
        job: JobId,
        snapshot: Box<DashboardSnapshot>,
        error: RuntimeError,
    },
    DashboardRefreshCancelled {
        job: JobId,
    },
    AdaptersUpdated(Vec<AdapterInfo>),
    TrafficUpdated(Vec<TrafficRow>),
    AdaptersRefreshFinished {
        job: JobId,
        adapters: Vec<AdapterInfo>,
    },
    AdaptersRefreshFailed {
        job: JobId,
        error: RuntimeError,
    },
    AdaptersRefreshCancelled {
        job: JobId,
    },
    TrafficRefreshFinished {
        job: JobId,
        rows: Vec<TrafficRow>,
    },
    TrafficRefreshFailed {
        job: JobId,
        error: RuntimeError,
    },
    TrafficRefreshCancelled {
        job: JobId,
    },
    AdapterConfigApplied(Result<String, String>),
    ScanStarted {
        job: JobId,
        total: u64,
    },
    ScanProgress {
        job: JobId,
        current: u64,
        total: u64,
    },
    ScanHostFound {
        job: JobId,
        host: ScanHost,
    },
    ScanFinished {
        job: JobId,
    },
    ScanCancelled {
        job: JobId,
    },
    PingStarted {
        job: JobId,
    },
    PingSample {
        job: JobId,
        sample: PingSample,
    },
    PingFinished {
        job: JobId,
        summary: PingSummary,
    },
    PingFailed {
        job: JobId,
        error: RuntimeError,
    },
    TraceStarted {
        job: JobId,
    },
    TraceHop {
        job: JobId,
        hop: TraceHop,
    },
    TraceFinished {
        job: JobId,
        hops: u8,
    },
    TraceFailed {
        job: JobId,
        error: RuntimeError,
    },
    PortScanStarted {
        job: JobId,
        total: u64,
    },
    PortScanProgress {
        job: JobId,
        scanned: u64,
        total: u64,
    },
    PortScanOpen {
        job: JobId,
        port: u16,
    },
    PortScanFinished {
        job: JobId,
        scanned: u64,
        total: u64,
        cancelled: bool,
    },
    PortScanFailed {
        job: JobId,
        error: RuntimeError,
    },
    PublicSpeedStarted {
        job: JobId,
        server: Option<String>,
    },
    PublicSpeedSample {
        job: JobId,
        sample: SpeedSample,
    },
    PublicSpeedFinished {
        job: JobId,
        summary: SpeedSummary,
    },
    PublicSpeedFailed {
        job: JobId,
        error: RuntimeError,
    },
    LinkQualityStarted {
        job: JobId,
    },
    LinkQualitySample {
        job: JobId,
        sample: LinkQualitySample,
    },
    LinkQualityFinished {
        job: JobId,
        summary: LinkQualitySummary,
    },
    LinkQualityFailed {
        job: JobId,
        error: RuntimeError,
    },
    LanSpeedStarted {
        job: JobId,
    },
    LanSpeedSample {
        job: JobId,
        sample: LanSpeedSample,
    },
    LanSpeedFinished {
        job: JobId,
        summary: LanSpeedSummary,
    },
    LanSpeedFailed {
        job: JobId,
        error: RuntimeError,
    },
}
