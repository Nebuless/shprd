//! Frozen shell surface layouts and pane-relative projection.

use crate::{
    endpoint::{Error, decode},
    render::{Cell, Cursor, Frame},
};
use bincode::Decode;

#[derive(Clone, Debug, Decode)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, Decode)]
pub struct Scroll {
    pub offset: u64,
    pub maximum: u64,
    pub viewport_rows: u16,
}

#[derive(Clone, Debug, Decode)]
pub struct Pane {
    pub id: String,
    pub content_revision: u64,
    pub rect: Rect,
    pub inner: Rect,
    pub scrollbar: Option<Rect>,
    pub scroll: Option<Scroll>,
    pub focused: bool,
    pub mouse_reporting: bool,
    pub pixel_mouse: bool,
    pub alternate_screen: bool,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

#[derive(Debug, Decode)]
pub struct Surface {
    pub boot_id: String,
    pub projection_revision: u64,
    pub revision: u64,
    pub frame: Frame,
    pub panes: Vec<Pane>,
}

#[derive(Decode)]
struct Row {
    x: u16,
    y: u16,
    cells: Vec<Cell>,
}

#[derive(Decode)]
struct Patch {
    boot_id: String,
    projection_revision: u64,
    base: u64,
    revision: u64,
    rows: Vec<Row>,
    panes: Vec<Pane>,
    cursor: Option<Cursor>,
}

impl Surface {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        // Optional split/popup/graphics data follows this frozen prefix.
        let (surface, _): (Self, usize) = decode(bytes)?;
        if surface.frame.cells.len()
            != usize::from(surface.frame.width) * usize::from(surface.frame.height)
        {
            return Err(Error::Contract("invalid surface dimensions"));
        }
        Ok(surface)
    }

    pub fn patch(&mut self, bytes: &[u8]) -> Result<bool, Error> {
        let (patch, _): (Patch, usize) = decode(bytes)?;
        if patch.boot_id != self.boot_id {
            return Err(Error::Contract("surface server boot changed"));
        }
        if patch.base != self.revision {
            return Ok(false);
        }
        let width = usize::from(self.frame.width);
        for row in patch.rows {
            if row.y >= self.frame.height || row.x >= self.frame.width {
                continue;
            }
            let start = usize::from(row.y) * width + usize::from(row.x);
            for (target, cell) in self.frame.cells[start..start + width - usize::from(row.x)]
                .iter_mut()
                .zip(row.cells)
            {
                *target = cell;
            }
        }
        self.projection_revision = patch.projection_revision;
        self.revision = patch.revision;
        self.panes = patch.panes;
        if patch.cursor.is_some() {
            self.frame.cursor = patch.cursor;
        }
        Ok(true)
    }

    pub fn crop(&self, pane_id: &str) -> Result<Frame, Error> {
        let rect = &self
            .panes
            .iter()
            .find(|pane| pane.id == pane_id)
            .ok_or(Error::Contract("pane absent from surface"))?
            .inner;
        if u32::from(rect.x) + u32::from(rect.width) > u32::from(self.frame.width)
            || u32::from(rect.y) + u32::from(rect.height) > u32::from(self.frame.height)
        {
            return Err(Error::Contract("pane outside surface bounds"));
        }
        let mut cells = Vec::with_capacity(usize::from(rect.width) * usize::from(rect.height));
        for y in usize::from(rect.y)..usize::from(rect.y) + usize::from(rect.height) {
            let start = y * usize::from(self.frame.width) + usize::from(rect.x);
            cells.extend_from_slice(&self.frame.cells[start..start + usize::from(rect.width)]);
        }
        let cursor = self.frame.cursor.as_ref().and_then(|cursor| {
            let x = cursor.x.checked_sub(rect.x)?;
            let y = cursor.y.checked_sub(rect.y)?;
            (x < rect.width && y < rect.height).then_some(Cursor {
                x,
                y,
                visible: cursor.visible,
                shape: cursor.shape,
            })
        });
        Ok(Frame {
            cells,
            width: rect.width,
            height: rect.height,
            cursor,
            hyperlinks: self.frame.hyperlinks.clone(),
            graphics: Vec::new(),
        })
    }
}
