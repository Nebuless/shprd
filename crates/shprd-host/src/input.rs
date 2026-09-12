//! Browser VT input to frozen Herdr semantic input events.

use bincode::Encode;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("input exceeds one MiB")]
    Limit,
    #[error("invalid input UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("input encoding: {0}")]
    Encode(#[from] bincode::error::EncodeError),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode)]
pub enum Key {
    Backspace,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Tab,
    BackTab,
    Delete,
    Insert,
    Esc,
    Char(u32),
    F(u8),
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key {
        code: Key,
        modifiers: u8,
    },
    Text(String),
    Paste(String),
    Mouse {
        kind: u32,
        button: Option<u32>,
        column: u16,
        row: u16,
        modifiers: u8,
        lines: u16,
    },
}

pub fn encode(pane: &str, events: &[Event]) -> Result<Vec<u8>, Error> {
    let config = bincode::config::standard();
    let mut bytes = bincode::encode_to_vec((13_u32, pane, events.len()), config)?;
    for event in events {
        let encoded = match event {
            Event::Key { code, modifiers } => bincode::encode_to_vec(
                (
                    0_u32,
                    code,
                    modifiers,
                    0_u32,
                    1_u16,
                    None::<u32>,
                    None::<String>,
                    false,
                    None::<u32>,
                    None::<u32>,
                ),
                config,
            )?,
            Event::Text(text) => bincode::encode_to_vec((1_u32, text), config)?,
            Event::Paste(text) => bincode::encode_to_vec((3_u32, text), config)?,
            Event::Mouse {
                kind,
                button,
                column,
                row,
                modifiers,
                lines,
            } => {
                let mut mouse = bincode::encode_to_vec((2_u32, kind), config)?;
                if let Some(button) = button {
                    mouse.extend(bincode::encode_to_vec(button, config)?);
                }
                mouse.extend(bincode::encode_to_vec(
                    (0_u32, column, row, None::<u32>, modifiers, lines),
                    config,
                )?);
                mouse
            }
        };
        bytes.extend(encoded);
    }
    Ok(bytes)
}

mod classifier;
pub use classifier::Classifier;
