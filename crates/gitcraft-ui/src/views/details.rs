//! What's below the history: a commit, a comparison, or a stash, each with a file list and a diff.

use egui::{Align, Align2, Color32, Label, Layout, Margin, RichText, Sense, Ui, vec2};
use gitcraft_core::model::FileChange;

use super::widgets::{
    GRIP, badge, btn, danger_text, file_row, fill, heading, hline, mono, muted, placeholder,
    primary, region, scroll, split_x, splitter, text,
};
use super::{Cx, diff};
use crate::actions::{Action, MenuRequest};
use crate::app::{Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
use crate::logic::{full_date, initials, name_hue, plural, short_sha};

fn files(
    ui: &mut Ui,
    cx: &Cx,
    id: &str,
    list: &[FileChange],
    selected: Option<&str>,
    rev: Option<&str>,
    empty: &str,
) {
    if list.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                RichText::new(empty)
                    .size(13.0)
                    .italics()
                    .color(cx.c().text_muted),
            );
        });
        return;
    }
    scroll(("files", id)).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for file in list {
            let row = file_row(ui, cx, file, selected == Some(file.path.as_str()), None);
            if row.row.clicked() {
                cx.send(Message::PickFile(file.path.clone()));
            } else if row.row.secondary_clicked()
                && let Some(rev) = rev
            {
                cx.send(Message::OpenMenu(MenuRequest::File {
                    area: None,
                    path: file.path.clone(),
                    rev: Some(rev.to_owned()),
                }));
            }
        }
    });
}

fn with_diff(ui: &mut Ui, cx: &Cx, tab: &RepoTab, empty: &str, left: impl FnOnce(&mut Ui)) {
    let c = cx.c();
    let area = ui.max_rect();
    let width = cx
        .app
        .settings
        .details_width
        .min(area.width() - 200.0)
        .max(120.0);
    let (list, rest) = split_x(area, width);
    let (grip, right) = split_x(rest, GRIP);
    region(ui, list, |ui| {
        fill(ui, list, c.panel);
        left(ui);
    });
    splitter(ui, cx, grip, Splitter::Details);
    region(ui, right, |ui| {
        diff::view(
            ui,
            cx,
            &tab.commit_pane,
            PaneKey::Commit,
            None,
            empty,
            None,
            right.width(),
        );
    });
}

fn hsl(hue: f32, saturation: f32, lightness: f32) -> Color32 {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let section = hue / 60.0;
    let x = chroma * (1.0 - (section % 2.0 - 1.0).abs());
    let (r, g, b) = match section as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round() as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

fn avatar(ui: &mut Ui, cx: &Cx, name: &str) {
    let color = hsl(name_hue(name) as f32, 0.55, 0.45);
    let (rect, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 16.0, color);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        initials(name),
        cx.look().ui(12.0),
        Color32::WHITE,
    );
}

/// How tall the part above the file list may get: just over half of the pane.
fn meta_height(cx: &Cx) -> f32 {
    let app = cx.app;
    let pane = app.window.y - app.settings.history_height - 150.0;
    (pane * 0.55).max(120.0)
}

fn fact(ui: &mut Ui, cx: &Cx, label: &str, value: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (rect, _) = ui.allocate_exact_size(vec2(62.0, 16.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            cx.look().ui(12.0),
            cx.c().text_muted,
        );
        value(ui);
    });
}

pub fn commit(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    if let Some(error) = &tab.detail_error {
        fill(ui, ui.max_rect(), c.bg);
        let rect = ui.max_rect();
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            error,
            look.ui(13.0),
            c.danger,
        );
        return;
    }
    let Some(detail) = &tab.detail else {
        placeholder(ui, look, "Loading…");
        return;
    };
    let empty = if detail.files.is_empty() {
        "This commit has no file changes"
    } else {
        "Select a file to see its changes"
    };
    with_diff(ui, cx, tab, empty, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        scroll(("meta", &detail.sha))
            .auto_shrink([false, true])
            .max_height(meta_height(cx))
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(14, 12))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            avatar(ui, cx, &detail.author);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                text(ui, detail.author.as_str(), 13.0, c.text);
                                muted(ui, look, format!("<{}>", detail.email), 13.0);
                                muted(ui, look, full_date(detail.time), 12.0);
                            });
                        });
                        if !detail.refs.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                                for label in &detail.refs {
                                    badge(ui, look, label);
                                }
                            });
                        }
                        ui.add(
                            Label::new(RichText::new(detail.message.as_str()).size(13.0))
                                .selectable(true)
                                .wrap(),
                        );
                        fact(ui, cx, "SHA", |ui| {
                            let response = ui.add(
                                Label::new(
                                    RichText::new(detail.sha.as_str())
                                        .size(12.0)
                                        .monospace()
                                        .color(c.accent),
                                )
                                .wrap()
                                .sense(Sense::click()),
                            );
                            if response.clicked() {
                                cx.send(Message::Copy(detail.sha.clone()));
                            }
                        });
                        fact(
                            ui,
                            cx,
                            if detail.parents.len() > 1 {
                                "Parents"
                            } else {
                                "Parent"
                            },
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if detail.parents.is_empty() {
                                    mono(ui, "(root commit)", 12.0, c.text);
                                }
                                for parent in &detail.parents {
                                    let response = ui.add(
                                        Label::new(
                                            RichText::new(short_sha(parent))
                                                .size(12.0)
                                                .monospace()
                                                .color(c.accent),
                                        )
                                        .sense(Sense::click()),
                                    );
                                    if response.clicked() {
                                        cx.send(Message::ShowCommit(parent.clone()));
                                    }
                                }
                            },
                        );
                        if detail.committer != detail.author || detail.commit_time != detail.time {
                            fact(ui, cx, "Committer", |ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(format!(
                                            "{}, {}",
                                            detail.committer,
                                            full_date(detail.commit_time)
                                        ))
                                        .size(12.0)
                                        .monospace(),
                                    )
                                    .wrap(),
                                );
                            });
                        }
                    });
            });
        hline(ui, look);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            heading(
                ui,
                look,
                &format!("{} changed", plural(detail.files.len(), "file")),
            );
        });
        ui.add_space(4.0);
        if detail.parents.len() > 1 {
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                muted(ui, look, "(compared to first parent)", 12.0);
            });
        }
        files(
            ui,
            cx,
            &detail.sha,
            &detail.files,
            tab.selected_file.as_deref(),
            Some(&detail.sha),
            "No files",
        );
    });
}

pub fn compare(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let Some(state) = &tab.compare else {
        return;
    };
    let subject = |sha: &str| {
        tab.commits
            .iter()
            .find(|commit| commit.sha == sha)
            .map_or_else(String::new, |commit| commit.subject.clone())
    };
    with_diff(ui, cx, tab, "Select a file to see its changes", |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
        egui::Frame::new()
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                text(ui, "Comparing commits", 13.0, c.text);
                for (prefix, sha) in [("", &state.base), ("→", &state.target)] {
                    ui.horizontal(|ui| {
                        muted(ui, look, prefix, 12.0);
                        mono(ui, short_sha(sha), 12.0, c.text);
                        muted(ui, look, subject(sha), 12.0);
                    });
                }
                if let Some(result) = &state.result {
                    muted(
                        ui,
                        look,
                        format!(
                            "{} only in target, {} only in base",
                            plural(result.target_only as usize, "commit"),
                            result.base_only
                        ),
                        12.0,
                    );
                }
                if let Some(error) = &state.error {
                    text(ui, error.as_str(), 12.0, c.danger);
                }
                ui.horizontal(|ui| {
                    if btn(ui, look, "Swap", true) {
                        cx.send(Message::CompareSwap);
                    }
                    if btn(ui, look, "Close", true) {
                        cx.send(Message::CompareClose);
                    }
                });
            });
        hline(ui, look);
        let list = state
            .result
            .as_ref()
            .map_or(&[][..], |result| &result.files);
        files(
            ui,
            cx,
            "compare",
            list,
            state.selected.as_deref(),
            Some(&state.target),
            "No differences",
        );
    });
}

pub fn stash(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let found = tab.stash.as_ref().and_then(|open| {
        tab.refs
            .stashes
            .iter()
            .find(|stash| stash.name == open.name)
            .map(|stash| (open, stash))
    });
    let Some((open, stash)) = found else {
        placeholder(ui, look, "Stash not found");
        return;
    };
    with_diff(ui, cx, tab, "Select a file to see its changes", |ui| {
        egui::Frame::new()
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                text(ui, stash.message.as_str(), 13.0, look.c.text);
                muted(
                    ui,
                    look,
                    format!("{} · {}", stash.name, full_date(stash.time)),
                    12.0,
                );
                ui.horizontal(|ui| {
                    if primary(ui, look, "Apply", true) {
                        cx.send(Message::Run(Command::StashApply {
                            name: stash.name.clone(),
                            pop: false,
                        }));
                    }
                    if btn(ui, look, "Pop", true) {
                        cx.send(Message::Run(Command::StashApply {
                            name: stash.name.clone(),
                            pop: true,
                        }));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if danger_text(ui, look, "Drop…", true) {
                            cx.send(Message::Action(Action::DropStash(stash.name.clone())));
                        }
                    });
                });
            });
        hline(ui, look);
        files(
            ui,
            cx,
            "stash",
            &open.files,
            open.selected.as_deref(),
            None,
            "No files",
        );
    });
}
