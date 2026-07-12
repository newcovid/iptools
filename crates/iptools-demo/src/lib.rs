//! Deterministic scenarios shared by the native and browser demos.

use std::{collections::VecDeque, str::FromStr};

use iptools_core::{
    AdapterApplyOutcome, AdapterInfo, DashboardInterface, DashboardSnapshot, Effect, JobId,
    LanSpeedMode, LanSpeedPhase, LanSpeedRequest, LanSpeedSample, LanSpeedSummary,
    LinkQualityAdapter, LinkQualityGrade, LinkQualityRequest, LinkQualitySample,
    LinkQualitySnapshot, LinkQualitySummary, PingRequest, PingSample, PingSummary, PortScanRequest,
    PortScanResult, PublicIpInfo, PublicSpeedRequest, RuntimeError, RuntimeErrorCode, RuntimeEvent,
    ScanHost, SpeedSample, SpeedSummary, ToolKind, TraceHop, TraceRequest, TrafficRow,
    WirelessSnapshot,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScenarioId {
    #[default]
    HomeNetwork,
    WifiDegraded,
    MultiAdapter,
}

impl ScenarioId {
    pub const ALL: [Self; 3] = [Self::HomeNetwork, Self::WifiDegraded, Self::MultiAdapter];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HomeNetwork => "home-network",
            Self::WifiDegraded => "wifi-degraded",
            Self::MultiAdapter => "multi-adapter",
        }
    }
}

impl FromStr for ScenarioId {
    type Err = ScenarioError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "home-network" => Ok(Self::HomeNetwork),
            "wifi-degraded" => Ok(Self::WifiDegraded),
            "multi-adapter" => Ok(Self::MultiAdapter),
            other => Err(ScenarioError::Unknown(other.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("unknown demo scenario: {0}")]
    Unknown(String),
    #[error("invalid built-in demo scenario: {0}")]
    Invalid(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Deserialize)]
struct Scenario {
    id: String,
    hostname: String,
    public_ip: String,
    download_bps: u64,
    upload_bps: u64,
    latency_ms: u64,
    adapters: Vec<AdapterInfo>,
    scan_hosts: Vec<ScanHost>,
}

#[derive(Debug, Clone)]
struct ScheduledEvent {
    at_ms: u64,
    event: RuntimeEvent,
}

/// Pure, tick-driven runtime. Advancing it never waits on wall-clock time.
#[derive(Debug)]
pub struct DemoRuntime {
    scenario: Scenario,
    elapsed_ms: u64,
    pending: VecDeque<ScheduledEvent>,
}

impl DemoRuntime {
    pub fn new(id: ScenarioId) -> Result<Self, ScenarioError> {
        let json = match id {
            ScenarioId::HomeNetwork => include_str!("../scenarios/home-network.json"),
            ScenarioId::WifiDegraded => include_str!("../scenarios/wifi-degraded.json"),
            ScenarioId::MultiAdapter => include_str!("../scenarios/multi-adapter.json"),
        };
        let scenario: Scenario = serde_json::from_str(json)?;
        debug_assert_eq!(scenario.id, id.as_str());
        Ok(Self {
            scenario,
            elapsed_ms: 0,
            pending: VecDeque::new(),
        })
    }

    pub fn scenario_id(&self) -> &str {
        &self.scenario.id
    }

    pub fn bootstrap(&self) -> Vec<RuntimeEvent> {
        vec![
            RuntimeEvent::DashboardUpdated(Box::new(self.dashboard_snapshot())),
            RuntimeEvent::AdaptersUpdated(self.scenario.adapters.clone()),
            RuntimeEvent::TrafficUpdated(self.traffic_rows()),
        ]
    }

    pub fn dispatch(&mut self, effect: Effect) -> Vec<RuntimeEvent> {
        match effect {
            Effect::PersistPreferences(_)
            | Effect::PersistSession(_)
            | Effect::PersistAdapterEdit { .. } => Vec::new(),
            Effect::RefreshDashboard { job, .. } => {
                vec![RuntimeEvent::DashboardRefreshFinished {
                    job,
                    snapshot: Box::new(self.dashboard_snapshot()),
                }]
            }
            Effect::RefreshAdapters { job } => {
                vec![RuntimeEvent::AdaptersRefreshFinished {
                    job,
                    adapters: self.scenario.adapters.clone(),
                }]
            }
            Effect::RefreshTraffic { job } => vec![RuntimeEvent::TrafficRefreshFinished {
                job,
                rows: self.traffic_rows(),
            }],
            Effect::ApplyAdapterConfig { job, request } => {
                let mut events = vec![RuntimeEvent::AdapterConfigStarted { job }];
                let result = self
                    .scenario
                    .adapters
                    .iter_mut()
                    .find(|adapter| adapter.guid == request.guid);
                match result {
                    Some(adapter) if !adapter.status.eq_ignore_ascii_case("disconnected") => {
                        adapter.dhcp_enabled = request.use_dhcp;
                        if !request.use_dhcp {
                            adapter.ipv4 = request.ip;
                            adapter.cidr = mask_prefix(&request.mask)
                                .map(|prefix| format!("{}/{prefix}", adapter.ipv4));
                        }
                        events.push(RuntimeEvent::AdapterConfigFinished {
                            job,
                            outcome: AdapterApplyOutcome::Simulated,
                        });
                    }
                    Some(_) => events.push(RuntimeEvent::AdapterConfigFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::Network,
                            "simulated adapter is disconnected",
                        ),
                    }),
                    None => events.push(RuntimeEvent::AdapterConfigFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::InvalidRequest,
                            "simulated adapter was not found",
                        ),
                    }),
                }
                events
            }
            Effect::StartScan { job, .. } => {
                self.cancel_job(job);
                let total = 254;
                self.schedule(0, RuntimeEvent::ScanStarted { job, total });
                let hosts = self.scenario.scan_hosts.clone();
                for (index, host) in hosts.into_iter().enumerate() {
                    let at = 350 * (index as u64 + 1);
                    self.schedule(
                        at,
                        RuntimeEvent::ScanProgress {
                            job,
                            current: ((index + 1) as u64 * 254
                                / self.scenario.scan_hosts.len() as u64),
                            total,
                        },
                    );
                    self.schedule(at + 80, RuntimeEvent::ScanHostFound { job, host });
                }
                self.schedule(
                    350 * (self.scenario.scan_hosts.len() as u64 + 1),
                    RuntimeEvent::ScanFinished { job },
                );
                Vec::new()
            }
            Effect::CancelScan(job) => {
                self.cancel_job(job);
                vec![RuntimeEvent::ScanCancelled { job }]
            }
            Effect::StartPing { job, request } => {
                self.start_ping(job, request);
                Vec::new()
            }
            Effect::StartTrace { job, request } => {
                self.start_trace(job, request);
                Vec::new()
            }
            Effect::StartPortScan { job, request } => {
                self.start_port_scan(job, request);
                Vec::new()
            }
            Effect::StartPublicSpeed { job, request } => {
                self.start_public_speed(job, request);
                Vec::new()
            }
            Effect::StartLinkQuality { job, request } => {
                self.start_link_quality(job, request);
                Vec::new()
            }
            Effect::StartLanSpeed { job, request } => {
                self.start_lan_speed(job, request);
                Vec::new()
            }
            Effect::StopPing(job)
            | Effect::StopTrace(job)
            | Effect::StopPortScan(job)
            | Effect::StopPublicSpeed(job)
            | Effect::StopLinkQuality(job)
            | Effect::StopLanSpeed(job) => {
                self.cancel_job(job);
                vec![cancelled_event(job)]
            }
        }
    }

    fn dashboard_snapshot(&self) -> DashboardSnapshot {
        let active_interface = self
            .scenario
            .adapters
            .first()
            .map(|adapter| DashboardInterface {
                name: adapter.name.clone(),
                description: adapter.kind.clone(),
                ipv4: adapter.ipv4.clone(),
                ssid: None,
                is_physical: true,
                dhcp_enabled: true,
            });
        DashboardSnapshot {
            observed_at: "2026-01-15 10:24:00".into(),
            hostname: self.scenario.hostname.clone(),
            os_name: "iptools demo".into(),
            os_version: "0.4".into(),
            active_interface,
            proxy: None,
            public_info: Some(PublicIpInfo {
                ip: self.scenario.public_ip.clone(),
                city: "Demo City".into(),
                region: "Lab".into(),
                country: "TEST".into(),
                isp: "Simulated network".into(),
            }),
            download_bps: self.scenario.download_bps,
            upload_bps: self.scenario.upload_bps,
            total_download: self.scenario.download_bps.saturating_mul(3_600),
            total_upload: self.scenario.upload_bps.saturating_mul(3_600),
        }
    }

    pub fn advance(&mut self, delta_ms: u64) -> Vec<RuntimeEvent> {
        self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
        let mut ready = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|event| event.at_ms <= self.elapsed_ms)
        {
            if let Some(event) = self.pending.pop_front() {
                ready.push(event.event);
            }
        }
        ready
    }

    fn start_ping(&mut self, job: JobId, request: PingRequest) {
        self.cancel_job(job);
        self.schedule(0, RuntimeEvent::PingStarted { job });
        for step in 1..=8 {
            self.schedule(
                step as u64 * 320,
                RuntimeEvent::PingSample {
                    job,
                    sample: PingSample {
                        sequence: step as u64,
                        latency_ms: Some(self.scenario.latency_ms + step as u64 % 4),
                        ttl: Some(64),
                        size: request.packet_size as usize,
                        sent: step as u64,
                        received: step as u64,
                        min_ms: Some(self.scenario.latency_ms),
                        average_ms: Some(self.scenario.latency_ms as f64 + 1.5),
                        max_ms: Some(self.scenario.latency_ms + 3),
                        loss_percent: 0.0,
                    },
                },
            );
        }
        self.schedule(
            2_900,
            RuntimeEvent::PingFinished {
                job,
                summary: PingSummary {
                    sent: 8,
                    received: 8,
                    min_ms: Some(self.scenario.latency_ms),
                    average_ms: Some(self.scenario.latency_ms as f64 + 1.5),
                    max_ms: Some(self.scenario.latency_ms + 3),
                    loss_percent: 0.0,
                },
            },
        );
    }

    fn start_trace(&mut self, job: JobId, request: TraceRequest) {
        self.cancel_job(job);
        self.schedule(0, RuntimeEvent::TraceStarted { job });
        let hops = request.max_hops.min(8);
        for ttl in 1..=hops {
            self.schedule(
                ttl as u64 * 320,
                RuntimeEvent::TraceHop {
                    job,
                    hop: TraceHop {
                        ttl,
                        address: Some(format!("192.0.2.{ttl}")),
                        hostname: None,
                        latency_ms: Some(self.scenario.latency_ms / 2 + ttl as u64 * 2),
                    },
                },
            );
        }
        self.schedule(2_900, RuntimeEvent::TraceFinished { job, hops });
    }

    fn start_port_scan(&mut self, job: JobId, request: PortScanRequest) {
        self.cancel_job(job);
        let total = u64::from(request.end_port.saturating_sub(request.start_port)) + 1;
        self.schedule(0, RuntimeEvent::PortScanStarted { job, total });
        let open_ports: Vec<u16> = [22, 443, 8_080]
            .into_iter()
            .filter(|port| (request.start_port..=request.end_port).contains(port))
            .collect();
        for step in 1..=8 {
            let scanned = total.saturating_mul(step) / 8;
            self.schedule(
                step * 320,
                RuntimeEvent::PortScanProgress {
                    job,
                    scanned,
                    total,
                },
            );
            if let Some(port) = open_ports.get(step as usize % open_ports.len().max(1)) {
                self.schedule(
                    step * 320 + 40,
                    RuntimeEvent::PortScanOpen {
                        job,
                        result: PortScanResult {
                            port: *port,
                            service: match port {
                                22 => "SSH",
                                443 => "HTTPS",
                                8_080 => "HTTP-Alt",
                                _ => "-",
                            }
                            .into(),
                        },
                    },
                );
            }
        }
        self.schedule(
            2_900,
            RuntimeEvent::PortScanFinished {
                job,
                scanned: total,
                total,
                cancelled: false,
            },
        );
    }

    fn start_public_speed(&mut self, job: JobId, request: PublicSpeedRequest) {
        self.cancel_job(job);
        self.schedule(
            0,
            RuntimeEvent::PublicSpeedStarted {
                job,
                server: Some("demo.invalid".into()),
            },
        );
        for step in 1..=8 {
            let elapsed_ms = request.max_duration_ms.saturating_mul(step) / 8;
            let bytes_per_second = 4_000_000 + step * 800_000;
            self.schedule(
                step * 320,
                RuntimeEvent::PublicSpeedSample {
                    job,
                    sample: SpeedSample {
                        elapsed_ms,
                        bytes: bytes_per_second * elapsed_ms / 1_000,
                        bytes_per_second,
                    },
                },
            );
        }
        self.schedule(
            2_900,
            RuntimeEvent::PublicSpeedFinished {
                job,
                summary: SpeedSummary {
                    average_bytes_per_second: 9_200_000,
                    peak_bytes_per_second: 10_400_000,
                    total_bytes: 138_000_000,
                },
            },
        );
    }

    fn start_link_quality(&mut self, job: JobId, request: LinkQualityRequest) {
        self.cancel_job(job);
        let adapter = request.adapter.or_else(|| {
            self.scenario.adapters.iter().find_map(|adapter| {
                (adapter.is_physical && adapter.ipv4.parse::<std::net::Ipv4Addr>().is_ok()).then(
                    || LinkQualityAdapter {
                        key: if adapter.guid.is_empty() {
                            adapter.mac.clone()
                        } else {
                            adapter.guid.clone()
                        },
                        name: adapter.name.clone(),
                        guid: adapter.guid.clone(),
                        ipv4: adapter.ipv4.clone(),
                        is_wifi: adapter.kind.eq_ignore_ascii_case("wireless")
                            || adapter.ssid.is_some(),
                        link_speed_bps: adapter.link_speed_bps,
                        mac: adapter.mac.clone(),
                    },
                )
            })
        });
        let Some(adapter) = adapter else {
            self.schedule(
                0,
                RuntimeEvent::LinkQualityFailed {
                    job,
                    error: RuntimeError::new(
                        RuntimeErrorCode::InvalidRequest,
                        "no demo adapter is available",
                    ),
                },
            );
            return;
        };
        let degraded = self.scenario.latency_ms > 100;
        let snapshot = LinkQualitySnapshot {
            wireless: adapter.is_wifi.then(|| WirelessSnapshot {
                ssid: self
                    .scenario
                    .adapters
                    .iter()
                    .find(|candidate| candidate.guid == adapter.guid)
                    .and_then(|candidate| candidate.ssid.clone())
                    .unwrap_or_else(|| "Demo Wi-Fi".into()),
                bssid: "02:AA:BB:CC:DD:01".into(),
                signal_quality: if degraded { 34 } else { 88 },
                rssi_dbm: if degraded { -79 } else { -55 },
                phy_type: "802.11ax · Wi-Fi 6".into(),
                wifi_generation: 6,
                band: "5 GHz".into(),
                channel: 36,
                frequency_mhz: 5_180,
                rx_rate_mbps: if degraded { 72 } else { 866 },
                tx_rate_mbps: if degraded { 58 } else { 780 },
                authentication: "WPA2-Personal".into(),
                cipher: "CCMP (AES)".into(),
            }),
            adapter,
        };
        self.schedule(
            0,
            RuntimeEvent::LinkQualityStarted {
                job,
                snapshot: Box::new(snapshot.clone()),
            },
        );
        let count = request.count.clamp(5, 8);
        let mut final_sample = None;
        for sequence in 1..=count {
            let lost = degraded && sequence % 4 == 0;
            let received = if degraded {
                sequence - sequence / 4
            } else {
                sequence
            };
            let latency = (!lost).then_some(self.scenario.latency_ms + u64::from(sequence % 4));
            let rssi = snapshot
                .wireless
                .as_ref()
                .map(|wireless| wireless.rssi_dbm - sequence as i32 % 3);
            let quality = snapshot
                .wireless
                .as_ref()
                .map(|wireless| wireless.signal_quality.saturating_sub(sequence % 3));
            let sample = LinkQualitySample {
                sequence,
                latency_ms: latency,
                sent: sequence,
                received,
                min_latency_ms: Some(self.scenario.latency_ms),
                average_latency_ms: Some(self.scenario.latency_ms as f64 + 1.5),
                max_latency_ms: Some(self.scenario.latency_ms + 3),
                jitter_ms: Some(if degraded { 18.0 } else { 2.0 }),
                loss_percent: (sequence - received) as f64 * 100.0 / sequence as f64,
                rssi_dbm: rssi,
                min_rssi_dbm: rssi.map(|value| value - 2),
                average_rssi_dbm: rssi.map(|value| value as f64 - 1.0),
                max_rssi_dbm: rssi,
                signal_quality: quality,
                min_signal_quality: quality.map(|value| value.saturating_sub(2)),
                average_signal_quality: quality.map(|value| value as f64 - 1.0),
                max_signal_quality: quality,
                link_speed_bps: snapshot.adapter.link_speed_bps,
            };
            self.schedule(
                sequence as u64 * 320,
                RuntimeEvent::LinkQualitySample {
                    job,
                    sample: sample.clone(),
                },
            );
            final_sample = Some(sample);
        }
        self.schedule(
            2_900,
            RuntimeEvent::LinkQualityFinished {
                job,
                summary: iptools_core::link_quality::summary_from_sample(
                    &snapshot,
                    &final_sample.expect("demo count is clamped above zero"),
                ),
            },
        );
    }

    fn start_lan_speed(&mut self, job: JobId, request: LanSpeedRequest) {
        self.cancel_job(job);
        let endpoint = if request.mode == LanSpeedMode::Server {
            format!("192.168.1.20:{}", request.port)
        } else {
            format!("{}:{}", request.peer, request.port)
        };
        self.schedule(0, RuntimeEvent::LanSpeedStarted { job, endpoint });
        self.schedule(
            40,
            RuntimeEvent::LanSpeedStatus {
                job,
                phase: if request.mode == LanSpeedMode::Server {
                    LanSpeedPhase::Listening
                } else {
                    LanSpeedPhase::Connecting
                },
            },
        );
        self.schedule(
            120,
            RuntimeEvent::LanSpeedStatus {
                job,
                phase: LanSpeedPhase::Connected,
            },
        );
        for step in 1..=8 {
            let elapsed_ms = request
                .duration_secs
                .saturating_mul(1_000)
                .saturating_mul(step)
                / 8;
            self.schedule(
                step * 320,
                RuntimeEvent::LanSpeedSample {
                    job,
                    sample: LanSpeedSample {
                        elapsed_ms,
                        tx_bps: (144 + step * 24) * 1_000_000,
                        rx_bps: (132 + step * 22) * 1_000_000,
                        tx_bytes: 45_000_000 * step,
                        rx_bytes: 41_250_000 * step,
                        loss_percent: Some(0.2),
                        jitter_ms: Some(1.4),
                    },
                },
            );
        }
        self.schedule(
            2_900,
            RuntimeEvent::LanSpeedFinished {
                job,
                summary: LanSpeedSummary {
                    tx_bytes: 360_000_000,
                    rx_bytes: 330_000_000,
                    elapsed_ms: request.duration_secs * 1_000,
                    loss_percent: Some(0.2),
                    jitter_ms: Some(1.4),
                    out_of_order: Some(2),
                },
            },
        );
    }

    fn schedule(&mut self, delay_ms: u64, event: RuntimeEvent) {
        let scheduled = ScheduledEvent {
            at_ms: self.elapsed_ms.saturating_add(delay_ms),
            event,
        };
        let index = self
            .pending
            .iter()
            .position(|existing| existing.at_ms > scheduled.at_ms)
            .unwrap_or(self.pending.len());
        self.pending.insert(index, scheduled);
    }

    fn cancel_job(&mut self, job: JobId) {
        self.pending
            .retain(|scheduled| event_job(&scheduled.event) != Some(job));
    }

    fn traffic_rows(&self) -> Vec<TrafficRow> {
        self.scenario
            .adapters
            .iter()
            .enumerate()
            .map(|(index, adapter)| TrafficRow {
                name: adapter.name.clone(),
                download_bps: self.scenario.download_bps / (index as u64 + 1),
                upload_bps: self.scenario.upload_bps / (index as u64 + 1),
                total_download: 8_589_934_592 * (index as u64 + 1),
                total_upload: 1_610_612_736 * (index as u64 + 1),
                session_download: 734_003_200 * (index as u64 + 1),
                session_upload: 125_829_120 * (index as u64 + 1),
            })
            .collect()
    }
}

fn event_job(event: &RuntimeEvent) -> Option<JobId> {
    match event {
        RuntimeEvent::DashboardRefreshFinished { job, .. }
        | RuntimeEvent::DashboardRefreshFailed { job, .. }
        | RuntimeEvent::DashboardRefreshCancelled { job }
        | RuntimeEvent::AdaptersRefreshFinished { job, .. }
        | RuntimeEvent::AdaptersRefreshFailed { job, .. }
        | RuntimeEvent::AdaptersRefreshCancelled { job }
        | RuntimeEvent::TrafficRefreshFinished { job, .. }
        | RuntimeEvent::TrafficRefreshFailed { job, .. }
        | RuntimeEvent::TrafficRefreshCancelled { job }
        | RuntimeEvent::AdapterConfigStarted { job }
        | RuntimeEvent::AdapterConfigFinished { job, .. }
        | RuntimeEvent::AdapterConfigFailed { job, .. }
        | RuntimeEvent::ScanStarted { job, .. }
        | RuntimeEvent::ScanProgress { job, .. }
        | RuntimeEvent::ScanHostFound { job, .. }
        | RuntimeEvent::ScanFinished { job }
        | RuntimeEvent::ScanCancelled { job }
        | RuntimeEvent::PingStarted { job }
        | RuntimeEvent::PingSample { job, .. }
        | RuntimeEvent::PingFinished { job, .. }
        | RuntimeEvent::PingFailed { job, .. }
        | RuntimeEvent::TraceStarted { job }
        | RuntimeEvent::TraceHop { job, .. }
        | RuntimeEvent::TraceFinished { job, .. }
        | RuntimeEvent::TraceFailed { job, .. }
        | RuntimeEvent::PortScanStarted { job, .. }
        | RuntimeEvent::PortScanProgress { job, .. }
        | RuntimeEvent::PortScanOpen { job, .. }
        | RuntimeEvent::PortScanFinished { job, .. }
        | RuntimeEvent::PortScanFailed { job, .. }
        | RuntimeEvent::PublicSpeedStarted { job, .. }
        | RuntimeEvent::PublicSpeedSample { job, .. }
        | RuntimeEvent::PublicSpeedFinished { job, .. }
        | RuntimeEvent::PublicSpeedFailed { job, .. }
        | RuntimeEvent::LinkQualityStarted { job, .. }
        | RuntimeEvent::LinkQualitySample { job, .. }
        | RuntimeEvent::LinkQualityFinished { job, .. }
        | RuntimeEvent::LinkQualityFailed { job, .. }
        | RuntimeEvent::LanSpeedStarted { job, .. }
        | RuntimeEvent::LanSpeedStatus { job, .. }
        | RuntimeEvent::LanSpeedSample { job, .. }
        | RuntimeEvent::LanSpeedFinished { job, .. }
        | RuntimeEvent::LanSpeedFailed { job, .. } => Some(*job),
        _ => None,
    }
}

fn cancelled_event(job: JobId) -> RuntimeEvent {
    match job.tool {
        ToolKind::Dashboard => RuntimeEvent::DashboardRefreshCancelled { job },
        ToolKind::Adapters => RuntimeEvent::AdaptersRefreshCancelled { job },
        ToolKind::AdapterEdit => RuntimeEvent::AdapterConfigFailed {
            job,
            error: RuntimeError::new(
                RuntimeErrorCode::Cancelled,
                "adapter configuration cancelled",
            ),
        },
        ToolKind::Traffic => RuntimeEvent::TrafficRefreshCancelled { job },
        ToolKind::Scanner => RuntimeEvent::ScanCancelled { job },
        ToolKind::Ping => RuntimeEvent::PingFinished {
            job,
            summary: PingSummary {
                sent: 0,
                received: 0,
                min_ms: None,
                average_ms: None,
                max_ms: None,
                loss_percent: 0.0,
            },
        },
        ToolKind::Trace => RuntimeEvent::TraceFinished { job, hops: 0 },
        ToolKind::PortScan => RuntimeEvent::PortScanFinished {
            job,
            scanned: 0,
            total: 0,
            cancelled: true,
        },
        ToolKind::PublicSpeed => RuntimeEvent::PublicSpeedFinished {
            job,
            summary: SpeedSummary {
                average_bytes_per_second: 0,
                peak_bytes_per_second: 0,
                total_bytes: 0,
            },
        },
        ToolKind::LinkQuality => RuntimeEvent::LinkQualityFinished {
            job,
            summary: LinkQualitySummary {
                score: 0.0,
                grade: LinkQualityGrade::Poor,
                weakest: None,
                dimensions: Vec::new(),
                sent: 0,
                received: 0,
                min_latency_ms: None,
                average_latency_ms: None,
                max_latency_ms: None,
                jitter_ms: None,
                loss_percent: 0.0,
                min_rssi_dbm: None,
                average_rssi_dbm: None,
                max_rssi_dbm: None,
                min_signal_quality: None,
                average_signal_quality: None,
                max_signal_quality: None,
                link_speed_bps: None,
            },
        },
        ToolKind::LanSpeed => RuntimeEvent::LanSpeedFinished {
            job,
            summary: LanSpeedSummary {
                tx_bytes: 0,
                rx_bytes: 0,
                elapsed_ms: 0,
                loss_percent: None,
                jitter_ms: None,
                out_of_order: None,
            },
        },
    }
}

fn mask_prefix(mask: &str) -> Option<u32> {
    let value = mask.parse::<std::net::Ipv4Addr>().ok().map(u32::from)?;
    let prefix = value.leading_ones();
    (value
        == if prefix == 0 {
            0
        } else {
            u32::MAX << (32 - prefix)
        })
    .then_some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptools_core::{
        AdapterConfigRequest, Effect, JobId, LanSpeedMode, LanSpeedRequest, PingRequest,
        ScanRequest,
    };

    #[test]
    fn all_scenarios_parse() {
        for id in ScenarioId::ALL {
            let runtime = DemoRuntime::new(id).unwrap();
            assert_eq!(runtime.scenario_id(), id.as_str());
            assert_eq!(runtime.bootstrap().len(), 3);
        }
    }

    #[test]
    fn scan_timeline_is_deterministic_and_cancellable() {
        let mut runtime = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let job = JobId {
            tool: ToolKind::Scanner,
            generation: 1,
        };
        runtime.dispatch(Effect::StartScan {
            job,
            request: ScanRequest {
                cidr: "192.168.1.0/24".into(),
                concurrency: 50,
            },
        });
        assert!(matches!(
            runtime.advance(0).as_slice(),
            [RuntimeEvent::ScanStarted { .. }]
        ));
        assert!(!runtime.advance(500).is_empty());
        assert!(matches!(
            runtime.dispatch(Effect::CancelScan(job)).as_slice(),
            [RuntimeEvent::ScanCancelled { .. }]
        ));
        assert!(runtime.advance(10_000).is_empty());
    }

    #[test]
    fn diagnostics_finish_without_wall_clock_waiting() {
        let mut runtime = DemoRuntime::new(ScenarioId::WifiDegraded).unwrap();
        let job = JobId {
            tool: ToolKind::Ping,
            generation: 1,
        };
        runtime.dispatch(Effect::StartPing {
            job,
            request: PingRequest {
                target: "192.0.2.1".into(),
                ..PingRequest::default()
            },
        });
        let events = runtime.advance(3_000);
        assert!(events.iter().any(|event| matches!(
            event,
            RuntimeEvent::PingFinished { job: current, .. } if *current == job
        )));

        let trace = JobId {
            tool: ToolKind::Trace,
            generation: 2,
        };
        runtime.dispatch(Effect::StartTrace {
            job: trace,
            request: iptools_core::TraceRequest::default(),
        });
        assert!(matches!(
            runtime.dispatch(Effect::StopTrace(trace)).as_slice(),
            [RuntimeEvent::TraceFinished { job: current, hops: 0 }]
                if *current == trace
        ));
        assert!(
            runtime
                .advance(10_000)
                .iter()
                .all(|event| event_job(event) != Some(trace))
        );
    }

    #[test]
    fn equal_scenario_and_input_produce_identical_event_sequences() {
        let mut native_demo = DemoRuntime::new(ScenarioId::MultiAdapter).unwrap();
        let mut web_demo = DemoRuntime::new(ScenarioId::MultiAdapter).unwrap();
        let job = JobId {
            tool: ToolKind::Ping,
            generation: 7,
        };
        let effect = Effect::StartPing {
            job,
            request: PingRequest::default(),
        };

        assert_eq!(native_demo.bootstrap(), web_demo.bootstrap());
        assert_eq!(
            native_demo.dispatch(effect.clone()),
            web_demo.dispatch(effect)
        );
        for delta in [0, 320, 640, 1_000, 2_000] {
            assert_eq!(native_demo.advance(delta), web_demo.advance(delta));
        }
    }

    #[test]
    fn lan_speed_timeline_is_typed_deterministic_and_cancellable() {
        let mut first = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let mut second = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let job = JobId {
            tool: ToolKind::LanSpeed,
            generation: 11,
        };
        let effect = Effect::StartLanSpeed {
            job,
            request: LanSpeedRequest {
                mode: LanSpeedMode::Client,
                peer: "192.0.2.25".into(),
                duration_secs: 1,
                ..LanSpeedRequest::default()
            },
        };
        assert_eq!(first.dispatch(effect.clone()), second.dispatch(effect));
        let first_events = first.advance(1_500);
        let second_events = second.advance(1_500);
        assert_eq!(first_events, second_events);
        assert!(first_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedStarted { job: current, .. } if *current == job
        )));
        assert!(first_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedSample { job: current, sample }
                if *current == job && sample.tx_bytes > 0 && sample.rx_bytes > 0
        )));

        let restarted = JobId {
            generation: 12,
            ..job
        };
        first.dispatch(Effect::StartLanSpeed {
            job: restarted,
            request: LanSpeedRequest::default(),
        });
        assert!(matches!(
            first.dispatch(Effect::StopLanSpeed(restarted)).as_slice(),
            [RuntimeEvent::LanSpeedFinished { job: current, summary }]
                if *current == restarted && summary.elapsed_ms == 0
        ));
        assert!(
            first
                .advance(10_000)
                .iter()
                .all(|event| event_job(event) != Some(restarted))
        );
    }

    #[test]
    fn adapter_edit_is_simulated_and_updates_only_demo_state() {
        let mut runtime = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let job = JobId {
            tool: ToolKind::AdapterEdit,
            generation: 4,
        };
        let events = runtime.dispatch(Effect::ApplyAdapterConfig {
            job,
            request: AdapterConfigRequest {
                guid: "demo-ethernet".into(),
                name: "Ethernet".into(),
                use_dhcp: false,
                ip: "10.20.30.40".into(),
                mask: "255.255.255.0".into(),
                gateway: Some("10.20.30.1".into()),
                dns: vec!["1.1.1.1".into()],
            },
        });
        assert!(matches!(
            events.as_slice(),
            [
                RuntimeEvent::AdapterConfigStarted { .. },
                RuntimeEvent::AdapterConfigFinished {
                    outcome: AdapterApplyOutcome::Simulated,
                    ..
                }
            ]
        ));
        let RuntimeEvent::AdaptersUpdated(adapters) = &runtime.bootstrap()[1] else {
            panic!()
        };
        assert_eq!(adapters[0].ipv4, "10.20.30.40");
        assert_eq!(adapters[0].cidr.as_deref(), Some("10.20.30.40/24"));

        let failed = runtime.dispatch(Effect::ApplyAdapterConfig {
            job: JobId {
                generation: 5,
                ..job
            },
            request: AdapterConfigRequest {
                guid: "missing".into(),
                name: "Missing".into(),
                use_dhcp: true,
                ip: String::new(),
                mask: String::new(),
                gateway: None,
                dns: Vec::new(),
            },
        });
        assert!(
            matches!(failed.last(), Some(RuntimeEvent::AdapterConfigFailed { error, .. }) if error.code == RuntimeErrorCode::InvalidRequest)
        );
    }
}
