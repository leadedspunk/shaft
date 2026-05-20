use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    SwitchPane,
    MoveUp,
    MoveDown,
    Enter,
    GoUp,
    GoHome,
    ToggleSelect,
    Copy,
    Move,
    MkDir,
    Delete,
    Rename,
    ToggleHidden,
    Confirm,
    Cancel,
    None,
}

pub fn map_key(event: KeyEvent) -> Action {
    match (event.modifiers, event.code) {
        (KeyModifiers::NONE, KeyCode::Char('q')) => Action::Quit,
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Action::Quit,

        (KeyModifiers::NONE, KeyCode::Tab) => Action::SwitchPane,

        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            Action::MoveUp
        }
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            Action::MoveDown
        }

        (KeyModifiers::NONE, KeyCode::Enter) => Action::Enter,

        (KeyModifiers::NONE, KeyCode::Backspace)
        | (KeyModifiers::NONE, KeyCode::Char('h')) => Action::GoUp,

        (KeyModifiers::NONE, KeyCode::Char('~')) => Action::GoHome,

        (KeyModifiers::NONE, KeyCode::Char(' ')) => Action::ToggleSelect,

        (KeyModifiers::NONE, KeyCode::F(2)) => Action::Rename,
        (KeyModifiers::NONE, KeyCode::F(5)) => Action::Copy,
        (KeyModifiers::NONE, KeyCode::F(6)) => Action::Move,
        (KeyModifiers::NONE, KeyCode::F(7)) => Action::MkDir,
        (KeyModifiers::NONE, KeyCode::F(8)) | (KeyModifiers::NONE, KeyCode::Delete) => {
            Action::Delete
        }
        (KeyModifiers::NONE, KeyCode::Char('.')) => Action::ToggleHidden,

        (KeyModifiers::NONE, KeyCode::Esc) => Action::Cancel,

        (KeyModifiers::NONE, KeyCode::Char('y')) => Action::Confirm,

        _ => Action::None,
    }
}

pub struct KeyLegend;

impl KeyLegend {
    pub fn entries() -> Vec<(&'static str, &'static str)> {
        vec![
            ("Tab", "Switch"),
            ("Enter", "Open"),
            ("h/Bsp", "Up"),
            ("~", "Home"),
            ("Space", "Sel"),
            ("F2", "Rename"),
            ("F5", "Copy"),
            ("F6", "Move"),
            ("F7", "MkDir"),
            ("F8", "Del"),
            (".", "Hidden"),
            ("q", "Quit"),
        ]
    }
}
