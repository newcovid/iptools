use serde::{Deserialize, Serialize};

use crate::{AdapterConfig, AdapterInfo, DiagnosticTool, ScanHost, TrafficRow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId {
    pub tool: ToolKind,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Scanner,
    Ping,
    Trace,
    PortScan,
    PublicSpeed,
    LinkQuality,
    LanSpeed,
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
pub struct DiagnosticRequest {
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    RefreshDashboard,
    RefreshAdapters,
    ApplyAdapterConfig(AdapterConfig),
    StartScan {
        job: JobId,
        request: ScanRequest,
    },
    CancelScan(JobId),
    StartPing {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopPing(JobId),
    StartTrace {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopTrace(JobId),
    StartPortScan {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopPortScan(JobId),
    StartPublicSpeed {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopPublicSpeed(JobId),
    StartLinkQuality {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopLinkQuality(JobId),
    StartLanSpeed {
        job: JobId,
        request: DiagnosticRequest,
    },
    StopLanSpeed(JobId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuntimeEvent {
    DashboardUpdated {
        hostname: String,
        public_ip: String,
        download_bps: u64,
        upload_bps: u64,
    },
    AdaptersUpdated(Vec<AdapterInfo>),
    TrafficUpdated(Vec<TrafficRow>),
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
    PortScanOpen {
        job: JobId,
        port: u16,
    },
    DiagnosticStarted {
        job: JobId,
    },
    DiagnosticProgress {
        job: JobId,
        progress: u8,
        primary: String,
        detail: String,
    },
    DiagnosticFinished {
        job: JobId,
        summary: String,
    },
    DiagnosticFailed {
        job: JobId,
        error: String,
    },
}
