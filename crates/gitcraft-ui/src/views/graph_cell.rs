//! Draws one row of the commit graph.

use gitcraft_core::graph::{self, GraphRow};
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

use crate::app::{Message, ROW_HEIGHT};

pub const LANE_WIDTH: f32 = 14.0;
const MAX_LANES: u32 = 24;

pub fn width_for(lanes: u32) -> f32 {
    lanes.min(MAX_LANES) as f32 * LANE_WIDTH + 4.0
}

pub struct GraphCell<'a> {
    pub row: &'a GraphRow,
    pub lanes: [Color; 8],
}

impl canvas::Program<Message> for GraphCell<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let x = |lane: u32| lane as f32 * LANE_WIDTH + LANE_WIDTH / 2.0;
        let mid = ROW_HEIGHT / 2.0;
        let color = |index: u8| self.lanes[index as usize % self.lanes.len()];
        for segment in &self.row.segments {
            let (from, to) = match segment.kind() {
                graph::FULL => (
                    Point::new(x(segment.from()), 0.0),
                    Point::new(x(segment.from()), ROW_HEIGHT),
                ),
                graph::TOP => (
                    Point::new(x(segment.from()), 0.0),
                    Point::new(x(segment.to()), mid),
                ),
                _ => (
                    Point::new(x(segment.from()), mid),
                    Point::new(x(segment.to()), ROW_HEIGHT),
                ),
            };
            let path = canvas::Path::new(|path| {
                path.move_to(from);
                if (from.x - to.x).abs() < f32::EPSILON {
                    path.line_to(to);
                } else {
                    let bend = (from.y + to.y) / 2.0;
                    path.bezier_curve_to(Point::new(from.x, bend), Point::new(to.x, bend), to);
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.6)
                    .with_color(color(segment.color())),
            );
        }
        let dot = canvas::Path::circle(Point::new(x(self.row.col), mid), 4.0);
        frame.fill(&dot, color(self.row.color));
        vec![frame.into_geometry()]
    }
}
