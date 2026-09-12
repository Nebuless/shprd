use shprd_host::render::{Cell, Frame, frame_to_ansi, hello};

#[test]
fn tagged_protocol_22_hello_matches_frozen_wire() -> Result<(), Box<dyn std::error::Error>> {
    // Given protocol22, where TerminalHello removed legacy launch mode.
    // When encoding the handshake, then byte fields match the tagged fixture.
    assert_eq!(hello(22, 100, 30)?, vec![0, 22, 100, 30, 0, 0, 0]);
    assert!(hello(21, 100, 30).is_err());
    assert_eq!(hello(20, 100, 30)?, vec![0, 20, 100, 30, 0, 0, 1, 0, 2]);
    Ok(())
}

#[test]
fn full_repaint_preserves_wide_padding_and_viewport() -> Result<(), Box<dyn std::error::Error>> {
    // Given a wide grapheme, source padding, then a real blank and a letter.
    let frame = Frame {
        cells: ["\u{4e2d}", " ", " ", "x"]
            .into_iter()
            .map(Cell::plain)
            .collect(),
        width: 4,
        height: 1,
        cursor: None,
        hyperlinks: vec![],
        graphics: vec![],
    };
    // When converting source cells to ANSI, then positions retain their meaning.
    assert!(frame_to_ansi(&frame, 4, 1)?.contains("\u{4e2d}\x1b[3G x"));
    assert!(!frame_to_ansi(&frame, 1, 1)?.contains('\u{4e2d}'));
    Ok(())
}
