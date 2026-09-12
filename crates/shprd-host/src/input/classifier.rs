//! Incremental browser VT parser, retaining incomplete escape and UTF-8 chunks.

use super::{Error, Event, Key};

#[derive(Default)]
pub struct Classifier {
    pending: Vec<u8>,
}

impl Classifier {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Event>, Error> {
        if self.pending.len().saturating_add(bytes.len()) > 1024 * 1024 {
            return Err(Error::Limit);
        }
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut offset = 0;
        while offset < self.pending.len() {
            let buf = &self.pending[offset..];
            if buf.starts_with(b"\x1b[200~") {
                let Some(end) = buf[6..].windows(6).position(|part| part == b"\x1b[201~") else {
                    break;
                };
                events.push(Event::Paste(
                    std::str::from_utf8(&buf[6..6 + end])?.to_owned(),
                ));
                offset += 12 + end;
                continue;
            }
            if buf[0] == 27 {
                let Some((event, length)) = escape(buf)? else {
                    break;
                };
                if let Some(event) = event {
                    events.push(event);
                }
                offset += length;
                continue;
            }
            if let Some(code) = control(buf[0]) {
                events.push(code);
                offset += 1;
                continue;
            }
            let Some((ch, length)) = character(buf)? else {
                break;
            };
            match events.last_mut() {
                Some(Event::Text(text)) => text.push(ch),
                _ => events.push(Event::Text(ch.to_string())),
            }
            offset += length;
        }
        self.pending.drain(..offset);
        Ok(events)
    }

    pub fn flush(&mut self) -> Vec<Event> {
        if self.pending == [27] {
            self.pending.clear();
            vec![key(Key::Esc, 0)]
        } else {
            Vec::new()
        }
    }
}

fn key(code: Key, modifiers: u8) -> Event {
    Event::Key { code, modifiers }
}

fn control(byte: u8) -> Option<Event> {
    Some(match byte {
        13 => key(Key::Enter, 0),
        9 => key(Key::Tab, 0),
        127 => key(Key::Backspace, 0),
        1..=26 => key(Key::Char(u32::from(byte) + 96), 2),
        0 => key(Key::Char(32), 2),
        28..=31 => key(Key::Char(u32::from(byte) + 64), 2),
        _ => return None,
    })
}

fn character(bytes: &[u8]) -> Result<Option<(char, usize)>, Error> {
    let length = match bytes[0] {
        0..=127 => 1,
        194..=223 => 2,
        224..=239 => 3,
        240..=244 => 4,
        _ => 1,
    };
    if bytes.len() < length {
        return Ok(None);
    }
    Ok(std::str::from_utf8(&bytes[..length])?
        .chars()
        .next()
        .map(|ch| (ch, length)))
}

fn modifiers(param: u32) -> u8 {
    let bits = param.saturating_sub(1);
    u8::from(bits & 1 != 0)
        | (u8::from(bits & 2 != 0) * 4)
        | (u8::from(bits & 4 != 0) * 2)
        | (u8::from(bits & 8 != 0) * 8)
}

fn final_key(byte: u8) -> Option<Key> {
    Some(match byte {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'Z' => Key::BackTab,
        b'P'..=b'S' => Key::F(byte - b'P' + 1),
        _ => return None,
    })
}

fn escape(buf: &[u8]) -> Result<Option<(Option<Event>, usize)>, Error> {
    if buf.len() < 2 {
        return Ok(None);
    }
    match buf[1] {
        b'[' => {
            let Some(end) = buf[2..]
                .iter()
                .position(|byte| (64..=126).contains(byte) || *byte == 27)
                .map(|n| n + 2)
            else {
                return Ok(None);
            };
            if buf[end] == 27 {
                return Ok(Some((None, end)));
            }
            let text = std::str::from_utf8(&buf[2..end])?;
            if text.starts_with('<') {
                return Ok(Some((mouse(text, buf[end]), end + 1)));
            }
            let params: Option<Vec<u32>> = text
                .split(';')
                .map(|s| {
                    if s.is_empty() {
                        Some(1)
                    } else {
                        s.parse().ok()
                    }
                })
                .collect();
            let Some(params) = params else {
                return Ok(Some((None, end + 1)));
            };
            let first = params.first().copied().unwrap_or(1);
            let modifier = params.get(1).copied().unwrap_or(1);
            let code = match buf[end] {
                b'~' => match first {
                    1 | 7 => Some(Key::Home),
                    2 => Some(Key::Insert),
                    3 => Some(Key::Delete),
                    4 | 8 => Some(Key::End),
                    5 => Some(Key::PageUp),
                    6 => Some(Key::PageDown),
                    11..=15 => u8::try_from(first - 10).ok().map(Key::F),
                    17..=21 => u8::try_from(first - 11).ok().map(Key::F),
                    23..=24 => u8::try_from(first - 12).ok().map(Key::F),
                    _ => None,
                },
                b'u' if first == 13 && params.len() <= 2 && (1..=16).contains(&modifier) => {
                    Some(Key::Enter)
                }
                b'P'..=b'S' if first != 1 => None,
                final_byte => final_key(final_byte),
            };
            Ok(Some((
                code.map(|code| key(code, modifiers(modifier))),
                end + 1,
            )))
        }
        b'O' => {
            if buf.len() < 3 {
                Ok(None)
            } else {
                Ok(Some((final_key(buf[2]).map(|code| key(code, 0)), 3)))
            }
        }
        27 => Ok(Some((Some(key(Key::Esc, 0)), 1))),
        second => {
            if let Some(Event::Key { code, modifiers }) = control(second) {
                return Ok(Some((Some(key(code, modifiers | 4)), 2)));
            }
            let Some((ch, length)) = character(&buf[1..])? else {
                return Ok(None);
            };
            Ok(Some((Some(key(Key::Char(u32::from(ch)), 4)), 1 + length)))
        }
    }
}

fn mouse(params: &str, final_byte: u8) -> Option<Event> {
    if !matches!(final_byte, b'M' | b'm') {
        return None;
    }
    let fields: Vec<&str> = params.strip_prefix('<')?.split(';').collect();
    if fields.len() != 3
        || fields
            .iter()
            .any(|value| value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let code: u8 = fields[0].parse().ok()?;
    if code > 127 {
        return None;
    }
    let column = u16::try_from(fields[1].parse::<u32>().ok()?.checked_sub(1)?).ok()?;
    let row = u16::try_from(fields[2].parse::<u32>().ok()?.checked_sub(1)?).ok()?;
    let button = code & 3;
    let motion = code & 32 != 0;
    let wheel = code & 64 != 0;
    if (wheel && (motion || final_byte == b'm'))
        || (!wheel && ((motion && final_byte == b'm') || (!motion && button == 3)))
    {
        return None;
    }
    let kind = if wheel {
        4 + u32::from(button)
    } else if motion {
        if button == 3 { 3 } else { 2 }
    } else if final_byte == b'm' {
        1
    } else {
        0
    };
    let button = if kind <= 2 {
        Some(match button {
            0 => 0,
            1 => 2,
            2 => 1,
            _ => return None,
        })
    } else {
        None
    };
    Some(Event::Mouse {
        kind,
        button,
        column,
        row,
        modifiers: u8::from(code & 4 != 0)
            | (u8::from(code & 8 != 0) * 4)
            | (u8::from(code & 16 != 0) * 2),
        lines: 1,
    })
}
