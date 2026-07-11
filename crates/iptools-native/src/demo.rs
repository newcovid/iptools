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

use crate::event::{Event, EventHandler};

pub async fn run(scenario: ScenarioId) -> Result<()> {
    let mut model = AppModel::default();
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
        dispatch_effects(&mut model, &mut runtime, effects);
    }

    events.shutdown().await;
    exit(&mut terminal)?;
    Ok(())
}

fn dispatch_effects(model: &mut AppModel, runtime: &mut DemoRuntime, effects: Vec<Effect>) {
    for effect in effects {
        for event in runtime.dispatch(effect) {
            model.update(Message::Runtime(event));
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
