#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use shprd_host::endpoint::{Endpoint, Event};
use tokio::net::UnixListener;
use tokio_util::codec::LengthDelimitedCodec;

#[test]
fn surface_crop_preserves_cells_cursor_and_patch_revisions()
-> Result<(), Box<dyn std::error::Error>> {
    // Given a full shell surface, with one pane inset inside the tab.
    let cell = |symbol: &str| (symbol.to_owned(), 0_u32, 0_u32, 0_u16, false, None::<u32>);
    let frame = (
        vec![cell("#"), cell("a"), cell("b"), cell("#")],
        4_u16,
        1_u16,
        Some((2_u16, 0_u16, true, 1_u8)),
        Vec::<String>::new(),
        Vec::<u8>::new(),
    );
    let pane = (
        "fixture:p1",
        1_u64,
        (0_u16, 0_u16, 4_u16, 1_u16),
        (1_u16, 0_u16, 2_u16, 1_u16),
        None::<(u16, u16, u16, u16)>,
        Some((0_u64, 10_u64, 1_u16)),
        true,
        false,
        false,
        false,
        0_u16,
        0_u16,
    );
    let bytes = bincode::encode_to_vec(
        ("boot-fixture", 3_u64, 7_u64, frame, vec![pane]),
        bincode::config::standard(),
    )?;
    let mut surface = shprd_host::surface::Surface::decode(&bytes)?;
    // When cropping the pane, then tab decorations disappear and cursor becomes pane-relative.
    let cropped = surface.crop("fixture:p1")?;
    assert_eq!(
        cropped
            .cells
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>(),
        "ab"
    );
    assert_eq!((cropped.width, cropped.height), (2, 1));
    assert_eq!(cropped.cursor.ok_or("cursor lost")?.x, 1);
    let patch = bincode::encode_to_vec(
        (
            "boot-fixture",
            3_u64,
            7_u64,
            8_u64,
            vec![(2_u16, 0_u16, vec![cell("c")])],
            vec![pane],
            None::<(u16, u16, bool, u8)>,
        ),
        bincode::config::standard(),
    )?;
    assert!(surface.patch(&patch)?);
    assert_eq!(surface.crop("fixture:p1")?.cells[1].symbol, "c");
    assert!(!surface.patch(&patch)?);
    assert_eq!(surface.revision, 8);
    Ok(())
}

#[tokio::test]
async fn endpoint_negotiates_before_scoped_requests() -> Result<(), Box<dyn std::error::Error>> {
    // Given the real endpoint socket with frozen generation-one envelopes.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("endpoint.sock");
    let listener = UnixListener::bind(&path)?;
    let server = async {
        let (socket, _) = listener.accept().await?;
        let mut wire = LengthDelimitedCodec::builder()
            .little_endian()
            .new_framed(socket);
        let hello = wire.next().await.ok_or("missing hello")??;
        let ((tag, kind, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&hello, bincode::config::standard())?;
        assert_eq!((tag, kind.as_str()), (20, "endpoint.hello.v1"));
        let hello: Value = serde_json::from_str(&data)?;
        assert_eq!(hello["generation"], 1);
        assert_eq!(hello["surface_size"], json!({"cols":100,"rows":30}));
        assert_eq!(hello["surface_codecs"], json!(["shell.surface.v1"]));
        assert_eq!(hello["endpoint_keybindings"], false);
        let welcome = json!({"generation":1,"server_version":"0.9.0","snapshot_codec":"shell.snapshot.v1","surface_codec":"shell.surface.v1","input_codec":"shell.input.semantic.v1","blob_codec":"shell.blob.v1","methods":["pane.focus"],"capabilities":["health_check"]});
        for (kind, data) in [
            ("endpoint.welcome.v1", welcome),
            (
                "shell.snapshot.v1",
                json!({"boot_id":"boot-fixture","revision":3}),
            ),
        ] {
            wire.send(
                bincode::encode_to_vec(
                    (20_u32, kind, data.to_string()),
                    bincode::config::standard(),
                )?
                .into(),
            )
            .await?;
        }
        let request = wire.next().await.ok_or("missing request")??;
        let ((tag, boot, data), _): ((u32, String, String), usize) =
            bincode::decode_from_slice(&request, bincode::config::standard())?;
        assert_eq!((tag, boot.as_str()), (15, "boot-fixture"));
        let request: Value = serde_json::from_str(&data)?;
        assert_eq!(
            request,
            json!({"id":"focus-1","method":"pane.focus","params":{"pane_id":"fixture:p1"}})
        );
        for (last, chunk) in [
            (false, &b"{\"result\":"[..]),
            (true, &b"{\"focused\":true}}"[..]),
        ] {
            wire.send(
                bincode::encode_to_vec(
                    (18_u32, "boot-fixture", "focus-1", last, chunk),
                    bincode::config::standard(),
                )?
                .into(),
            )
            .await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let client = async {
        // When negotiating and submitting an advertised method.
        let mut endpoint = Endpoint::connect(&path, 100, 30).await?;
        assert_eq!(endpoint.negotiation().server_version, "0.9.0");
        assert!(
            endpoint
                .request("bad", "unadvertised", &json!({}))
                .await
                .is_err()
        );
        endpoint
            .request("focus-1", "pane.focus", &json!({"pane_id":"fixture:p1"}))
            .await?;
        // Then chunks reassemble once with matching boot and request identity.
        loop {
            if let Event::Reply { id, value } = endpoint.next().await? {
                assert_eq!(id, "focus-1");
                assert_eq!(value, json!({"result":{"focused":true}}));
                break;
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        tokio::try_join!(server, client)
    })
    .await??;
    Ok(())
}
