//! Native Port Scan effect handler.

use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use futures::{StreamExt, stream};
use iptools_core::{JobId, PortScanRequest, RuntimeError, RuntimeErrorCode, RuntimeEvent};

use super::NativeRuntime;

impl NativeRuntime {
    pub(super) fn spawn_port_scan(&mut self, job: JobId, request: PortScanRequest) {
        self.spawn(job, move |token, events| async move {
            let total = u64::from(request.end_port.saturating_sub(request.start_port)) + 1;
            let invalid = request.target.trim().is_empty()
                || request.start_port == 0
                || request.end_port < request.start_port;
            if invalid {
                let _ = events
                    .send(RuntimeEvent::PortScanFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::InvalidRequest,
                            "diag_port_err_target",
                        ),
                    })
                    .await;
                return Ok(());
            }

            let _ = events
                .send(RuntimeEvent::PortScanStarted { job, total })
                .await;
            let target = request.target.trim().to_string();
            let resolved = async {
                match target.parse::<IpAddr>() {
                    Ok(ip) => Some(ip),
                    Err(_) => tokio::net::lookup_host((target.as_str(), 0u16))
                        .await
                        .ok()
                        .and_then(|mut addresses| addresses.next().map(|address| address.ip())),
                }
            };
            let ip = tokio::select! {
                _ = token.cancelled() => {
                    let _ = events.send(RuntimeEvent::PortScanFinished {
                        job,
                        scanned: 0,
                        total,
                        cancelled: true,
                    }).await;
                    return Ok(());
                }
                ip = resolved => ip,
            };
            let Some(ip) = ip else {
                let _ = events
                    .send(RuntimeEvent::PortScanFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::ResolveTarget,
                            "diag_port_err_target",
                        ),
                    })
                    .await;
                return Ok(());
            };

            let ports: Vec<u16> = (request.start_port..=request.end_port).collect();
            let scanned = Arc::new(AtomicU64::new(0));
            let mut scan = stream::iter(ports)
                .map(|port| {
                    let events = events.clone();
                    let scanned = Arc::clone(&scanned);
                    let token = token.clone();
                    async move {
                        if token.is_cancelled() {
                            return;
                        }
                        let address = SocketAddr::new(ip, port);
                        let connect = tokio::net::TcpStream::connect(address);
                        if let Ok(Ok(_stream)) =
                            tokio::time::timeout(Duration::from_millis(request.timeout_ms), connect)
                                .await
                        {
                            let _ = events.send(RuntimeEvent::PortScanOpen { job, port }).await;
                        }
                        scanned.fetch_add(1, Ordering::Relaxed);
                    }
                })
                .buffer_unordered(request.concurrency.clamp(1, 1_024));
            let mut ticker = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = ticker.tick() => {
                        let current = scanned.load(Ordering::Relaxed);
                        let _ = events.send(RuntimeEvent::PortScanProgress {
                            job,
                            scanned: current,
                            total,
                        }).await;
                    }
                    item = scan.next() => if item.is_none() { break },
                }
            }
            let current = scanned.load(Ordering::Relaxed);
            let _ = events
                .send(RuntimeEvent::PortScanFinished {
                    job,
                    scanned: current,
                    total,
                    cancelled: token.is_cancelled(),
                })
                .await;
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use iptools_core::{
        Action, AppModel, DiagnosticTool, Effect, InputEvent, Message, Page, TaskStatus, ToolKind,
    };

    use super::*;

    async fn drive_until_terminal(model: &mut AppModel, runtime: &mut NativeRuntime) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                runtime.reap_finished();
                while let Some(event) = runtime.try_recv() {
                    model.update(Message::Runtime(event));
                }
                if model.diagnostics.port_scan.common.job.is_none() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("port scan should reach a terminal event");
    }

    #[tokio::test]
    async fn native_handler_drives_the_shared_port_scan_reducer() {
        let mut model = AppModel::default();
        model.page = Page::Diagnostics;
        model.diagnostics.tool = DiagnosticTool::PortScan;
        model.diagnostics.port_scan.request = PortScanRequest {
            target: "127.0.0.1".into(),
            start_port: 9,
            end_port: 9,
            timeout_ms: 20,
            concurrency: 1,
        };
        let [effect] = model
            .update(Message::Input(InputEvent::Action(Action::Toggle)))
            .try_into()
            .expect("port scan should emit one effect");
        let mut runtime = NativeRuntime::new();
        runtime.dispatch(effect).unwrap();
        drive_until_terminal(&mut model, &mut runtime).await;

        assert_eq!(model.diagnostics.port_scan.common.status, TaskStatus::Done);
        assert_eq!(model.diagnostics.port_scan.total, 1);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn invalid_request_returns_typed_failure() {
        let mut model = AppModel::default();
        model.page = Page::Diagnostics;
        model.diagnostics.tool = DiagnosticTool::PortScan;
        model.diagnostics.port_scan.request = PortScanRequest {
            target: String::new(),
            start_port: 0,
            end_port: 0,
            timeout_ms: 20,
            concurrency: 1,
        };
        let [effect] = model
            .update(Message::Input(InputEvent::Action(Action::Toggle)))
            .try_into()
            .unwrap();
        let mut runtime = NativeRuntime::new();
        runtime.dispatch(effect).unwrap();
        drive_until_terminal(&mut model, &mut runtime).await;

        assert!(matches!(
            model.diagnostics.port_scan.common.status,
            TaskStatus::Failed(_)
        ));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn replacement_finishes_old_generation_as_cancelled() {
        let mut runtime = NativeRuntime::new();
        let first = JobId {
            tool: ToolKind::PortScan,
            generation: 1,
        };
        let second = JobId {
            generation: 2,
            ..first
        };
        let request = PortScanRequest {
            target: "127.0.0.1".into(),
            start_port: 9,
            end_port: 9,
            timeout_ms: 20,
            concurrency: 1,
        };
        runtime
            .dispatch(Effect::StartPortScan {
                job: first,
                request: request.clone(),
            })
            .unwrap();
        runtime
            .dispatch(Effect::StartPortScan {
                job: second,
                request,
            })
            .unwrap();

        let events =
            tokio::time::timeout(Duration::from_secs(5), async {
                let mut events = Vec::new();
                loop {
                    runtime.reap_finished();
                    while let Some(event) = runtime.try_recv() {
                        events.push(event);
                    }
                    let first_cancelled = events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::PortScanFinished { job, cancelled: true, .. } if *job == first
                ));
                    let second_finished = events.iter().any(|event| {
                        matches!(
                            event,
                            RuntimeEvent::PortScanFinished { job, .. } if *job == second
                        )
                    });
                    if first_cancelled && second_finished {
                        return events;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("both port scan generations should finish");
        assert!(!events.is_empty());
        runtime.shutdown().await;
    }
}
