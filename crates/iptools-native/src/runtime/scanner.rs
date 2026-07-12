//! Native Scanner effect handler.

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use ipnetwork::Ipv4Network;
use iptools_core::{JobId, RuntimeEvent, ScanHost, ScanRequest};

use super::NativeRuntime;
use crate::utils::net;

impl NativeRuntime {
    pub(super) fn spawn_scan(&mut self, job: JobId, request: ScanRequest) {
        self.spawn(job, move |token, events| async move {
            let network = request.cidr.parse::<Ipv4Network>().ok();
            let total = network.map_or(0, |network| {
                if network.prefix() < 31 {
                    u64::from(network.size().saturating_sub(2))
                } else {
                    u64::from(network.size())
                }
            });
            let _ = events.send(RuntimeEvent::ScanStarted { job, total }).await;

            if let Some(network) = network {
                let ips: Vec<Ipv4Addr> = if network.prefix() < 31 {
                    network
                        .iter()
                        .skip(1)
                        .take(network.size().saturating_sub(2) as usize)
                        .collect()
                } else {
                    network.iter().collect()
                };
                let completed = Arc::new(AtomicU64::new(0));
                let worker_completed = Arc::clone(&completed);
                let worker_token = token.clone();
                let worker_events = events.clone();
                let worker_count = request.concurrency.max(1).min(ips.len().max(1));
                let workers = tokio::task::spawn_blocking(move || {
                    let ips = Arc::new(ips);
                    let next = Arc::new(AtomicUsize::new(0));
                    std::thread::scope(|scope| {
                        for _ in 0..worker_count {
                            let ips = Arc::clone(&ips);
                            let next = Arc::clone(&next);
                            let completed = Arc::clone(&worker_completed);
                            let token = worker_token.clone();
                            let events = worker_events.clone();
                            scope.spawn(move || loop {
                                if token.is_cancelled() {
                                    break;
                                }
                                let index = next.fetch_add(1, Ordering::Relaxed);
                                let Some(&ip) = ips.get(index) else {
                                    break;
                                };

                                if let Some(mac) = net::resolve_mac_address(ip)
                                    && !token.is_cancelled()
                                {
                                    let hostname = net::resolve_hostname(IpAddr::V4(ip))
                                        .unwrap_or_default();
                                    if !token.is_cancelled() {
                                        let _ = events.blocking_send(RuntimeEvent::ScanHostFound {
                                            job,
                                            host: ScanHost {
                                                ip: ip.to_string(),
                                                vendor: crate::utils::oui::lookup(&mac)
                                                    .unwrap_or("-")
                                                    .to_string(),
                                                mac,
                                                hostname,
                                            },
                                        });
                                    }
                                }
                                completed.fetch_add(1, Ordering::Relaxed);
                            });
                        }
                    });
                });
                tokio::pin!(workers);
                let mut ticker = tokio::time::interval(Duration::from_millis(250));
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {
                            let current = completed.load(Ordering::Relaxed);
                            let _ = events.send(RuntimeEvent::ScanProgress { job, current, total }).await;
                        }
                        result = &mut workers => {
                            if let Err(error) = result {
                                tracing::warn!(%error, "scanner worker pool failed to join");
                            }
                            break;
                        }
                    }
                }
                let current = completed.load(Ordering::Relaxed);
                let _ = events
                    .send(RuntimeEvent::ScanProgress {
                        job,
                        current,
                        total,
                    })
                    .await;
            }

            let terminal_event = if token.is_cancelled() {
                RuntimeEvent::ScanCancelled { job }
            } else {
                RuntimeEvent::ScanFinished { job }
            };
            let _ = events.send(terminal_event).await;
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use iptools_core::{
        Action, AppModel, InputEvent, Message, Page, ScanRequest, TaskStatus, ToolKind,
    };

    use super::*;

    #[tokio::test]
    async fn native_handler_drives_the_shared_scanner_reducer() {
        let mut model = AppModel::default();
        model.page = Page::Scanner;
        model.scanner.cidr = "invalid-cidr".into();
        let [effect] = model
            .update(Message::Input(InputEvent::Action(Action::Toggle)))
            .try_into()
            .expect("scanner should emit one effect");
        let mut runtime = NativeRuntime::new();
        runtime.dispatch(effect).unwrap();

        for _ in 0..64 {
            tokio::task::yield_now().await;
            runtime.reap_finished();
            while let Some(event) = runtime.try_recv() {
                model.update(Message::Runtime(event));
            }
            if model.scanner.job.is_none() {
                break;
            }
        }

        assert_eq!(model.scanner.status, TaskStatus::Done);
        assert_eq!(model.scanner.total, 0);
        assert!(model.scanner.job.is_none());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn replacement_cancels_old_scan_and_finishes_new_generation() {
        let mut runtime = NativeRuntime::new();
        let first = JobId {
            tool: ToolKind::Scanner,
            generation: 1,
        };
        let second = JobId {
            generation: 2,
            ..first
        };
        runtime
            .dispatch(iptools_core::Effect::StartScan {
                job: first,
                request: ScanRequest {
                    cidr: "192.0.2.1/32".into(),
                    concurrency: 1,
                },
            })
            .unwrap();
        runtime
            .dispatch(iptools_core::Effect::StartScan {
                job: second,
                request: ScanRequest {
                    cidr: "invalid-cidr".into(),
                    concurrency: 1,
                },
            })
            .unwrap();

        let events = tokio::time::timeout(Duration::from_secs(5), async {
            let mut events = Vec::new();
            loop {
                runtime.reap_finished();
                while let Some(event) = runtime.try_recv() {
                    events.push(event);
                }
                let first_cancelled = events.iter().any(
                    |event| matches!(event, RuntimeEvent::ScanCancelled { job } if *job == first),
                );
                let second_finished = events.iter().any(
                    |event| matches!(event, RuntimeEvent::ScanFinished { job } if *job == second),
                );
                if first_cancelled && second_finished {
                    return events;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("both scan generations should reach terminal events");

        assert!(
            events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScanCancelled { job } if *job == first))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::ScanFinished { job } if *job == second))
        );
        runtime.shutdown().await;
    }
}
