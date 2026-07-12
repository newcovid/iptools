use serde::{Deserialize, Serialize};

/// Platform-neutral input delivered to the application reducer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputEvent {
    Key(KeyEvent),
    /// A platform key together with the user-configured semantic binding.
    ///
    /// Native needs both values: text editors consume the physical key while
    /// navigation honors the configurable native key map. Web and deterministic
    /// demo adapters can keep using `Key` and the built-in bindings.
    MappedKey {
        key: KeyEvent,
        action: Option<Action>,
    },
    Mouse(MouseEvent),
    Action(Action),
}

impl InputEvent {
    pub const fn key(&self) -> Option<KeyEvent> {
        match self {
            Self::Key(key) | Self::MappedKey { key, .. } => Some(*key),
            Self::Mouse(_) | Self::Action(_) => None,
        }
    }

    pub fn action(&self) -> Option<Action> {
        match self {
            Self::Key(key) => key.action(),
            Self::MappedKey { action, .. } => *action,
            Self::Action(action) => Some(*action),
            Self::Mouse(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl KeyEvent {
    pub const fn plain(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: Modifiers::NONE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyCode {
    Char(char),
    F(u8),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Home,
    End,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        control: false,
        alt: false,
        shift: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseKind {
    Down,
    Up,
    Click,
    Move,
    ScrollUp,
    ScrollDown,
}

/// Semantic actions shared by keyboard, mouse and touch adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    Quit,
    ToggleLanguage,
    NextPage,
    PreviousPage,
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Refresh,
    Edit,
    Toggle,
    Help,
    SelectPage(u8),
    SelectDiagnostic(u8),
    FocusDiagnostic(crate::DiagnosticFocus),
    SelectDiagnosticField(usize, usize),
    SelectDiagnosticHistory(usize),
    ResetDemo,
    History,
    SelectAdapter(usize),
    SelectAdapterField(crate::AdapterField, usize),
    SelectAdapterHistory(usize),
    SelectScannerInput(usize),
    ActivateScannerPanel,
    SelectScannerHistory(usize),
    SelectSetting(usize),
}

impl KeyEvent {
    /// Resolve built-in demo bindings without depending on a terminal library.
    pub fn action(self) -> Option<Action> {
        use KeyCode::*;

        match (self.code, self.modifiers) {
            (Char('c' | 'q'), Modifiers { control: true, .. }) => Some(Action::Quit),
            (Char('l'), Modifiers { control: true, .. }) => Some(Action::ToggleLanguage),
            (Char('r'), Modifiers { control: true, .. }) => Some(Action::History),
            (Tab, Modifiers { shift: true, .. }) | (BackTab, _) => Some(Action::PreviousPage),
            (Tab, _) => Some(Action::NextPage),
            (Up | Char('w'), _) => Some(Action::Up),
            (Down | Char('s'), _) => Some(Action::Down),
            (Left | Char('a'), _) => Some(Action::Left),
            (Right | Char('d'), _) => Some(Action::Right),
            (Enter, _) => Some(Action::Confirm),
            (Esc, _) => Some(Action::Back),
            (Char('r'), Modifiers { control: false, .. }) => Some(Action::Refresh),
            (Char('e'), _) => Some(Action::Edit),
            (Char(' '), _) => Some(Action::Toggle),
            (F(1), _) => Some(Action::Help),
            _ => None,
        }
    }
}
