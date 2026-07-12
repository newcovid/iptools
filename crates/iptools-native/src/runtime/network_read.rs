use std::{collections::HashMap, time::Instant};

use iptools_core::{AdapterInfo, JobId, RuntimeError, RuntimeErrorCode, RuntimeEvent, TrafficRow};
use sysinfo::Networks;

use super::{NativeRuntime, RuntimeTaskError};
use crate::utils::net;

#[derive(Debug, Clone, Copy)]
struct NetworkPoint {
    download_bps: u64,
    upload_bps: u64,
    total_download: u64,
    total_upload: u64,
    session_download: u64,
    session_upload: u64,
}

#[derive(Debug, Clone, Copy)]
struct CounterSample {
    received: u64,
    transmitted: u64,
    sampled_at: Instant,
    download_bps: u64,
    upload_bps: u64,
}

pub(super) struct NetworkSampler {
    networks: Networks,
    history: HashMap<String, CounterSample>,
    initial: HashMap<String, (u64, u64)>,
}

impl NetworkSampler {
    pub(super) fn new() -> Self {
        let mut networks = Networks::new_with_refreshed_list();
        networks.refresh(true);
        let initial = networks
            .iter()
            .map(|(name, data)| {
                (
                    name.clone(),
                    (data.total_received(), data.total_transmitted()),
                )
            })
            .collect();
        Self {
            networks,
            history: HashMap::new(),
            initial,
        }
    }

    fn sample(&mut self) -> HashMap<String, NetworkPoint> {
        self.networks.refresh(true);
        let now = Instant::now();
        let mut points = HashMap::new();
        for (name, data) in &self.networks {
            let received = data.total_received();
            let transmitted = data.total_transmitted();
            let initial = self
                .initial
                .entry(name.clone())
                .or_insert((received, transmitted));
            let (download_bps, upload_bps) = self.history.get(name).map_or((0, 0), |previous| {
                let elapsed = now.duration_since(previous.sampled_at).as_secs_f64();
                if elapsed > 0.1 {
                    (
                        (received.saturating_sub(previous.received) as f64 / elapsed) as u64,
                        (transmitted.saturating_sub(previous.transmitted) as f64 / elapsed) as u64,
                    )
                } else {
                    (previous.download_bps, previous.upload_bps)
                }
            });
            self.history.insert(
                name.clone(),
                CounterSample {
                    received,
                    transmitted,
                    sampled_at: now,
                    download_bps,
                    upload_bps,
                },
            );
            points.insert(
                name.clone(),
                NetworkPoint {
                    download_bps,
                    upload_bps,
                    total_download: received,
                    total_upload: transmitted,
                    session_download: received.saturating_sub(initial.0),
                    session_upload: transmitted.saturating_sub(initial.1),
                },
            );
        }
        points
    }
}

impl NativeRuntime {
    pub(super) fn spawn_adapters_refresh(&mut self, job: JobId) {
        let points = self.network_sampler.sample();
        let gate = self.adapter_gate.clone();
        self.spawn(job, move |token, events| async move {
            let permit = tokio::select! {
                _ = token.cancelled() => {
                    events.send(RuntimeEvent::AdaptersRefreshCancelled { job }).await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                    return Ok(());
                }
                permit = gate.acquire_owned() => permit
                    .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?,
            };
            let result = tokio::task::spawn_blocking(net::get_interfaces).await;
            drop(permit);
            if token.is_cancelled() {
                events
                    .send(RuntimeEvent::AdaptersRefreshCancelled { job })
                    .await
                    .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                return Ok(());
            }
            match result {
                Ok(interfaces) => {
                    let mut adapters = interfaces
                        .into_iter()
                        .map(|interface| {
                            let point = points.get(&interface.name).copied();
                            adapter_info(interface, point)
                        })
                        .collect::<Vec<_>>();
                    adapters.sort_by(|left, right| {
                        right
                            .status
                            .cmp(&left.status)
                            .then_with(|| left.name.cmp(&right.name))
                    });
                    events
                        .send(RuntimeEvent::AdaptersRefreshFinished { job, adapters })
                        .await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                }
                Err(error) => {
                    events
                        .send(RuntimeEvent::AdaptersRefreshFailed {
                            job,
                            error: RuntimeError::new(
                                RuntimeErrorCode::Internal,
                                format!("adapter enumeration failed: {error}"),
                            ),
                        })
                        .await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                }
            }
            Ok(())
        });
    }

    pub(super) fn spawn_traffic_refresh(&mut self, job: JobId) {
        let mut rows = self
            .network_sampler
            .sample()
            .into_iter()
            .filter(|(name, _)| !ignored_interface(name))
            .map(|(name, point)| TrafficRow {
                name,
                download_bps: point.download_bps,
                upload_bps: point.upload_bps,
                total_download: point.total_download,
                total_upload: point.total_upload,
                session_download: point.session_download,
                session_upload: point.session_upload,
            })
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| {
            right
                .download_bps
                .cmp(&left.download_bps)
                .then_with(|| left.name.cmp(&right.name))
        });
        self.spawn(job, move |token, events| async move {
            let event = if token.is_cancelled() {
                RuntimeEvent::TrafficRefreshCancelled { job }
            } else {
                RuntimeEvent::TrafficRefreshFinished { job, rows }
            };
            events
                .send(event)
                .await
                .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
            Ok(())
        });
    }
}

fn adapter_info(interface: net::InterfaceInfo, point: Option<NetworkPoint>) -> AdapterInfo {
    let point = point.unwrap_or(NetworkPoint {
        download_bps: 0,
        upload_bps: 0,
        total_download: 0,
        total_upload: 0,
        session_download: 0,
        session_upload: 0,
    });
    AdapterInfo {
        name: interface.name,
        description: interface.description,
        guid: interface.guid,
        kind: interface.interface_type,
        ipv4: interface.ipv4.first().cloned().unwrap_or_default(),
        cidr: interface.cidr,
        ipv6: interface.ipv6,
        mac: interface.mac,
        status: if interface.is_up { "up" } else { "down" }.into(),
        ssid: interface.ssid,
        dhcp_enabled: interface.dhcp_enabled,
        is_physical: interface.is_physical,
        link_speed_bps: interface.link_speed_bps,
        download_bps: point.download_bps,
        upload_bps: point.upload_bps,
        total_download: point.total_download,
        total_upload: point.total_upload,
    }
}

fn ignored_interface(name: &str) -> bool {
    const KEYWORDS: [&str; 13] = [
        "loopback",
        "pseudo",
        "isatap",
        "teredo",
        "npcap",
        "packet driver",
        "genicam",
        "tpacket",
        "driver-",
        "lltdio",
        "rspndr",
        "virtual box",
        "vmware",
    ];
    let name = name.to_lowercase();
    KEYWORDS.iter().any(|keyword| name.contains(keyword))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iptools_core::{Action, AppModel, InputEvent, Message, Page, TaskStatus};

    use super::*;

    async fn drive_until_idle(model: &mut AppModel, runtime: &mut NativeRuntime) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                runtime.reap_finished();
                while let Some(event) = runtime.try_recv() {
                    model.update(Message::Runtime(event));
                }
                if model.adapters.job.is_none() && model.traffic.job.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("network read should finish");
    }

    #[tokio::test]
    async fn shared_runtime_refreshes_adapters_and_traffic() {
        let mut runtime = NativeRuntime::new();
        let mut model = AppModel::default();
        model.demo = false;
        model.page = Page::Adapters;
        let [adapter_effect] = model
            .update(Message::Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        runtime.dispatch(adapter_effect).unwrap();
        drive_until_idle(&mut model, &mut runtime).await;
        assert_eq!(model.adapters.status, TaskStatus::Done);

        model.page = Page::Traffic;
        let [traffic_effect] = model
            .update(Message::Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        runtime.dispatch(traffic_effect).unwrap();
        drive_until_idle(&mut model, &mut runtime).await;
        assert_eq!(model.traffic.status, TaskStatus::Done);
    }

    #[test]
    fn legacy_filter_keywords_are_preserved() {
        assert!(ignored_interface("Npcap Loopback Adapter"));
        assert!(ignored_interface("VMware Network Adapter VMnet8"));
        assert!(!ignored_interface("Ethernet"));
    }
}
