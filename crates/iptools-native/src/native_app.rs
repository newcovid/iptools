use anyhow::Result;
use chrono::Local;
use iptools_core::{Action, AppModel, Effect, InputEvent, Message};
use iptools_ui::UiState;
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

use crate::{
    config::Config,
    event::{Event, EventHandler},
    frontend,
    runtime::NativeRuntime,
};

const TICK_MS: u64 = 250;
const TRAFFIC_REFRESH_TICKS: u64 = 4;
const ADAPTER_REFRESH_TICKS: u64 = 8;

pub async fn run(config_path: Option<String>) -> Result<()> {
    let mut config = Config::load(config_path.as_deref());
    let keymap = config.keymap();
    let mut model = AppModel::default();
    model.demo = false;
    model.apply_config(&config);
    let mut runtime = NativeRuntime::new();
    dispatch_effects(&mut runtime, &mut config, model.bootstrap_effects())?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut events = EventHandler::new(TICK_MS);
    let mut ui = UiState::default();
    frontend::enter(&mut terminal)?;

    let mut ticks = 0_u64;
    let run_result = async {
        while model.running {
            terminal.draw(|frame| iptools_ui::render(frame, &model, &mut ui))?;
            let mut effects = Vec::new();
            match events.next().await? {
                Event::Tick => {
                    ticks = ticks.saturating_add(1);
                    runtime.reap_finished();
                    while let Some(event) = runtime.try_recv() {
                        effects.extend(model.update(Message::Runtime(event)));
                    }
                    effects.extend(model.update(Message::Tick(TICK_MS)));
                    if ticks.is_multiple_of(TRAFFIC_REFRESH_TICKS) {
                        effects.extend(model.update(Message::Clock(
                            Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                        )));
                        effects.extend(model.refresh_traffic());
                    }
                    if ticks.is_multiple_of(ADAPTER_REFRESH_TICKS) && model.adapters.edit.is_none()
                    {
                        effects.extend(model.refresh_adapters());
                    }
                }
                Event::Key(key) => {
                    if let Some(input) = frontend::mapped_key(key, &keymap) {
                        effects.extend(model.update(Message::Input(input)));
                    }
                }
                Event::Mouse(mouse) => {
                    let action = match mouse.kind {
                        crossterm::event::MouseEventKind::ScrollUp => Some(Action::Up),
                        crossterm::event::MouseEventKind::ScrollDown => Some(Action::Down),
                        crossterm::event::MouseEventKind::Down(
                            crossterm::event::MouseButton::Left,
                        ) => ui.hit_test(mouse.column, mouse.row),
                        _ => None,
                    };
                    if let Some(action) = action {
                        effects.extend(model.update(Message::Input(InputEvent::Action(action))));
                    }
                }
                Event::Resize => {}
            }
            dispatch_effects(&mut runtime, &mut config, effects)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    // Restore the user's terminal immediately after Quit. Scanner workers may
    // still be finishing their current short, blocking OS probe; they no
    // longer keep the alternate screen visible while structured shutdown
    // joins them.
    let exit_result = frontend::exit(&mut terminal);
    events.shutdown().await;
    runtime.shutdown().await;
    run_result?;
    exit_result?;
    Ok(())
}

fn dispatch_effects(
    runtime: &mut NativeRuntime,
    config: &mut Config,
    effects: Vec<Effect>,
) -> Result<()> {
    for effect in effects {
        if frontend::persist_effect(config, &effect) {
            continue;
        }
        runtime.dispatch(effect)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptools_core::{Language, Preferences};

    #[test]
    fn native_dispatch_persists_without_sending_storage_effects_to_runtime() {
        let path = std::env::temp_dir().join(format!(
            "iptools-native-runner-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut config = Config::load(Some(path.to_str().unwrap()));
        let mut runtime = NativeRuntime::new();
        dispatch_effects(
            &mut runtime,
            &mut config,
            vec![Effect::PersistPreferences(Preferences {
                language: Language::Zh,
                theme: iptools_core::ThemeId::Dracula,
                scan_concurrency: 90,
            })],
        )
        .unwrap();
        let saved: iptools_core::ConfigData =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved.language, Language::Zh);
        assert_eq!(saved.theme, iptools_core::ThemeId::Dracula);
        assert_eq!(saved.scan_concurrency, 90);
        std::fs::remove_file(path).unwrap();
    }
}
