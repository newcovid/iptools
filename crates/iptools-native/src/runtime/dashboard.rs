use std::{env, time::Instant};

use chrono::Local;
use iptools_core::{
    DashboardInterface, DashboardRequest, DashboardSnapshot, JobId, PublicIpInfo, RuntimeError,
    RuntimeErrorCode, RuntimeEvent,
};
use sysinfo::System;
use tokio_util::sync::CancellationToken;

use super::{NativeRuntime, RuntimeTaskError};
use crate::utils::{net, pubip};

#[derive(Debug)]
pub(super) struct TrafficSample {
    interface: String,
    received: u64,
    transmitted: u64,
    sampled_at: Instant,
}

enum FetchFailure {
    Cancelled,
    Failed(RuntimeError),
}

impl NativeRuntime {
    pub(super) fn spawn_dashboard_refresh(&mut self, job: JobId, request: DashboardRequest) {
        let snapshot = self.collect_dashboard_snapshot();
        self.spawn(job, move |token, events| async move {
            match fetch_public_info(&request, &token).await {
                Ok(info) => {
                    let mut snapshot = snapshot;
                    snapshot.public_info = Some(info);
                    events
                        .send(RuntimeEvent::DashboardRefreshFinished {
                            job,
                            snapshot: Box::new(snapshot),
                        })
                        .await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                }
                Err(FetchFailure::Cancelled) => {
                    events
                        .send(RuntimeEvent::DashboardRefreshCancelled { job })
                        .await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                }
                Err(FetchFailure::Failed(error)) => {
                    events
                        .send(RuntimeEvent::DashboardRefreshFailed {
                            job,
                            snapshot: Box::new(snapshot),
                            error,
                        })
                        .await
                        .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
                }
            }
            Ok(())
        });
    }

    fn collect_dashboard_snapshot(&mut self) -> DashboardSnapshot {
        self.dashboard_networks.refresh(true);
        let mut interfaces = net::get_interfaces();
        interfaces.sort_by_key(|interface| std::cmp::Reverse(score_interface(interface)));
        let active = interfaces.into_iter().next();
        let now = Instant::now();
        let mut download_bps = 0;
        let mut upload_bps = 0;
        let mut total_download = 0;
        let mut total_upload = 0;

        if let Some(interface) = &active
            && let Some((_, data)) = self
                .dashboard_networks
                .iter()
                .find(|(name, _)| **name == interface.name)
        {
            total_download = data.total_received();
            total_upload = data.total_transmitted();
            if let Some(previous) = &self.dashboard_sample
                && previous.interface == interface.name
            {
                let elapsed = now.duration_since(previous.sampled_at).as_secs_f64();
                if elapsed > 0.0 {
                    download_bps =
                        (total_download.saturating_sub(previous.received) as f64 / elapsed) as u64;
                    upload_bps =
                        (total_upload.saturating_sub(previous.transmitted) as f64 / elapsed) as u64;
                }
            }
            self.dashboard_sample = Some(TrafficSample {
                interface: interface.name.clone(),
                received: total_download,
                transmitted: total_upload,
                sampled_at: now,
            });
        } else {
            self.dashboard_sample = None;
        }

        DashboardSnapshot {
            observed_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            hostname: System::host_name().unwrap_or_default(),
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            active_interface: active.map(|interface| DashboardInterface {
                name: interface.name,
                description: interface.description,
                ipv4: interface.ipv4.first().cloned().unwrap_or_default(),
                ssid: interface.ssid,
                is_physical: interface.is_physical,
                dhcp_enabled: interface.dhcp_enabled,
            }),
            proxy: detect_proxy(),
            public_info: None,
            download_bps,
            upload_bps,
            total_download,
            total_upload,
        }
    }
}

async fn fetch_public_info(
    request: &DashboardRequest,
    token: &CancellationToken,
) -> Result<PublicIpInfo, FetchFailure> {
    if request.public_ip.endpoints.is_empty() {
        return Err(FetchFailure::Failed(RuntimeError::new(
            RuntimeErrorCode::InvalidRequest,
            "no public IP endpoints configured",
        )));
    }

    let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8));
    if !request.public_ip.use_system_proxy {
        builder = builder.no_proxy();
    }
    let client = builder.build().map_err(|error| {
        FetchFailure::Failed(RuntimeError::new(
            RuntimeErrorCode::Internal,
            error.to_string(),
        ))
    })?;

    let mut last_error = RuntimeError::new(RuntimeErrorCode::Network, "public IP request failed");
    for endpoint in &request.public_ip.endpoints {
        let response = tokio::select! {
            _ = token.cancelled() => return Err(FetchFailure::Cancelled),
            response = client.get(&endpoint.url).send() => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                last_error = RuntimeError::new(RuntimeErrorCode::Network, error.to_string());
                continue;
            }
        };
        let status = response.status();
        let body = tokio::select! {
            _ = token.cancelled() => return Err(FetchFailure::Cancelled),
            body = response.text() => body,
        };
        match body {
            Ok(body) => {
                if let Some(info) = pubip::parse(&endpoint.kind, &body) {
                    return Ok(info);
                }
                last_error = RuntimeError::new(
                    RuntimeErrorCode::Network,
                    format!("unable to parse {} response ({status})", endpoint.kind),
                );
            }
            Err(error) => {
                last_error = RuntimeError::new(RuntimeErrorCode::Network, error.to_string());
            }
        }
    }
    Err(FetchFailure::Failed(last_error))
}

fn score_interface(interface: &net::InterfaceInfo) -> u8 {
    u8::from(interface.is_up) * 10
        + u8::from(interface.is_physical) * 5
        + u8::from(!interface.ipv4.is_empty()) * 5
        + u8::from(interface.dhcp_enabled)
}

fn detect_proxy() -> Option<String> {
    let from_env = env::var("HTTP_PROXY")
        .or_else(|_| env::var("http_proxy"))
        .or_else(|_| env::var("HTTPS_PROXY"))
        .or_else(|_| env::var("https_proxy"))
        .ok();
    if from_env.is_some() {
        return from_env;
    }

    #[cfg(target_os = "windows")]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};

        let settings = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
            .ok()?;
        let enabled: u32 = settings.get_value("ProxyEnable").unwrap_or(0);
        if enabled == 1 {
            let server: String = settings.get_value("ProxyServer").ok()?;
            if !server.is_empty() {
                return Some(server);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let mode = Command::new("gsettings")
            .args(["get", "org.gnome.system.proxy", "mode"])
            .output()
            .ok()
            .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .unwrap_or_default();
        if mode.contains("manual") {
            let host = Command::new("gsettings")
                .args(["get", "org.gnome.system.proxy.http", "host"])
                .output()
                .ok()
                .map(|output| {
                    String::from_utf8_lossy(&output.stdout)
                        .trim()
                        .trim_matches('\'')
                        .to_string()
                })
                .unwrap_or_default();
            let port = Command::new("gsettings")
                .args(["get", "org.gnome.system.proxy.http", "port"])
                .output()
                .ok()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .unwrap_or_default();
            if !host.is_empty() {
                return Some(format!("{host}:{port}"));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iptools_core::{
        Action, AppModel, Effect, Endpoint, InputEvent, Message, PublicIpConfig, TaskStatus,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    async fn drive_until_terminal(model: &mut AppModel, runtime: &mut NativeRuntime) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                runtime.reap_finished();
                while let Some(event) = runtime.try_recv() {
                    model.update(Message::Runtime(event));
                }
                if model.dashboard.job.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("dashboard refresh should finish");
    }

    #[tokio::test]
    async fn refresh_uses_configured_endpoint_and_updates_shared_model() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\n127.0.0.1",
                )
                .await
                .unwrap();
        });

        let mut model = AppModel::default();
        model.apply_config(&iptools_core::ConfigData {
            public_ip: PublicIpConfig {
                endpoints: vec![Endpoint {
                    url: format!("http://{address}"),
                    kind: "plaintext".into(),
                }],
                use_system_proxy: false,
            },
            ..iptools_core::ConfigData::default()
        });
        let [effect] = model
            .update(Message::Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        assert!(matches!(effect, Effect::RefreshDashboard { .. }));
        let mut runtime = NativeRuntime::new();
        runtime.dispatch(effect).unwrap();
        drive_until_terminal(&mut model, &mut runtime).await;
        server.await.unwrap();

        assert_eq!(model.dashboard.status, TaskStatus::Done);
        assert_eq!(
            model.dashboard.snapshot.public_info.as_ref().unwrap().ip,
            "127.0.0.1"
        );
        assert!(!model.dashboard.snapshot.hostname.is_empty());
    }

    #[tokio::test]
    async fn empty_endpoint_list_is_a_typed_failure() {
        let mut model = AppModel::default();
        model.apply_config(&iptools_core::ConfigData {
            public_ip: PublicIpConfig {
                endpoints: Vec::new(),
                use_system_proxy: false,
            },
            ..iptools_core::ConfigData::default()
        });
        let [effect] = model
            .update(Message::Input(InputEvent::Action(Action::Refresh)))
            .try_into()
            .unwrap();
        let mut runtime = NativeRuntime::new();
        runtime.dispatch(effect).unwrap();
        drive_until_terminal(&mut model, &mut runtime).await;

        assert!(matches!(model.dashboard.status, TaskStatus::Failed(_)));
        assert_eq!(
            model.dashboard.error.as_ref().unwrap().code,
            RuntimeErrorCode::InvalidRequest
        );
    }
}
