//! Throwaway spike. Scrolls by itself for a fixed time, prints frame statistics and exits.
//!
//!     graph-iced rows    # 100,000 rows: widgets, with one small canvas per row for the graph
//!     graph-iced canvas  # 100,000 rows: one canvas draws everything that is visible
//!     graph-iced diff    # 20,000 diff lines: widgets
//!
//! Environment: ICED_BACKEND=tiny-skia for the software renderer,
//! ICED_PRESENT_MODE=immediate to measure past vsync, SPIKE_SECONDS, SPIKE_START (a fraction
//! of the list to start from), SPIKE_SHOT (write a PNG after 90 frames, then stop).

use gitcraft_core::graph::{self, GraphRow};
use iced::widget::{canvas, column, container, mouse_area, row, scrollable, space, text};
use iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Size, Subscription, Task, Theme, mouse,
};
use native_theme_iced::{ResolvedTheme, styles};
use std::cell::Cell;
use std::time::{Duration, Instant};

const ROW: f32 = 26.0;
const LANE: f32 = 14.0;
const COMMITS: u32 = 100_000;
const DIFF_LINES: usize = 20_000;
const STEP: f32 = 410.0; // pixels per frame: a fast scroll, every row changes
const LIST: &str = "list";
const LANES: [Color; 8] = [
    Color::from_rgb(0.26, 0.52, 0.96),
    Color::from_rgb(0.20, 0.66, 0.33),
    Color::from_rgb(0.92, 0.40, 0.20),
    Color::from_rgb(0.61, 0.32, 0.88),
    Color::from_rgb(0.90, 0.20, 0.45),
    Color::from_rgb(0.10, 0.66, 0.70),
    Color::from_rgb(0.80, 0.60, 0.10),
    Color::from_rgb(0.45, 0.50, 0.55),
];

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Rows,
    Canvas,
    Diff,
}

struct Commit {
    subject: String,
    author: &'static str,
    date: String,
    badge: Option<String>,
    graph: GraphRow,
}

struct Spike {
    mode: Mode,
    theme: Theme,
    resolved: ResolvedTheme,
    commits: Vec<Commit>,
    diff: Vec<(char, String)>,
    offset: f32,
    height: f32,
    started: Instant,
    first_frame: Option<Duration>,
    last_frame: Option<Instant>,
    frames: Vec<Duration>,
    view_time: Cell<Duration>,
    view_calls: Cell<u32>,
    theme_load: Duration,
    data_build: Duration,
    seconds: u64,
    shot: Option<String>,
}

#[derive(Debug, Clone)]
enum Message {
    Frame(Instant),
    Scrolled(f32),
    Shot(iced::window::Screenshot),
}

fn commits() -> Vec<Commit> {
    let list: Vec<(u32, Vec<u32>)> = (0..COMMITS)
        .map(|i| {
            let mut parents = vec![];
            if i + 1 < COMMITS {
                parents.push(i + 1);
                if i % 10 == 0 && i + 12 < COMMITS {
                    parents.push(i + 2 + i % 9);
                }
                if i % 97 == 0 && i + 60 < COMMITS {
                    parents.push(i + 40 + i % 19);
                }
            }
            (i, parents)
        })
        .collect();
    graph::layout(&list)
        .into_iter()
        .enumerate()
        .map(|(i, graph)| Commit {
            subject: format!(
                "Commit number {i}: adjust the frobnicator and update Grüße 日本語 notes"
            ),
            author: if i % 3 == 0 {
                "Jürgen Könnemann"
            } else {
                "Parity User"
            },
            date: format!("2026-{:02}-{:02}", i % 12 + 1, i % 28 + 1),
            badge: (i % 37 == 0).then(|| format!("feature/branch-{i}")),
            graph,
        })
        .collect()
}

fn diff_lines() -> Vec<(char, String)> {
    (0..DIFF_LINES)
        .map(|i| {
            let kind = [' ', ' ', '+', '-'][i % 4];
            (kind, format!("    fn example_{i}(value: &str) -> Result<(), Error> {{ do_something(value, {i})?; Ok(()) }}"))
        })
        .collect()
}

fn boot() -> (Spike, Task<Message>) {
    let started = Instant::now();
    let mode = match std::env::args().nth(1).as_deref() {
        Some("canvas") => Mode::Canvas,
        Some("diff") => Mode::Diff,
        _ => Mode::Rows,
    };
    let theme_start = Instant::now();
    let (theme, resolved) = match native_theme_iced::from_system() {
        Ok(loaded) => (loaded.theme, loaded.resolved),
        Err(error) => {
            eprintln!("from_system failed, using a preset: {error}");
            native_theme_iced::from_preset("adwaita", false).expect("bundled preset")
        }
    };
    let theme_load = theme_start.elapsed();
    let data_start = Instant::now();
    let (commits, diff) = if mode == Mode::Diff {
        (vec![], diff_lines())
    } else {
        (commits(), vec![])
    };
    let data_build = data_start.elapsed();
    let seconds = std::env::var("SPIKE_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    let start: f32 = std::env::var("SPIKE_START")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let rows = if mode == Mode::Diff {
        diff.len()
    } else {
        commits.len()
    };
    let spike = Spike {
        mode,
        theme,
        resolved,
        commits,
        diff,
        offset: (rows as f32 * ROW - 2000.0).max(0.0) * start,
        height: 900.0,
        started,
        first_frame: None,
        last_frame: None,
        frames: Vec::new(),
        view_time: Cell::new(Duration::ZERO),
        view_calls: Cell::new(0),
        theme_load,
        data_build,
        seconds,
        shot: std::env::var("SPIKE_SHOT").ok(),
    };
    (spike, Task::none())
}

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .map(str::to_owned)
        })
        .and_then(|l| {
            l.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map_or(0.0, |kb| kb / 1024.0)
}

fn report(spike: &Spike) {
    let mut ms: Vec<f64> = spike
        .frames
        .iter()
        .map(|d| d.as_secs_f64() * 1000.0)
        .collect();
    ms.sort_by(f64::total_cmp);
    let pct = |p: f64| {
        ms.get(((ms.len() as f64 - 1.0) * p) as usize)
            .copied()
            .unwrap_or(0.0)
    };
    let view_ms =
        spike.view_time.get().as_secs_f64() * 1000.0 / f64::from(spike.view_calls.get().max(1));
    let name = match spike.mode {
        Mode::Rows => "rows",
        Mode::Canvas => "canvas",
        Mode::Diff => "diff",
    };
    println!(
        "{name}: {} frames, ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1}; view {view_ms:.2} ms; \
         theme {:.0} ms, data {:.0} ms, first frame {:.0} ms; rss {:.0} MB; scrolled {:.0} px",
        ms.len(),
        pct(0.5),
        pct(0.95),
        pct(0.99),
        pct(1.0),
        spike.theme_load.as_secs_f64() * 1000.0,
        spike.data_build.as_secs_f64() * 1000.0,
        spike.first_frame.unwrap_or_default().as_secs_f64() * 1000.0,
        rss_mb(),
        spike.offset,
    );
}

fn update(spike: &mut Spike, message: Message) -> Task<Message> {
    match message {
        Message::Frame(now) => {
            if spike.first_frame.is_none() {
                spike.first_frame = Some(spike.started.elapsed());
            }
            if let Some(last) = spike.last_frame {
                spike.frames.push(now.duration_since(last));
            }
            spike.last_frame = Some(now);
            if spike.started.elapsed() > Duration::from_secs(spike.seconds) {
                report(spike);
                return iced::exit();
            }
            if spike.shot.is_some() && spike.frames.len() == 90 {
                return iced::window::latest()
                    .and_then(iced::window::screenshot)
                    .map(Message::Shot);
            }
            if spike.shot.is_some() && spike.frames.len() > 90 {
                return Task::none();
            }
            let total = total_height(spike);
            spike.offset = (spike.offset + STEP) % (total - spike.height).max(1.0);
            if spike.mode == Mode::Canvas {
                return Task::none();
            }
            iced::widget::operation::scroll_to(
                LIST,
                scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(spike.offset),
                },
            )
        }
        Message::Scrolled(height) => {
            spike.height = height;
            Task::none()
        }
        Message::Shot(shot) => {
            let file = std::fs::File::create(spike.shot.as_ref().unwrap()).unwrap();
            let mut png = png::Encoder::new(
                std::io::BufWriter::new(file),
                shot.size.width,
                shot.size.height,
            );
            png.set_color(png::ColorType::Rgba);
            png.set_depth(png::BitDepth::Eight);
            png.write_header()
                .unwrap()
                .write_image_data(&shot.rgba)
                .unwrap();
            report(spike);
            iced::exit()
        }
    }
}

fn total_height(spike: &Spike) -> f32 {
    if spike.mode == Mode::Diff {
        spike.diff.len() as f32 * ROW
    } else {
        spike.commits.len() as f32 * ROW
    }
}

struct GraphCell<'a>(&'a GraphRow);

fn draw_graph(frame: &mut canvas::Frame, row: &GraphRow, top: f32) {
    let x = |lane: u32| lane as f32 * LANE + LANE / 2.0;
    let mid = top + ROW / 2.0;
    for s in &row.segments {
        let (a, b) = match s.kind() {
            graph::FULL => (
                Point::new(x(s.from()), top),
                Point::new(x(s.from()), top + ROW),
            ),
            graph::TOP => (Point::new(x(s.from()), top), Point::new(x(s.to()), mid)),
            _ => (
                Point::new(x(s.from()), mid),
                Point::new(x(s.to()), top + ROW),
            ),
        };
        let path = canvas::Path::new(|p| {
            p.move_to(a);
            if (a.x - b.x).abs() < f32::EPSILON {
                p.line_to(b);
            } else {
                let c = (a.y + b.y) / 2.0;
                p.bezier_curve_to(Point::new(a.x, c), Point::new(b.x, c), b);
            }
        });
        frame.stroke(
            &path,
            canvas::Stroke::default()
                .with_width(1.6)
                .with_color(LANES[s.color() as usize % LANES.len()]),
        );
    }
    let dot = canvas::Path::circle(Point::new(x(row.col), mid), 4.0);
    frame.fill(&dot, LANES[row.color as usize % LANES.len()]);
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
        draw_graph(&mut frame, self.0, 0.0);
        vec![frame.into_geometry()]
    }
}

struct WholeList<'a>(&'a Spike);

impl canvas::Program<Message> for WholeList<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let spike = self.0;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let first = (spike.offset / ROW) as usize;
        let color = theme.palette().text;
        let visible = (bounds.height / ROW) as usize + 2;
        for (i, commit) in spike.commits.iter().enumerate().skip(first).take(visible) {
            let top = i as f32 * ROW - spike.offset;
            draw_graph(&mut frame, &commit.graph, top);
            let graph_width = commit.graph.width as f32 * LANE + 8.0;
            let label = |content: &str, x: f32| canvas::Text {
                content: content.to_owned(),
                position: Point::new(x, top + 5.0),
                color,
                size: 14.0.into(),
                ..canvas::Text::default()
            };
            frame.fill_text(label(&commit.subject, graph_width));
            frame.fill_text(label(commit.author, bounds.width - 300.0));
            frame.fill_text(label(&commit.date, bounds.width - 110.0));
        }
        vec![frame.into_geometry()]
    }
}

fn rows_view(spike: &Spike) -> Element<'_, Message> {
    let first = (spike.offset / ROW) as usize;
    let visible = (spike.height / ROW) as usize + 2;
    let mut items: Vec<Element<'_, Message>> = vec![space().height(first as f32 * ROW).into()];
    for commit in spike.commits.iter().skip(first).take(visible) {
        let width = commit.graph.width as f32 * LANE + 4.0;
        let mut line = row![canvas(GraphCell(&commit.graph)).width(width).height(ROW),]
            .spacing(6)
            .height(ROW)
            .align_y(iced::Center);
        if let Some(badge) = &commit.badge {
            line = line.push(
                container(text(badge.as_str()).size(12))
                    .padding([1, 6])
                    .style(container::rounded_box),
            );
        }
        line = line
            .push(text(commit.subject.as_str()).size(14).width(Length::Fill))
            .push(text(commit.author).size(14).width(180))
            .push(text(commit.date.as_str()).size(14).width(90));
        items.push(mouse_area(line).into());
    }
    let shown = first + visible;
    let below = spike.commits.len().saturating_sub(shown) as f32 * ROW;
    items.push(space().height(below).into());
    list(spike, column(items))
}

fn diff_view(spike: &Spike) -> Element<'_, Message> {
    let first = (spike.offset / ROW) as usize;
    let visible = (spike.height / ROW) as usize + 2;
    let mut items: Vec<Element<'_, Message>> = vec![space().height(first as f32 * ROW).into()];
    for (i, (kind, content)) in spike.diff.iter().enumerate().skip(first).take(visible) {
        let background = match kind {
            '+' => Some(Color::from_rgba(0.2, 0.7, 0.3, 0.16)),
            '-' => Some(Color::from_rgba(0.9, 0.2, 0.2, 0.16)),
            _ => None,
        };
        let line = row![
            text(i + 1).size(13).width(56).font(iced::Font::MONOSPACE),
            text(content.as_str()).size(13).font(iced::Font::MONOSPACE),
        ]
        .height(ROW)
        .align_y(iced::Center);
        items.push(
            container(line)
                .width(Length::Fill)
                .style(move |_: &Theme| container::Style {
                    background: background.map(Into::into),
                    ..container::Style::default()
                })
                .into(),
        );
    }
    let below = spike.diff.len().saturating_sub(first + visible) as f32 * ROW;
    items.push(space().height(below).into());
    list(spike, column(items))
}

fn list<'a>(spike: &'a Spike, content: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    scrollable(content.width(Length::Fill))
        .id(LIST)
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(scrollable::Direction::Vertical(styles::scrollbar(
            &spike.resolved,
        )))
        .style(styles::scrollable(&spike.resolved))
        .on_scroll(|v| Message::Scrolled(v.bounds().height))
        .into()
}

fn view(spike: &Spike) -> Element<'_, Message> {
    let start = Instant::now();
    let element = match spike.mode {
        Mode::Rows => rows_view(spike),
        Mode::Diff => diff_view(spike),
        Mode::Canvas => canvas(WholeList(spike))
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    };
    spike.view_time.set(spike.view_time.get() + start.elapsed());
    spike.view_calls.set(spike.view_calls.get() + 1);
    element
}

fn subscription(_: &Spike) -> Subscription<Message> {
    iced::window::frames().map(Message::Frame)
}

fn main() -> iced::Result {
    iced::application(boot, update, view)
        .title("GitCraft iced spike")
        .theme(|spike: &Spike| spike.theme.clone())
        .style(|spike: &Spike, theme| styles::application(&spike.resolved)(theme))
        .subscription(subscription)
        .window_size(Size::new(1200.0, 900.0))
        .run()
}
