use pdf_writer::Rect;
use std::collections::HashMap;

/// Label collision index bounded by the viewport.
pub(crate) struct Collisions {
    viewport: Rect,
    cell: f32,
    cells: HashMap<(i32, i32), Vec<Rect>>,
}

impl Collisions {
    pub(crate) fn new(width: f32, height: f32) -> Self {
        Self {
            viewport: Rect::new(0.0, 0.0, width, height),
            cell: (width.max(height) / 64.0).max(1.0),
            cells: HashMap::new(),
        }
    }
    fn cells_for(&self, bounds: Rect) -> impl Iterator<Item = (i32, i32)> + use<> {
        let x1 = (bounds.x1.max(0.0) / self.cell) as i32;
        let x2 = (bounds.x2.min(self.viewport.x2) / self.cell) as i32;
        let y1 = (bounds.y1.max(0.0) / self.cell) as i32;
        let y2 = (bounds.y2.min(self.viewport.y2) / self.cell) as i32;
        (x1..=x2).flat_map(move |x| (y1..=y2).map(move |y| (x, y)))
    }
    pub(crate) fn visible(&self, bounds: Rect) -> bool {
        intersects(self.viewport, bounds)
    }
    pub(crate) fn overlaps(&self, bounds: Rect) -> bool {
        self.cells_for(bounds).any(|cell| {
            self.cells
                .get(&cell)
                .is_some_and(|items| items.iter().any(|previous| intersects(*previous, bounds)))
        })
    }
    pub(crate) fn insert(&mut self, bounds: Rect) {
        for cell in self.cells_for(bounds) {
            self.cells.entry(cell).or_default().push(bounds);
        }
    }
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.x1 < b.x2 && b.x1 < a.x2 && a.y1 < b.y2 && b.y1 < a.y2
}
