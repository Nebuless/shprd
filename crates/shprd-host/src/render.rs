//! Frozen Herdr wire layouts and ANSI repaint conversion.

use bincode::{Decode, Encode};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported Herdr protocol {0}")]
    Protocol(u32),
    #[error("wire encoding: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("invalid frame dimensions")]
    Dimensions,
}

#[derive(Debug, Clone, Decode, Encode, PartialEq, Eq)]
pub struct Cell {
    pub symbol: String,
    pub fg: u32,
    pub bg: u32,
    pub modifier: u16,
    pub skip: bool,
    pub hyperlink: Option<u32>,
}

impl Cell {
    pub fn plain(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_owned(),
            fg: 0,
            bg: 0,
            modifier: 0,
            skip: false,
            hyperlink: None,
        }
    }
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
    pub shape: u8,
}

#[derive(Debug, Clone, Decode, Encode)]
pub struct Frame {
    pub cells: Vec<Cell>,
    pub width: u16,
    pub height: u16,
    pub cursor: Option<Cursor>,
    pub hyperlinks: Vec<String>,
    pub graphics: Vec<u8>,
}

pub fn hello(protocol: u32, cols: u16, rows: u16) -> Result<Vec<u8>, Error> {
    if !((14..=20).contains(&protocol) || protocol == 22) {
        return Err(Error::Protocol(protocol));
    }
    let config = bincode::config::standard();
    if protocol == 22 {
        return Ok(bincode::encode_to_vec(
            (0_u32, protocol, cols, rows, 0_u16, 0_u16, false),
            config,
        )?);
    }
    Ok(bincode::encode_to_vec(
        (
            0_u32,
            protocol,
            cols,
            rows,
            0_u16,
            0_u16,
            1_u32,
            0_u32,
            if protocol >= 20 { 2_u32 } else { 1_u32 },
        ),
        config,
    )?)
}

fn color(packed: u32, foreground: bool) -> String {
    let kind = packed >> 24;
    let value = packed & 0x00ff_ffff;
    let base = if foreground { 30 } else { 40 };
    match kind {
        0 if value == 0 => (base + 9).to_string(),
        0 if value <= 8 => (base + value - 1).to_string(),
        0 => (base + 60 + value - 9).to_string(),
        1 => format!("{};5;{}", base + 8, value & 255),
        _ => format!(
            "{};2;{};{};{}",
            base + 8,
            (value >> 16) & 255,
            (value >> 8) & 255,
            value & 255
        ),
    }
}

fn style(cell: &Cell) -> String {
    let mut codes = Vec::new();
    for (flag, code) in [(1, "1"), (2, "2"), (4, "3")] {
        if cell.modifier & flag != 0 {
            codes.push(code.to_owned());
        }
    }
    if cell.modifier & 8 != 0 {
        let underline = cell.modifier >> 12;
        codes.push(if (1..=5).contains(&underline) {
            format!("4:{underline}")
        } else {
            "4".to_owned()
        });
    }
    for (flag, code) in [(0x30, "5"), (0x40, "7"), (0x80, "8"), (0x100, "9")] {
        if cell.modifier & flag != 0 {
            codes.push(code.to_owned());
        }
    }
    codes.push(color(cell.fg, true));
    codes.push(color(cell.bg, false));
    format!("\x1b[0m\x1b[{}m", codes.join(";"))
}

/// Full repaint, clipped to source viewport without wrapping wide cells.
pub fn frame_to_ansi(frame: &Frame, cols: u16, rows: u16) -> Result<String, Error> {
    if frame.cells.len() != usize::from(frame.width) * usize::from(frame.height) {
        return Err(Error::Dimensions);
    }
    let width = usize::from(frame.width.min(cols));
    let height = usize::from(frame.height.min(rows));
    let mut output = String::from("\x1b[0m\x1b[H\x1b[2J\x1b[?7l");
    for y in 0..height {
        if y > 0 {
            output.push_str(&format!("\x1b[{};1H", y + 1));
        }
        let start = y * usize::from(frame.width);
        let mut end = width;
        while end > 0 && frame.cells[start + end - 1] == Cell::plain(" ") {
            end -= 1;
        }
        let mut x = 0;
        let mut last: Option<&Cell> = None;
        let mut linked = false;
        while x < end {
            let cell = &frame.cells[start + x];
            if cell.skip {
                x += 1;
                continue;
            }
            let cell_width = cell.symbol.width();
            if x + cell_width > width {
                break;
            }
            if last.is_none_or(|previous| {
                previous.fg != cell.fg
                    || previous.bg != cell.bg
                    || previous.modifier != cell.modifier
                    || previous.hyperlink != cell.hyperlink
            }) {
                if linked {
                    output.push_str("\x1b]8;;\x1b\\");
                    linked = false;
                }
                output.push_str(&style(cell));
                if let Some(uri) = cell
                    .hyperlink
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| frame.hyperlinks.get(i))
                {
                    output.push_str(&format!("\x1b]8;;{uri}\x1b\\"));
                    linked = true;
                }
                last = Some(cell);
            }
            output.push_str(&cell.symbol);
            x += cell_width.max(1);
            if cell_width > 1 && x < end {
                output.push_str(&format!("\x1b[{}G", x + 1));
            }
        }
        if linked {
            output.push_str("\x1b]8;;\x1b\\");
        }
    }
    output.push_str("\x1b[?7h");
    match &frame.cursor {
        Some(cursor)
            if cursor.visible
                && usize::from(cursor.x) < width
                && usize::from(cursor.y) < height =>
        {
            output.push_str(&format!(
                "\x1b[0m\x1b[{};{}H\x1b[{} q\x1b[?25h",
                u32::from(cursor.y) + 1,
                u32::from(cursor.x) + 1,
                cursor.shape
            ))
        }
        _ => output.push_str("\x1b[?25l"),
    }
    Ok(output)
}
