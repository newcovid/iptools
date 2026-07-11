//! Deterministic scenarios shared by the native and browser demos.

use std::{collections::VecDeque, str::FromStr};

use iptools_core::{
    AdapterInfo, DiagnosticRequest, Effect, JobId, RuntimeEvent, ScanHost, ToolKind, TrafficRow,
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
            RuntimeEvent::DashboardUpdated {
                hostname: self.scenario.hostname.clone(),
                public_ip: self.scenario.public_ip.clone(),
                download_bps: self.scenario.download_bps,
                upload_bps: self.scenario.upload_bps,
            },
            RuntimeEvent::AdaptersUpdated(self.scenario.adapters.clone()),
            RuntimeEvent::TrafficUpdated(self.traffic_rows()),
        ]
    }

    pub fn dispatch(&mut self, effect: Effect) -> Vec<RuntimeEvent> {
        match effect {
            Effect::RefreshDashboard => vec![self.bootstrap()[0].clone()],
            Effect::RefreshAdapters => vec![RuntimeEvent::AdaptersUpdated(
                self.scenario.adapters.clone(),
            )],
            Effect::ApplyAdapterConfig(config) => vec![RuntimeEvent::AdapterConfigApplied(Ok(
                format!("simulated configuration applied to {}", config.name),
            ))],
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
            Effect::StartPing { job, request }
            | Effect::StartTrace { job, request }
            | Effect::StartPortScan { job, request }
            | Effect::StartPublicSpeed { job, request }
            | Effect::StartLinkQuality { job, request }
            | Effect::StartLanSpeed { job, request } => {
                self.start_diagnostic(job, request);
                Vec::new()
            }
            Effect::StopPing(job)
            | Effect::StopTrace(job)
            | Effect::StopPortScan(job)
            | Effect::StopPublicSpeed(job)
            | Effect::StopLinkQuality(job)
            | Effect::StopLanSpeed(job) => {
                self.cancel_job(job);
                vec![RuntimeEvent::DiagnosticFinished {
                    job,
                    summary: "stopped by user".into(),
                }]
            }
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

    fn start_diagnostic(&mut self, job: JobId, request: DiagnosticRequest) {
        self.cancel_job(job);
        self.schedule(0, RuntimeEvent::DiagnosticStarted { job });
        for step in 1..=8 {
            let progress = step * 12;
            let (primary, detail) =
                diagnostic_sample(job.tool, step, self.scenario.latency_ms, &request.target);
            self.schedule(
                step as u64 * 320,
                RuntimeEvent::DiagnosticProgress {
                    job,
                    progress,
                    primary,
                    detail,
                },
            );
        }
        self.schedule(
            2_900,
            RuntimeEvent::DiagnosticFinished {
                job,
                summary: diagnostic_summary(job.tool, self.scenario.latency_ms),
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
            })
            .collect()
    }
}

fn event_job(event: &RuntimeEvent) -> Option<JobId> {
    match event {
        RuntimeEvent::ScanStarted { job, .. }
        | RuntimeEvent::ScanProgress { job, .. }
        | RuntimeEvent::ScanHostFound { job, .. }
        | RuntimeEvent::ScanFinished { job }
        | RuntimeEvent::ScanCancelled { job }
        | RuntimeEvent::DiagnosticStarted { job }
        | RuntimeEvent::DiagnosticProgress { job, .. }
        | RuntimeEvent::DiagnosticFinished { job, .. }
        | RuntimeEvent::DiagnosticFailed { job, .. } => Some(*job),
        _ => None,
    }
}

fn diagnostic_sample(tool: ToolKind, step: u8, latency: u64, target: &str) -> (String, String) {
    match tool {
        ToolKind::Ping => (
            format!("reply from {target}: time={} ms", latency + step as u64 % 4),
            format!("sequence={step} ttl=64"),
        ),
        ToolKind::Trace => (
            format!("hop {step}: 192.0.2.{step}"),
            format!("{} ms", latency / 2 + step as u64 * 2),
        ),
        ToolKind::PortScan => (
            format!("scanned {} ports", step as u16 * 1024),
            if matches!(step, 2 | 5 | 7) {
                format!("open: {}", [22, 443, 8080][(step as usize / 2) % 3])
            } else {
                "no new open port".into()
            },
        ),
        ToolKind::PublicSpeed => (
            format!("download {:.1} MiB/s", 4.0 + step as f64 * 0.8),
            format!("sample {step}/8"),
        ),
        ToolKind::LinkQuality => (
            format!("latency={} ms · jitter={} ms", latency, 3 + step % 5),
            format!("RSSI={} dBm · loss={} %", -55 - step as i16 * 2, step % 4),
        ),
        ToolKind::LanSpeed => (
            format!("throughput {} MiB/s", 18 + step as u16 * 3),
            format!("stream {}/8", step),
        ),
        ToolKind::Scanner => unreachable!(),
    }
}

fn diagnostic_summary(tool: ToolKind, latency: u64) -> String {
    match tool {
        ToolKind::Ping => format!("8 packets · average {latency} ms · 0% loss"),
        ToolKind::Trace => "route completed in 8 hops".into(),
        ToolKind::PortScan => "3 open ports found".into(),
        ToolKind::PublicSpeed => "average 9.2 MiB/s · peak 10.4 MiB/s".into(),
        ToolKind::LinkQuality => {
            if latency > 100 {
                "score 58/100 · weak wireless link".into()
            } else {
                "score 92/100 · excellent".into()
            }
        }
        ToolKind::LanSpeed => "average 36 MiB/s · 0.2% loss".into(),
        ToolKind::Scanner => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptools_core::{DiagnosticRequest, Effect, JobId, ScanRequest};

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
            request: DiagnosticRequest {
                target: "192.0.2.1".into(),
            },
        });
        let events = runtime.advance(3_000);
        assert!(events.iter().any(|event| matches!(
            event,
            RuntimeEvent::DiagnosticFinished { job: current, .. } if *current == job
        )));
    }
}
