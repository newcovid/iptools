use anyhow::Result;
use crossterm::{
    event::{KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use iptools_core::{Action, AppModel, Effect, InputEvent, KeyCode, KeyEvent, Message, Modifiers};
use iptools_demo::{DemoRuntime, ScenarioId};
use iptools_ui::UiState;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use crate::config::Config;
use crate::event::{Event, EventHandler};

pub async fn run(scenario: ScenarioId, config_path: Option<String>) -> Result<()> {
    let mut config = Config::load(config_path.as_deref());
    let mut model = AppModel::default();
    model.apply_config(&config);
    let mut runtime = DemoRuntime::new(scenario)?;
    for event in runtime.bootstrap() {
        model.update(Message::Runtime(event));
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut events = EventHandler::new(250);
    let mut ui = UiState::default();
    enter(&mut terminal)?;

    while model.running {
        terminal.draw(|frame| iptools_ui::render(frame, &model, &mut ui))?;
        let effects = match events.next().await? {
            Event::Tick => {
                let mut effects = model.update(Message::Tick(250));
                for event in runtime.advance(250) {
                    effects.extend(model.update(Message::Runtime(event)));
                }
                effects
            }
            Event::Key(key) => model.update(Message::Input(InputEvent::Key(convert_key(key)))),
            Event::Mouse(mouse) => {
                let action = match mouse.kind {
                    crossterm::event::MouseEventKind::ScrollUp => Some(Action::Up),
                    crossterm::event::MouseEventKind::ScrollDown => Some(Action::Down),
                    crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                        ui.hit_test(mouse.column, mouse.row)
                    }
                    _ => None,
                };
                action.map_or_else(Vec::new, |action| {
                    model.update(Message::Input(InputEvent::Action(action)))
                })
            }
            Event::Resize => Vec::new(),
        };
        dispatch_effects(&mut model, &mut runtime, &mut config, effects);
    }

    events.shutdown().await;
    exit(&mut terminal)?;
    Ok(())
}

fn dispatch_effects(
    model: &mut AppModel,
    runtime: &mut DemoRuntime,
    config: &mut Config,
    effects: Vec<Effect>,
) {
    for effect in effects {
        match effect {
            Effect::PersistPreferences(preferences) => {
                config.language = preferences.language;
                config.scan_concurrency = preferences.scan_concurrency;
                config.save();
            }
            Effect::PersistSession(update) => {
                match update {
                    iptools_core::SessionUpdate::Ping(value) => config.session.ping = value,
                    iptools_core::SessionUpdate::Trace(value) => config.session.trace = value,
                    iptools_core::SessionUpdate::PortScan(value) => {
                        config.session.port_scan = value
                    }
                    iptools_core::SessionUpdate::LanSpeed(value) => {
                        config.session.lan_speed = value
                    }
                    iptools_core::SessionUpdate::LinkQuality(value) => {
                        config.session.link_quality = value
                    }
                    iptools_core::SessionUpdate::TargetHistory(value) => {
                        config.session.history.targets = value;
                    }
                    iptools_core::SessionUpdate::Ui(value) => config.session.ui = value,
                    iptools_core::SessionUpdate::Reset(ui) => {
                        config.session = iptools_core::SessionState {
                            ui,
                            ..iptools_core::SessionState::default()
                        };
                    }
                }
                config.save();
            }
            Effect::PersistAdapterEdit {
                guid,
                params,
                history,
            } => {
                config.session.adapter_edit.adapters.insert(guid, params);
                config.session.history.adapter = history;
                config.save();
            }
            effect => {
                for event in runtime.dispatch(effect) {
                    model.update(Message::Runtime(event));
                }
            }
        }
    }
}

fn convert_key(event: CrosstermKeyEvent) -> KeyEvent {
    let code = match event.code {
        CrosstermKeyCode::Char(value) => KeyCode::Char(value),
        CrosstermKeyCode::F(value) => KeyCode::F(value),
        CrosstermKeyCode::Enter => KeyCode::Enter,
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::Tab => KeyCode::Tab,
        CrosstermKeyCode::BackTab => KeyCode::BackTab,
        CrosstermKeyCode::Backspace => KeyCode::Backspace,
        CrosstermKeyCode::Delete => KeyCode::Delete,
        CrosstermKeyCode::Home => KeyCode::Home,
        CrosstermKeyCode::End => KeyCode::End,
        CrosstermKeyCode::Up => KeyCode::Up,
        CrosstermKeyCode::Down => KeyCode::Down,
        CrosstermKeyCode::Left => KeyCode::Left,
        CrosstermKeyCode::Right => KeyCode::Right,
        _ => return KeyEvent::plain(KeyCode::Esc),
    };
    KeyEvent {
        code,
        modifiers: Modifiers {
            control: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
        },
    }
}

fn enter<B>(terminal: &mut Terminal<B>) -> Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(())
}

fn exit<B>(terminal: &mut Terminal<B>) -> Result<()>
where
    B: ratatui::backend::Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    terminal.show_cursor()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    disable_raw_mode()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use iptools_core::{AdapterEditParams, Language, Preferences};

    use super::*;

    #[test]
    fn native_demo_persists_shared_preference_effects() {
        let path = std::env::temp_dir().join(format!(
            "iptools-demo-settings-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = Config::load(Some(path.to_str().unwrap()));
        let mut model = AppModel::default();
        let mut runtime = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        dispatch_effects(
            &mut model,
            &mut runtime,
            &mut config,
            vec![Effect::PersistPreferences(Preferences {
                language: Language::Zh,
                scan_concurrency: 120,
            })],
        );

        let saved: iptools_core::ConfigData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.language, Language::Zh);
        assert_eq!(saved.scan_concurrency, 120);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn native_demo_persists_guid_scoped_adapter_form_without_real_network_io() {
        let path = std::env::temp_dir().join(format!(
            "iptools-demo-adapter-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = Config::load(Some(path.to_str().unwrap()));
        let mut model = AppModel::default();
        let mut runtime = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let params = AdapterEditParams {
            use_dhcp: false,
            ip: "10.0.0.8".into(),
            mask: "255.255.255.0".into(),
            gateway: "10.0.0.1".into(),
            dns1: "1.1.1.1".into(),
            dns2: String::new(),
        };
        dispatch_effects(
            &mut model,
            &mut runtime,
            &mut config,
            vec![Effect::PersistAdapterEdit {
                guid: "demo-ethernet".into(),
                params: params.clone(),
                history: vec!["10.0.0.8".into()],
            }],
        );
        let saved: iptools_core::ConfigData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved.session.adapter_edit.adapters.get("demo-ethernet"),
            Some(&params)
        );
        assert_eq!(saved.session.history.adapter, ["10.0.0.8"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn native_demo_persists_shared_diagnostic_sessions_and_target_history() {
        let path = std::env::temp_dir().join(format!(
            "iptools-demo-diagnostics-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = Config::load(Some(path.to_str().unwrap()));
        let mut model = AppModel::default();
        let mut runtime = DemoRuntime::new(ScenarioId::HomeNetwork).unwrap();
        let ping = iptools_core::PingPersist {
            target: "ping.example".into(),
            interval_ms: 500,
            timeout_ms: 900,
            packet_size: 64,
        };
        let trace = iptools_core::TracePersist {
            target: "trace.example".into(),
            max_hops: "12".into(),
            timeout_ms: "800".into(),
        };
        let port_scan = iptools_core::PortScanPersist {
            target: "ports.example".into(),
            start_port: "20".into(),
            end_port: "443".into(),
            timeout_ms: "250".into(),
        };
        let lan_speed = iptools_core::LanSpeedPersist {
            mode: "client".into(),
            peer: "lan.example".into(),
            port: "50505".into(),
            proto: "udp".into(),
            direction: "bidir".into(),
            duration: "10".into(),
            streams: "2".into(),
            payload: "1400".into(),
            rate: "100".into(),
        };
        let link = iptools_core::LinkQualityPersist {
            adapters: [(
                "wifi-guid".into(),
                iptools_core::LinkParams {
                    target: "link.example".into(),
                    ..iptools_core::LinkParams::default()
                },
            )]
            .into_iter()
            .collect(),
            selected: Some("wifi-guid".into()),
        };
        dispatch_effects(
            &mut model,
            &mut runtime,
            &mut config,
            vec![
                Effect::PersistSession(iptools_core::SessionUpdate::Ping(ping.clone())),
                Effect::PersistSession(iptools_core::SessionUpdate::Trace(trace.clone())),
                Effect::PersistSession(iptools_core::SessionUpdate::PortScan(port_scan.clone())),
                Effect::PersistSession(iptools_core::SessionUpdate::LanSpeed(lan_speed.clone())),
                Effect::PersistSession(iptools_core::SessionUpdate::LinkQuality(link.clone())),
                Effect::PersistSession(iptools_core::SessionUpdate::TargetHistory(vec![
                    "trace.example".into(),
                    "ping.example".into(),
                ])),
            ],
        );
        let saved: iptools_core::ConfigData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.session.ping, ping);
        assert_eq!(saved.session.trace, trace);
        assert_eq!(saved.session.port_scan, port_scan);
        assert_eq!(saved.session.lan_speed, lan_speed);
        assert_eq!(saved.session.link_quality, link);
        assert_eq!(
            saved.session.history.targets,
            ["trace.example", "ping.example"]
        );
        let keep_ui = iptools_core::UiPersist {
            last_tab: iptools_core::Page::Settings as u8,
            last_diag_tool: 3,
        };
        dispatch_effects(
            &mut model,
            &mut runtime,
            &mut config,
            vec![Effect::PersistSession(iptools_core::SessionUpdate::Reset(
                keep_ui.clone(),
            ))],
        );
        let reset: iptools_core::ConfigData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reset.session.ui, keep_ui);
        assert_eq!(reset.session.ping, iptools_core::PingPersist::default());
        assert_eq!(
            reset.session.port_scan,
            iptools_core::PortScanPersist::default()
        );
        assert_eq!(
            reset.session.lan_speed,
            iptools_core::LanSpeedPersist::default()
        );
        assert!(reset.session.history.targets.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
