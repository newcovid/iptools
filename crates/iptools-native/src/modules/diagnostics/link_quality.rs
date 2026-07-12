//! Native source-bound link-quality probes for the structured runtime.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use tokio::sync::mpsc;

use crate::utils::{
    net,
    wlan::{self, WirelessInfo},
};

/// Bridge the established source-bound ICMP and radio sampling algorithm into the
/// shared runtime. The legacy page remains available until the native cutover,
/// but both paths use the same platform probes.
pub(crate) async fn run_shared(
    job: iptools_core::JobId,
    request: iptools_core::LinkQualityRequest,
    cancellation: tokio_util::sync::CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
) -> Result<(), String> {
    use iptools_core::{RuntimeErrorCode, RuntimeEvent};

    let Some(adapter) = request.adapter.clone() else {
        send_link_failure(
            &events,
            job,
            RuntimeErrorCode::InvalidRequest,
            "no active physical IPv4 adapter is available",
        )
        .await?;
        return Ok(());
    };
    let source = match adapter.ipv4.parse::<Ipv4Addr>() {
        Ok(source) => source,
        Err(_) => {
            send_link_failure(
                &events,
                job,
                RuntimeErrorCode::InvalidRequest,
                "selected adapter has no valid IPv4 source address",
            )
            .await?;
            return Ok(());
        }
    };
    let target = request.target.trim();
    if target.is_empty() {
        send_link_failure(
            &events,
            job,
            RuntimeErrorCode::InvalidRequest,
            "target cannot be empty",
        )
        .await?;
        return Ok(());
    }
    let Some(destination) = resolve_v4(target).await else {
        send_link_failure(
            &events,
            job,
            RuntimeErrorCode::ResolveTarget,
            "target did not resolve to an IPv4 address",
        )
        .await?;
        return Ok(());
    };

    let wireless = if adapter.is_wifi {
        let guid = adapter.guid.clone();
        tokio::task::spawn_blocking(move || wlan::query(&guid))
            .await
            .ok()
            .flatten()
            .map(shared_wireless_snapshot)
    } else {
        None
    };
    let snapshot = iptools_core::LinkQualitySnapshot {
        adapter: adapter.clone(),
        wireless,
    };
    events
        .send(RuntimeEvent::LinkQualityStarted {
            job,
            snapshot: Box::new(snapshot.clone()),
        })
        .await
        .map_err(|error| error.to_string())?;

    let count = request.count.clamp(5, 100);
    let interval = Duration::from_millis(request.interval_ms.clamp(50, 5_000));
    let timeout_ms = request.timeout_ms.clamp(100, 10_000) as u32;
    let packet_size = request.packet_size.min(1_472) as usize;
    let mut statistics = SharedLinkStatistics::default();
    let mut last_emit = None;
    let mut last_sample = None;

    for sequence in 0..count {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let mut probe = tokio::task::spawn_blocking(move || {
            super::icmp::echo_once_from(source, destination, 128, timeout_ms, packet_size)
        });
        let result = tokio::select! {
            _ = cancellation.cancelled() => {
                let _ = probe.await;
                return Ok(());
            }
            result = &mut probe => result,
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                send_link_failure(
                    &events,
                    job,
                    RuntimeErrorCode::Internal,
                    &format!("ICMP probe task failed: {error}"),
                )
                .await?;
                return Ok(());
            }
        };
        if result.status == u32::MAX {
            send_link_failure(
                &events,
                job,
                RuntimeErrorCode::PermissionDenied,
                "source-bound ICMP is unsupported or permission was denied",
            )
            .await?;
            return Ok(());
        }
        let latency = result.reached().then_some(result.rtt_ms).flatten();
        let (rssi, quality, link_speed) = if adapter.is_wifi {
            let guid = adapter.guid.clone();
            match tokio::task::spawn_blocking(move || wlan::query(&guid)).await {
                Ok(Some(wireless)) => {
                    (Some(wireless.rssi_dbm), Some(wireless.signal_quality), None)
                }
                _ => (None, None, None),
            }
        } else {
            let guid = adapter.guid.clone();
            let speed = tokio::task::spawn_blocking(move || net::link_speed_for_guid(&guid))
                .await
                .ok()
                .flatten();
            (None, None, speed)
        };
        let sample = statistics.observe(sequence + 1, latency, rssi, quality, link_speed);
        let now = tokio::time::Instant::now();
        if sequence + 1 == count
            || last_emit
                .is_none_or(|previous| now.duration_since(previous) >= Duration::from_millis(250))
        {
            last_emit = Some(now);
            events
                .send(RuntimeEvent::LinkQualitySample {
                    job,
                    sample: sample.clone(),
                })
                .await
                .map_err(|error| error.to_string())?;
        }
        last_sample = Some(sample);
        if sequence + 1 < count {
            tokio::select! {
                _ = cancellation.cancelled() => return Ok(()),
                _ = tokio::time::sleep(interval) => {}
            }
        }
    }

    let summary = iptools_core::link_quality::summary_from_sample(
        &snapshot,
        &last_sample.expect("link-quality count is clamped above zero"),
    );
    events
        .send(RuntimeEvent::LinkQualityFinished { job, summary })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

async fn send_link_failure(
    events: &mpsc::Sender<iptools_core::RuntimeEvent>,
    job: iptools_core::JobId,
    code: iptools_core::RuntimeErrorCode,
    message: &str,
) -> Result<(), String> {
    events
        .send(iptools_core::RuntimeEvent::LinkQualityFailed {
            job,
            error: iptools_core::RuntimeError::new(code, message),
        })
        .await
        .map_err(|error| error.to_string())
}

fn shared_wireless_snapshot(wireless: WirelessInfo) -> iptools_core::WirelessSnapshot {
    iptools_core::WirelessSnapshot {
        ssid: wireless.ssid,
        bssid: wireless.bssid,
        signal_quality: wireless.signal_quality,
        rssi_dbm: wireless.rssi_dbm,
        phy_type: wireless.phy_type,
        wifi_generation: wireless.wifi_gen,
        band: wireless.band,
        channel: wireless.channel,
        frequency_mhz: wireless.freq_mhz,
        rx_rate_mbps: wireless.rx_rate_mbps,
        tx_rate_mbps: wireless.tx_rate_mbps,
        authentication: wireless.auth,
        cipher: wireless.cipher,
    }
}

#[derive(Default)]
struct SharedLinkStatistics {
    latencies: Vec<u64>,
    rssi: Vec<i32>,
    quality: Vec<u32>,
    link_speed_bps: Option<u64>,
}

impl SharedLinkStatistics {
    fn observe(
        &mut self,
        sequence: u32,
        latency_ms: Option<u64>,
        rssi_dbm: Option<i32>,
        signal_quality: Option<u32>,
        link_speed_bps: Option<u64>,
    ) -> iptools_core::LinkQualitySample {
        if let Some(latency) = latency_ms {
            self.latencies.push(latency);
        }
        if let Some(rssi) = rssi_dbm {
            self.rssi.push(rssi);
        }
        if let Some(quality) = signal_quality {
            self.quality.push(quality);
        }
        if link_speed_bps.is_some() {
            self.link_speed_bps = link_speed_bps;
        }
        let received = self.latencies.len() as u32;
        let average_latency = average_u64(&self.latencies);
        let jitter = if self.latencies.len() > 1 {
            Some(
                self.latencies
                    .windows(2)
                    .map(|pair| pair[0].abs_diff(pair[1]) as f64)
                    .sum::<f64>()
                    / (self.latencies.len() - 1) as f64,
            )
        } else {
            None
        };
        iptools_core::LinkQualitySample {
            sequence,
            latency_ms,
            sent: sequence,
            received,
            min_latency_ms: self.latencies.iter().copied().min(),
            average_latency_ms: average_latency,
            max_latency_ms: self.latencies.iter().copied().max(),
            jitter_ms: jitter,
            loss_percent: (sequence - received) as f64 * 100.0 / sequence as f64,
            rssi_dbm,
            min_rssi_dbm: self.rssi.iter().copied().min(),
            average_rssi_dbm: average_i32(&self.rssi),
            max_rssi_dbm: self.rssi.iter().copied().max(),
            signal_quality,
            min_signal_quality: self.quality.iter().copied().min(),
            average_signal_quality: average_u32(&self.quality),
            max_signal_quality: self.quality.iter().copied().max(),
            link_speed_bps: self.link_speed_bps,
        }
    }
}

fn average_u64(values: &[u64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<u64>() as f64 / values.len() as f64)
}

fn average_i32(values: &[i32]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<i32>() as f64 / values.len() as f64)
}

fn average_u32(values: &[u32]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<u32>() as f64 / values.len() as f64)
}

/// Resolve a target to the first IPv4 address.
async fn resolve_v4(target: &str) -> Option<Ipv4Addr> {
    if let Ok(IpAddr::V4(v4)) = target.parse::<IpAddr>() {
        return Some(v4);
    }
    if let Ok(addresses) = tokio::net::lookup_host((target, 0_u16)).await {
        for address in addresses {
            if let IpAddr::V4(v4) = address.ip() {
                return Some(v4);
            }
        }
    }
    None
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    #[test]
    fn coalesced_sample_keeps_latency_loss_radio_and_link_statistics() {
        let mut statistics = SharedLinkStatistics::default();
        statistics.observe(1, Some(20), Some(-55), Some(90), None);
        statistics.observe(2, None, Some(-57), Some(86), None);
        let sample = statistics.observe(3, Some(30), Some(-56), Some(88), Some(1_000_000_000));
        assert_eq!(sample.sent, 3);
        assert_eq!(sample.received, 2);
        assert_eq!(sample.min_latency_ms, Some(20));
        assert_eq!(sample.average_latency_ms, Some(25.0));
        assert_eq!(sample.max_latency_ms, Some(30));
        assert_eq!(sample.jitter_ms, Some(10.0));
        assert!((sample.loss_percent - 33.333).abs() < 0.01);
        assert_eq!(sample.average_rssi_dbm, Some(-56.0));
        assert_eq!(sample.average_signal_quality, Some(88.0));
        assert_eq!(sample.link_speed_bps, Some(1_000_000_000));
    }
}
