//! The app's 24×24 outline icons.

use std::collections::HashMap;
use std::sync::LazyLock;

use iced::widget::svg;
use iced::{Color, Element};

const PATHS: [(&str, &str); 36] = [
    (
        "fetch",
        "M23 4v6h-6M1 20v-6h6M3.5 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15",
    ),
    (
        "pull",
        "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3",
    ),
    (
        "push",
        "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M17 8l-5-5-5 5M12 3v12",
    ),
    (
        "branch",
        "M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9",
    ),
    (
        "merge",
        "M18 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21V9a9 9 0 0 0 9 9",
    ),
    ("stash", "M21 8v13H3V8M1 3h22v5H1zM10 12h4"),
    (
        "tag",
        "M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82zM7 7h.01",
    ),
    ("cloud", "M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10z"),
    (
        "settings",
        "M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6",
    ),
    (
        "search",
        "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.35-4.35",
    ),
    ("x", "M18 6L6 18M6 6l12 12"),
    ("plus", "M12 5v14M5 12h14"),
    ("minus", "M5 12h14"),
    (
        "folder",
        "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z",
    ),
    ("chevron-right", "M9 18l6-6-6-6"),
    ("chevron-down", "M6 9l6 6 6-6"),
    ("chevron-up", "M18 15l-6-6-6 6"),
    ("check", "M20 6L9 17l-5-5"),
    (
        "edit",
        "M12 20h9M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z",
    ),
    (
        "history",
        "M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01",
    ),
    (
        "file",
        "M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9zM13 2v7h7",
    ),
    (
        "commit",
        "M12 16a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM1.05 12H7M17.01 12h5.95",
    ),
    (
        "box",
        "M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z",
    ),
    (
        "alert",
        "M12 9v4M12 17h.01M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z",
    ),
    (
        "grip",
        "M9 5h.01M9 12h.01M9 19h.01M15 5h.01M15 12h.01M15 19h.01",
    ),
    ("changes", "M12 3v18M3 12h18"),
    ("undo", "M3 7v6h6M21 17a9 9 0 0 0-15-6.7L3 13"),
    (
        "copy",
        "M20 9h-9a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-9a2 2 0 0 0-2-2zM5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1",
    ),
    (
        "repo",
        "M4 19.5A2.5 2.5 0 0 1 6.5 17H20M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z",
    ),
    (
        "user",
        "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M12 11a4 4 0 1 0 0-8 4 4 0 0 0 0 8z",
    ),
    (
        "clock",
        "M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20zM12 6v6l4 2",
    ),
    (
        "home",
        "M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM9 22V12h6v10",
    ),
    ("more", "M5 12h.01M12 12h.01M19 12h.01"),
    ("arrow-up", "M12 19V5M5 12l7-7 7 7"),
    ("arrow-down", "M12 5v14M19 12l-7 7-7-7"),
    ("menu", "M3 6h18M3 12h18M3 18h18"),
];

static HANDLES: LazyLock<HashMap<&'static str, svg::Handle>> = LazyLock::new(|| {
    PATHS
        .iter()
        .map(|(name, path)| {
            let markup = format!(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' \
                 stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='{path}'/></svg>"
            );
            (*name, svg::Handle::from_memory(markup.into_bytes()))
        })
        .collect()
});

pub fn icon<'a, Message: 'a>(name: &str, size: f32, color: Color) -> Element<'a, Message> {
    let handle = HANDLES.get(name).unwrap_or_else(|| &HANDLES["x"]).clone();
    svg(handle)
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}
