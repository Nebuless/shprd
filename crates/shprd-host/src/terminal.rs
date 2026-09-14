//! Direct terminal transport for verified Herdr protocols14-20 and22.

use crate::{herdr, render};
use bincode::{Decode, Encode};
use futures_util::{SinkExt, StreamExt};
use std::{path::Path, time::Duration};
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Socket(#[from] herdr::Error),
    #[error(transparent)]
    Render(#[from] render::Error),
    #[error("terminal I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal decode: {0}")]
    Decode(#[from] bincode::error::DecodeError),
    #[error("terminal encode: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("terminal connection closed")]
    Closed,
    #[error("terminal handshake timed out")]
    Timeout,
    #[error("invalid terminal handshake: {0}")]
    Handshake(String),
    #[error("terminal already attached")]
    Attached,
}

#[derive(Debug)]
pub enum Output {
    Terminal {
        seq: u64,
        width: u16,
        height: u16,
        full: bool,
        mouse_reporting: Option<bool>,
        bytes: Vec<u8>,
    },
    Frame(render::Frame),
    Clipboard(String),
    Mouse {
        enabled: bool,
        pixels: bool,
    },
    Keyboard {
        flags: u16,
        modify_other_keys: u8,
    },
    Closed(Option<String>),
}

#[derive(Debug)]
pub enum Command {
    Input(Vec<u8>),
    Resize(u16, u16),
    Scroll {
        direction: String,
        lines: u16,
        column: Option<u16>,
        row: Option<u16>,
        source: String,
    },
}

pub(crate) struct WriteCommand {
    pub command: Command,
    pub reply: oneshot::Sender<Result<(), String>>,
}

#[derive(Debug)]
pub struct Terminal {
    wire: Framed<herdr::Socket, LengthDelimitedCodec>,
    protocol: u32,
    attached: Option<String>,
}

impl Terminal {
    pub async fn connect(path: &Path, protocol: u32, cols: u16, rows: u16) -> Result<Self, Error> {
        let hello = render::hello(protocol, cols, rows)?;
        tokio::time::timeout(Duration::from_secs(8), async {
            let socket = herdr::connect(path).await?;
            let mut wire = LengthDelimitedCodec::builder()
                .little_endian()
                .max_frame_length(32 * 1024 * 1024)
                .new_framed(socket);
            wire.send(hello.into()).await?;
            let bytes = wire.next().await.ok_or(Error::Closed)??;
            let (tag, version, encoding, error): (u32, u32, u32, Option<String>) = decode(&bytes)?;
            if let Some(error) = error {
                return Err(Error::Handshake(error));
            }
            if tag != 0 || version != protocol || encoding != 1 {
                return Err(Error::Handshake(
                    "unexpected protocol or encoding".to_owned(),
                ));
            }
            Ok(Self {
                wire,
                protocol,
                attached: None,
            })
        })
        .await
        .map_err(|_| Error::Timeout)?
    }

    async fn send(&mut self, value: impl Encode) -> Result<(), Error> {
        let bytes = bincode::encode_to_vec(value, bincode::config::standard())?;
        self.wire.send(bytes.into()).await?;
        Ok(())
    }

    pub async fn attach(&mut self, id: &str, takeover: bool) -> Result<(), Error> {
        if self.attached.as_deref() == Some(id) {
            return Ok(());
        }
        if self.attached.is_some() {
            return Err(Error::Attached);
        }
        self.send((5_u32, id, takeover)).await?;
        self.attached = Some(id.to_owned());
        Ok(())
    }

    pub async fn input(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.send((1_u32, bytes)).await
    }

    pub async fn scroll(
        &mut self,
        direction: &str,
        lines: u16,
        column: Option<u16>,
        row: Option<u16>,
        source: &str,
    ) -> Result<(), Error> {
        let source = if source == "page-key" { 1_u32 } else { 0_u32 };
        let direction = if direction == "up" { 0_u32 } else { 1_u32 };
        self.send((6_u32, source, direction, lines, column, row, 0_u8))
            .await
    }

    pub async fn resize(&mut self, cols: u16, rows: u16) -> Result<(), Error> {
        if self.protocol == 22 {
            self.send((3_u32, cols, rows, 0_u16, 0_u16, false)).await
        } else {
            self.send((3_u32, cols, rows, 0_u16, 0_u16)).await
        }
    }

    pub(crate) async fn run(
        mut self,
        mut commands: mpsc::Receiver<WriteCommand>,
        outputs: mpsc::UnboundedSender<Output>,
    ) -> Result<(), Error> {
        let mut mouse_reporting = None;
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(WriteCommand { command, reply }) = command else { return Ok(()); };
                    let result = tokio::time::timeout(Duration::from_secs(8), async {
                        match command {
                            Command::Input(bytes) => self.input(&bytes).await,
                            Command::Resize(cols, rows) => self.resize(cols, rows).await,
                            Command::Scroll { direction, lines, column, row, source } =>
                                self.scroll(&direction, lines, column, row, &source).await,
                        }
                    }).await.unwrap_or(Err(Error::Timeout));
                    let _ = reply.send(result.as_ref().map_err(ToString::to_string).copied());
                    result?;
                }
                output = self.next() => {
                    let mut output = output?;
                    match &mut output {
                        Output::Mouse { enabled, .. } => mouse_reporting = Some(*enabled),
                        Output::Terminal { mouse_reporting: mode, .. } => *mode = mouse_reporting,
                        _ => {}
                    }
                    let closed = matches!(output, Output::Closed(_));
                    if outputs.send(output).is_err() || closed { return Ok(()); }
                }
            }
        }
    }

    /// Partial length-prefixed frames remain buffered when this future is cancelled.
    pub async fn next(&mut self) -> Result<Output, Error> {
        loop {
            let bytes = self.wire.next().await.ok_or(Error::Closed)??;
            let (tag, offset): (u32, usize) =
                bincode::decode_from_slice(&bytes, bincode::config::standard())?;
            let body = &bytes[offset..];
            let modern = self.protocol == 22;
            if tag == if modern { 1 } else { 2 } {
                let (seq, width, height, full, bytes) = decode(body)?;
                return Ok(Output::Terminal {
                    seq,
                    width,
                    height,
                    full,
                    mouse_reporting: None,
                    bytes,
                });
            }
            if tag == 1 && !modern {
                return Ok(Output::Frame(decode(body)?));
            }
            if tag == if modern { 3 } else { 4 } {
                return Ok(Output::Closed(decode(body)?));
            }
            if tag == if modern { 5 } else { 6 } {
                return Ok(Output::Clipboard(decode(body)?));
            }
            if tag == if modern { 8 } else { 9 } {
                let (enabled, pixels) = if modern {
                    decode(body)?
                } else {
                    (decode(body)?, false)
                };
                return Ok(Output::Mouse { enabled, pixels });
            }
            if tag == 16 && modern {
                let (flags, modify_other_keys) = decode(body)?;
                return Ok(Output::Keyboard {
                    flags,
                    modify_other_keys,
                });
            }
        }
    }
}

fn decode<T: Decode<()>>(bytes: &[u8]) -> Result<T, Error> {
    let (value, _) = bincode::decode_from_slice(
        bytes,
        bincode::config::standard().with_limit::<{ 32 * 1024 * 1024 }>(),
    )?;
    Ok(value)
}
