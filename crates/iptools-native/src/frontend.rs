use anyhow::Result;
use crossterm::{
    event::{KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use iptools_core::{Action, Effect, InputEvent, KeyCode, KeyEvent, Modifiers};
use ratatui::Terminal;
use std::io;

use crate::{
    config::Config,
    keymap::{Action as NativeAction, KeyMap},
};

pub(crate) fn mapped_key(event: CrosstermKeyEvent, keymap: &KeyMap) -> Option<InputEvent> {
    Some(InputEvent::MappedKey {
        key: convert_key(event)?,
        action: keymap.action_for(event).map(convert_action),
    })
}

pub(crate) fn plain_key(event: CrosstermKeyEvent) -> Option<InputEvent> {
    convert_key(event).map(InputEvent::Key)
}

fn convert_action(action: NativeAction) -> Action {
    match action {
        NativeAction::Quit => Action::Quit,
        NativeAction::ToggleLanguage => Action::ToggleLanguage,
        NativeAction::NextTab => Action::NextPage,
        NativeAction::PrevTab => Action::PreviousPage,
        NativeAction::Up => Action::Up,
        NativeAction::Down => Action::Down,
        NativeAction::Left => Action::Left,
        NativeAction::Right => Action::Right,
        NativeAction::Confirm => Action::Confirm,
        NativeAction::Back => Action::Back,
        NativeAction::Refresh => Action::Refresh,
        NativeAction::History => Action::History,
        NativeAction::Edit => Action::Edit,
        NativeAction::Toggle => Action::Toggle,
        NativeAction::Help => Action::Help,
    }
}

fn convert_key(event: CrosstermKeyEvent) -> Option<KeyEvent> {
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
        _ => return None,
    };
    Some(KeyEvent {
        code,
        modifiers: Modifiers {
            control: event.modifiers.contains(KeyModifiers::CONTROL),
            alt: event.modifiers.contains(KeyModifiers::ALT),
            shift: event.modifiers.contains(KeyModifiers::SHIFT),
        },
    })
}

pub(crate) fn persist_effect(config: &mut Config, effect: &Effect) -> bool {
    match effect {
        Effect::PersistPreferences(preferences) => {
            config.language = preferences.language;
            config.scan_concurrency = preferences.scan_concurrency;
        }
        Effect::PersistSession(update) => match update {
            iptools_core::SessionUpdate::Ping(value) => config.session.ping = value.clone(),
            iptools_core::SessionUpdate::Trace(value) => config.session.trace = value.clone(),
            iptools_core::SessionUpdate::PortScan(value) => {
                config.session.port_scan = value.clone();
            }
            iptools_core::SessionUpdate::LanSpeed(value) => {
                config.session.lan_speed = value.clone();
            }
            iptools_core::SessionUpdate::LinkQuality(value) => {
                config.session.link_quality = value.clone();
            }
            iptools_core::SessionUpdate::TargetHistory(value) => {
                config.session.history.targets = value.clone();
            }
            iptools_core::SessionUpdate::Ui(value) => config.session.ui = value.clone(),
            iptools_core::SessionUpdate::Reset(ui) => {
                config.session = iptools_core::SessionState {
                    ui: ui.clone(),
                    ..iptools_core::SessionState::default()
                };
            }
        },
        Effect::PersistAdapterEdit {
            guid,
            params,
            history,
        } => {
            config
                .session
                .adapter_edit
                .adapters
                .insert(guid.clone(), params.clone());
            config.session.history.adapter = history.clone();
        }
        _ => return false,
    }
    config.save();
    true
}

pub(crate) fn enter<B>(terminal: &mut Terminal<B>) -> Result<()>
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

pub(crate) fn exit<B>(terminal: &mut Terminal<B>) -> Result<()>
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
    use crossterm::event::{KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent};

    use super::*;

    #[test]
    fn mapped_keys_keep_physical_text_and_custom_semantic_action() {
        let mut persisted = crate::keymap::PersistedKeymap::new();
        persisted.insert("down".into(), vec!["j".into()]);
        let map = KeyMap::from_persisted(&persisted);
        let input = mapped_key(
            CrosstermKeyEvent::new(CrosstermKeyCode::Char('j'), KeyModifiers::NONE),
            &map,
        )
        .unwrap();
        assert_eq!(input.key().map(|key| key.code), Some(KeyCode::Char('j')));
        assert_eq!(input.action(), Some(Action::Down));
    }
}
