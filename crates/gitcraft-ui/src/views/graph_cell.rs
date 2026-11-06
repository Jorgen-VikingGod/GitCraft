//! Draws one row of the commit graph.

use egui::epaint::CubicBezierShape;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use gitcraft_core::graph::{self, GraphRow};

use crate::app::ROW_HEIGHT;

pub const LANE_WIDTH: f32 = 14.0;
const PAD: f32 = 10.0;
const MAX_LANES: u32 = 24;

/// The same for every row, so that the text after it lines up.
pub fn width_for(lanes: u32) -> f32 {
    PAD * 2.0 + (lanes.clamp(1, MAX_LANES) - 1) as f32 * LANE_WIDTH + 4.0
}

pub struct GraphCell<'a> {
    pub row: &'a GraphRow,
    pub lanes: [Color32; 8],
    pub background: Color32,
    pub merge: bool,
    pub head: bool,
}

impl GraphCell<'_> {
    /// Paints the cell with its top left corner at `origin`.
    pub fn paint(&self, painter: &Painter, origin: Pos2) {
        let at = |x: f32, y: f32| Pos2::new(origin.x + x, origin.y + y);
        let x = |lane: u32| PAD + lane as f32 * LANE_WIDTH;
        let (h, mid) = (ROW_HEIGHT, ROW_HEIGHT / 2.0);
        let color = |index: u8| self.lanes[index as usize % self.lanes.len()];
        let painter = painter.with_clip_rect(
            Rect::from_min_size(origin, egui::vec2(f32::INFINITY, h))
                .intersect(painter.clip_rect()),
        );
        for segment in &self.row.segments {
            let (x1, x2) = (x(segment.from()), x(segment.to()));
            let stroke = Stroke::new(2.0, color(segment.color()));
            let straight = (x1 - x2).abs() < f32::EPSILON;
            let shape = match segment.kind() {
                graph::FULL => Shape::line_segment([at(x1, 0.0), at(x1, h)], stroke),
                graph::TOP if straight => Shape::line_segment([at(x1, 0.0), at(x1, mid)], stroke),
                graph::TOP => Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    [
                        at(x1, 0.0),
                        at(x1, mid * 0.8),
                        at(x2, mid * 0.4),
                        at(x2, mid),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                )),
                _ if straight => Shape::line_segment([at(x1, mid), at(x1, h)], stroke),
                _ => Shape::CubicBezier(CubicBezierShape::from_points_stroke(
                    [
                        at(x1, mid),
                        at(x1, mid + mid * 0.6),
                        at(x2, mid + mid * 0.2),
                        at(x2, h),
                    ],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                )),
            };
            painter.add(shape);
        }
        let centre = at(x(self.row.col), mid);
        let lane = color(self.row.color);
        if self.merge {
            // A merge is a hollow ring.
            painter.circle(centre, 4.0, self.background, Stroke::new(2.5, lane));
        } else if self.head {
            painter.circle(centre, 5.5, lane, Stroke::new(2.0, self.background));
        } else {
            painter.circle_filled(centre, 4.5, lane);
        }
    }
}
