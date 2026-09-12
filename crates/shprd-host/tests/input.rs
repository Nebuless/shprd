use shprd_host::input::{Classifier, Event, Key, encode};

#[test]
fn input_preserves_fragmented_text_paste_and_modified_keys()
-> Result<(), Box<dyn std::error::Error>> {
    // Given input chunks split across UTF-8, CSI and bracketed paste boundaries.
    let mut parser = Classifier::default();
    assert!(parser.feed(&[0xe4, 0xb8])?.is_empty());
    assert_eq!(
        parser.feed(&[0xad])?,
        vec![Event::Text("\u{4e2d}".to_owned())]
    );
    assert!(parser.feed(b"\x1b[200~hello")?.is_empty());
    // When remaining input arrives, then no split sequence leaks as text.
    assert_eq!(
        parser.feed(b"\r\nworld\x1b[201~\x1b[13;2u\x1b\x7f")?,
        vec![
            Event::Paste("hello\r\nworld".to_owned()),
            Event::Key {
                code: Key::Enter,
                modifiers: 1
            },
            Event::Key {
                code: Key::Backspace,
                modifiers: 4
            }
        ]
    );
    assert!(parser.feed(b"\x1b")?.is_empty());
    assert_eq!(
        parser.flush(),
        vec![Event::Key {
            code: Key::Esc,
            modifiers: 0
        }]
    );
    Ok(())
}

#[test]
fn semantic_encoding_matches_literal_enter_wire() -> Result<(), Box<dyn std::error::Error>> {
    // Given one Enter press for pane p1.
    let events = vec![Event::Key {
        code: Key::Enter,
        modifiers: 0,
    }];
    // When encoding, then variant order and optional fields match tagged Herdr wire.
    assert_eq!(
        encode("p1", &events)?,
        b"\x0d\x02p1\x01\x00\x01\x00\x00\x01\x00\x00\x00\x00\x00"
    );
    Ok(())
}

#[test]
fn sgr_mouse_rejects_invalid_coordinates_and_preserves_button_order()
-> Result<(), Box<dyn std::error::Error>> {
    let mut parser = Classifier::default();
    // Given middle-button SGR press and an invalid zero coordinate.
    // When classified, then only valid pane-relative mouse input survives.
    let events = parser.feed(b"\x1b[<1;3;4M\x1b[<0;0;2M")?;
    assert_eq!(
        events,
        vec![Event::Mouse {
            kind: 0,
            button: Some(2),
            column: 2,
            row: 3,
            modifiers: 0,
            lines: 1
        }]
    );
    Ok(())
}
