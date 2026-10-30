//! Draws one row of the commit graph.

use gitcraft_core::graph::{self, GraphRow};
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme, mouse};

use crate::app::{Message, ROW_HEIGHT};

pub const LANE_WIDTH: f32 = 14.0;
const PAD: f32 = 10.0;
const MAX_LANES: u32 = 24;

/// The same for every row, so that the text after it lines up.
pub fn width_for(lanes: u32) -> f32 {
    PAD * 2.0 + (lanes.clamp(1, MAX_LANES) - 1) as f32 * LANE_WIDTH + 4.0
}

pub struct GraphCell<'a> {
    pub row: &'a GraphRow,
    pub lanes: [Color; 8],
    pub background: Color,
    pub merge: bool,
    pub head: bool,
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
        let x = |lane: u32| PAD + lane as f32 * LANE_WIDTH;
        let (h, mid) = (ROW_HEIGHT, ROW_HEIGHT / 2.0);
        let color = |index: u8| self.lanes[index as usize % self.lanes.len()];
        for segment in &self.row.segments {
            let (x1, x2) = (x(segment.from()), x(segment.to()));
            let path = canvas::Path::new(|path| match segment.kind() {
                graph::FULL => {
                    path.move_to(Point::new(x1, 0.0));
                    path.line_to(Point::new(x1, h));
                }
                graph::TOP => {
                    path.move_to(Point::new(x1, 0.0));
                    if (x1 - x2).abs() < f32::EPSILON {
                        path.line_to(Point::new(x1, mid));
                    } else {
                        path.bezier_curve_to(
                            Point::new(x1, mid * 0.8),
                            Point::new(x2, mid * 0.4),
                            Point::new(x2, mid),
                        );
                    }
                }
                _ => {
                    path.move_to(Point::new(x1, mid));
                    if (x1 - x2).abs() < f32::EPSILON {
                        path.line_to(Point::new(x1, h));
                    } else {
                        path.bezier_curve_to(
                            Point::new(x1, mid + mid * 0.6),
                            Point::new(x2, mid + mid * 0.2),
                            Point::new(x2, h),
                        );
                    }
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(2.0)
                    .with_color(color(segment.color())),
            );
        }
        let centre = Point::new(x(self.row.col), mid);
        let lane = color(self.row.color);
        if self.merge {
            // A merge is a hollow ring.
            let dot = canvas::Path::circle(centre, 4.0);
            frame.fill(&dot, self.background);
            frame.stroke(
                &dot,
                canvas::Stroke::default().with_width(2.5).with_color(lane),
            );
        } else if self.head {
            let dot = canvas::Path::circle(centre, 5.5);
            frame.fill(&dot, lane);
            frame.stroke(
                &dot,
                canvas::Stroke::default()
                    .with_width(2.0)
                    .with_color(self.background),
            );
        } else {
            frame.fill(&canvas::Path::circle(centre, 4.5), lane);
        }
        vec![frame.into_geometry()]
    }
}
