//! Full-window tools: reflog, blame, file history, conflict resolver, interactive rebase, settings.

use egui::{
    Align, Align2, Color32, Context, Frame, Id, Layout, Margin, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use gitcraft_core::model::{BlameLine, Commit, ConflictChunk, ReflogEntry};

use super::widgets::{
    Kind, area_edit, bar, btn, button_tip, card, check, choice, fill, floating, line_edit, muted,
    paint_text, placeholder, primary, region, scroll, segmented, split_x, split_y, text,
    text_width, toggle,
};
use super::{Cx, diff, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{
    BLAME_ROW_HEIGHT, ConflictState, DiffPane, Message, Overlay, PaneKey, REBASE_ROW_HEIGHT,
    RebaseState,
};
use crate::logic::{Choice, has_conflict_markers, plural, short_date, short_sha};

const REBASE_ACTIONS: [(&str, &str, &str); 6] = [
    ("pick", "p", "Use the commit"),
    ("reword", "r", "Use the commit, edit its message"),
    ("edit", "e", "Stop to amend the commit"),
    (
        "squash",
        "s",
        "Meld into the commit above, combine messages",
    ),
    (
        "fixup",
        "f",
        "Meld into the commit above, discard this message",
    ),
    ("drop", "d", "Remove the commit"),
];

/// The frame of a large tool: a title bar with a Close button, and room for `body`.
fn dialog(
    ctx: &Context,
    cx: &Cx,
    title: &str,
    subtitle: &str,
    extra: Option<&dyn Fn(&mut Ui)>,
    body: impl FnOnce(&mut Ui, Rect),
) {
    let look = cx.look();
    let c = &look.c;
    let window = cx.app.window;
    let (width, height) = ((window.x - 60.0).max(300.0), (window.y - 50.0).max(200.0));
    egui::Modal::new(Id::unique("overlay"))
        .backdrop_color(c.backdrop)
        .frame(floating(look, 12).inner_margin(Margin::ZERO))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
            let (_, rect) = ui.allocate_space(vec2(width, height));
            let (head, rest) = split_y(rect, 66.0);
            region(ui, head, |ui| {
                bar(ui, 66.0, Color32::TRANSPARENT, 20.0, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        // Centre the title and subtitle in the header.
                        ui.add_space(if subtitle.is_empty() { 18.0 } else { 9.0 });
                        text(ui, title, 22.0, c.text);
                        if !subtitle.is_empty() {
                            muted(ui, look, subtitle, look.text_size);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if btn(ui, look, "Close", true) {
                            cx.send(Message::OverlayClose);
                        }
                        if let Some(extra) = extra {
                            extra(ui);
                        }
                    });
                });
            });
            let (line, content) = split_y(rest, 1.0);
            fill(ui, line, c.border);
            region(ui, content, |ui| body(ui, content));
        });
}

pub fn show(ctx: &Context, cx: &Cx, overlay: &Overlay) {
    match overlay {
        Overlay::Settings => settings(ctx, cx),
        Overlay::Reflog {
            entries,
            loading,
            error,
        } => reflog(ctx, cx, entries, *loading, error.as_deref()),
        Overlay::Blame {
            path,
            rev,
            lines,
            loading,
            error,
            hovered,
        } => blame(
            ctx,
            cx,
            path,
            rev.as_deref(),
            lines,
            *loading,
            error.as_deref(),
            hovered.as_deref(),
        ),
        Overlay::FileHistory {
            path,
            commits,
            selected,
            pane,
            loading,
            error,
        } => file_history(
            ctx,
            cx,
            path,
            commits,
            selected.as_deref(),
            pane,
            *loading,
            error.as_deref(),
        ),
        Overlay::Conflict(state) => conflict(ctx, cx, state),
        Overlay::Rebase(state) => rebase(ctx, cx, state),
    }
}

fn reflog(ctx: &Context, cx: &Cx, entries: &[ReflogEntry], loading: bool, error: Option<&str>) {
    let look = cx.look();
    dialog(
        ctx,
        cx,
        "Reflog",
        "Every position HEAD has been at. Right-click an entry for all commit actions.",
        None,
        |ui, _| {
            if loading {
                placeholder(ui, look, "Loading…");
            } else if let Some(error) = error {
                placeholder(ui, look, error);
            } else if entries.is_empty() {
                placeholder(ui, look, "The reflog is empty");
            } else {
                scroll("reflog").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for entry in entries {
                        reflog_row(ui, cx, entry);
                    }
                });
            }
        },
    );
}

fn reflog_row(ui: &mut Ui, cx: &Cx, entry: &ReflogEntry) {
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    if response.hovered() {
        fill(ui, rect, c.hover);
    }
    let mid = rect.center().y;
    let size = look.text_size;
    let buttons = 150.0;
    let flex = (rect.width() - 12.0 - 90.0 - 66.0 - 112.0 - buttons - 5.0 * 8.0).max(100.0);
    let (action_w, subject_w) = (flex * 0.4, flex * 0.6);
    let mut x = rect.left() + 12.0;
    let clipped = |x: f32, width: f32| {
        ui.painter().with_clip_rect(Rect::from_min_size(
            Pos2::new(x, rect.top()),
            vec2(width, rect.height()),
        ))
    };
    paint_text(
        &clipped(x, 90.0),
        Pos2::new(x, mid),
        &entry.selector,
        look.mono(size),
        c.text,
    );
    x += 98.0;
    paint_text(
        &clipped(x, 66.0),
        Pos2::new(x, mid),
        short_sha(&entry.sha),
        look.mono(size),
        c.text,
    );
    x += 74.0;
    paint_text(
        &clipped(x, action_w),
        Pos2::new(x, mid),
        &entry.action,
        look.ui(size),
        c.text_muted,
    );
    x += action_w + 8.0;
    paint_text(
        &clipped(x, subject_w),
        Pos2::new(x, mid),
        &entry.subject,
        look.ui(size),
        c.text,
    );
    x += subject_w + 8.0;
    paint_text(
        &clipped(x, 112.0),
        Pos2::new(x, mid),
        &short_date(entry.time),
        look.ui(size),
        c.text,
    );
    let slot = Rect::from_min_max(
        Pos2::new(rect.right() - buttons - 8.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    let mut pressed = false;
    region(ui, slot, |ui| {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            if btn(ui, look, "Show", true) {
                cx.send(Message::ShowCommit(entry.sha.clone()));
                pressed = true;
            }
            if btn(ui, look, "Branch…", true) {
                cx.send(Message::Action(Action::NewBranch {
                    start: Some(entry.sha.clone()),
                    label: None,
                }));
                pressed = true;
            }
        });
    });
    if pressed {
        return;
    }
    if response.double_clicked() {
        cx.send(Message::ShowCommit(entry.sha.clone()));
    } else if response.secondary_clicked() {
        cx.send(Message::OpenMenu(MenuRequest::Commit {
            sha: entry.sha.clone(),
            focus: None,
        }));
    }
}

#[allow(clippy::too_many_arguments)]
fn blame(
    ctx: &Context,
    cx: &Cx,
    path: &str,
    rev: Option<&str>,
    lines: &[BlameLine],
    loading: bool,
    error: Option<&str>,
    hovered: Option<&str>,
) {
    let look = cx.look();
    let subtitle = match rev {
        Some(rev) => format!("{path} @ {}", short_sha(rev)),
        None => path.to_owned(),
    };
    let extra = |ui: &mut Ui| {
        if btn(ui, look, "File history", true) {
            cx.send(Message::OpenFileHistory(path.to_owned()));
        }
    };
    dialog(ctx, cx, "Blame", &subtitle, Some(&extra), |ui, _| {
        if loading {
            placeholder(ui, look, "Loading…");
            return;
        }
        if let Some(error) = error {
            placeholder(ui, look, error);
            return;
        }
        let (min, max) = lines.iter().fold((i64::MAX, i64::MIN), |(lo, hi), line| {
            (lo.min(line.time), hi.max(line.time))
        });
        let span = (max - min).max(1) as f32;
        scroll("blame").show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let width = ui.max_rect().width();
            ui.set_height(lines.len() as f32 * BLAME_ROW_HEIGHT);
            let first = ((viewport.min.y / BLAME_ROW_HEIGHT) as usize).saturating_sub(6);
            let end = (((viewport.max.y / BLAME_ROW_HEIGHT).ceil() as usize) + 6).min(lines.len());
            for at in first..end {
                let rect = Rect::from_min_size(
                    origin + vec2(0.0, at as f32 * BLAME_ROW_HEIGHT),
                    vec2(width, BLAME_ROW_HEIGHT),
                );
                blame_row(ui, cx, rect, lines, at, min, span, hovered);
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn blame_row(
    ui: &mut Ui,
    cx: &Cx,
    rect: Rect,
    lines: &[BlameLine],
    at: usize,
    min: i64,
    span: f32,
    hovered: Option<&str>,
) {
    let look = cx.look();
    let c = &look.c;
    let line = &lines[at];
    let committed = line.sha.bytes().any(|b| b != b'0');
    let response = ui.interact(rect, ui.scope_id().with(("blame", at)), Sense::click());
    if hovered == Some(line.sha.as_str()) {
        fill(ui, rect, c.hover);
    }
    if response.hovered() && hovered != Some(line.sha.as_str()) {
        cx.send(Message::BlameHover(line.sha.clone()));
    }
    let age = (line.time - min) as f32 / span;
    ui.painter().rect_filled(
        Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
        0.0,
        c.accent.gamma_multiply(0.15 + age * 0.85),
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let mid = rect.center().y;
    let starts = at == 0 || lines[at - 1].sha != line.sha;
    if starts {
        paint_text(
            &painter,
            Pos2::new(rect.left() + 7.0, mid),
            short_sha(&line.sha),
            look.mono(12.0),
            c.text,
        );
        let author = painter.with_clip_rect(Rect::from_min_size(
            Pos2::new(rect.left() + 71.0, rect.top()),
            vec2(110.0, rect.height()),
        ));
        paint_text(
            &author,
            Pos2::new(rect.left() + 71.0, mid),
            &line.author,
            look.ui(12.0),
            c.text,
        );
        paint_text(
            &painter,
            Pos2::new(rect.left() + 185.0, mid),
            &short_date(line.time),
            look.ui(12.0),
            c.text,
        );
    }
    paint_text(
        &painter,
        Pos2::new(rect.left() + 297.0, mid),
        &line.line.to_string(),
        look.mono(12.0),
        c.text_muted,
    );
    paint_text(
        &painter,
        Pos2::new(rect.left() + 345.0, mid),
        &line.text,
        look.mono(13.0),
        c.text,
    );
    // All zeros means the line isn't committed yet, so there's no commit to show.
    if committed && response.clicked() {
        cx.send(Message::ShowCommit(line.sha.clone()));
    }
}

#[allow(clippy::too_many_arguments)]
fn file_history(
    ctx: &Context,
    cx: &Cx,
    path: &str,
    commits: &[Commit],
    selected: Option<&str>,
    pane: &DiffPane,
    loading: bool,
    error: Option<&str>,
) {
    let look = cx.look();
    let c = &look.c;
    dialog(ctx, cx, "File history", path, None, |ui, rect| {
        let (list, right) = split_x(rect, 360.0);
        region(ui, list, |ui| {
            scroll("file_history").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if loading {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        muted(ui, look, "Loading…", look.text_size);
                    });
                } else if let Some(error) = error {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        text(ui, error, look.text_size, c.text);
                    });
                }
                for commit in commits {
                    history_item(ui, cx, commit, selected == Some(commit.sha.as_str()));
                }
            });
        });
        let current = selected.and_then(|sha| commits.iter().find(|commit| commit.sha == sha));
        let actions = |ui: &mut Ui| {
            let Some(commit) = current else { return };
            let target = commit
                .file
                .as_ref()
                .map_or(path, |file| file.path.as_str())
                .to_owned();
            let can_blame = commit.file.as_ref().is_none_or(|file| file.status != "D");
            if btn(ui, look, "Blame at this commit", can_blame) {
                cx.send(Message::OpenBlame(target, Some(commit.sha.clone())));
            }
            if btn(ui, look, "Show in history", true) {
                cx.send(Message::ShowCommit(commit.sha.clone()));
            }
        };
        region(ui, right, |ui| {
            diff::view(
                ui,
                cx,
                pane,
                PaneKey::Overlay,
                None,
                "Select a commit",
                Some(&actions),
                right.width(),
            );
        });
        ui.painter().vline(
            right.left() - 0.5,
            right.y_range(),
            Stroke::new(1.0, c.border),
        );
    });
}

fn history_item(ui: &mut Ui, cx: &Cx, commit: &Commit, chosen: bool) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let status = commit.file.as_ref().map_or("", |file| file.status.as_str());
    let orig = commit
        .file
        .as_ref()
        .and_then(|file| file.orig_path.as_ref());
    let height = if orig.is_some() { 56.0 } else { 42.0 };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if chosen {
        fill(ui, rect, c.selection);
    } else if response.hovered() {
        fill(ui, rect, c.hover);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    paint_text(
        &painter,
        Pos2::new(rect.left() + 10.0, rect.top() + 14.0),
        &commit.subject,
        look.ui(size),
        c.text,
    );
    paint_text(
        &painter,
        Pos2::new(rect.left() + 10.0, rect.top() + 30.0),
        &format!(
            "{status}  {} · {} · {}",
            commit.author,
            short_date(commit.time),
            short_sha(&commit.sha)
        ),
        look.ui(size - 1.0),
        c.text_muted,
    );
    if let Some(orig) = orig {
        paint_text(
            &painter,
            Pos2::new(rect.left() + 10.0, rect.top() + 45.0),
            &format!("renamed from {orig}"),
            look.ui(size - 1.0),
            c.text_muted,
        );
    }
    if response.clicked() {
        cx.send(Message::FileHistorySelect(commit.sha.clone()));
    }
}

fn conflict(ctx: &Context, cx: &Cx, state: &ConflictState) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    dialog(
        ctx,
        cx,
        "Resolve conflict",
        &state.path,
        None,
        |ui, rect| {
            if let Some(error) = &state.error {
                placeholder(ui, look, error);
                return;
            }
            let Some(data) = &state.data else {
                placeholder(ui, look, "Loading…");
                return;
            };
            let resolve = |side: Option<&str>| {
                Message::Action(Action::ConflictResolve {
                    path: state.path.clone(),
                    side: side.map(str::to_owned),
                })
            };
            let count = state.conflict_count();
            let footer = 46.0;
            let toolbar = 36.0;
            let flexible = (rect.height() - toolbar - footer - 3.0).max(60.0);
            let (top, rest) = split_y(rect, toolbar);
            let (line1, rest) = split_y(rest, 1.0);
            let (chunks_area, rest) = split_y(rest, flexible * 0.6);
            let (line2, rest) = split_y(rest, 1.0);
            let (result_area, rest) = split_y(rest, flexible * 0.4);
            let (line3, footer_area) = split_y(rest, 1.0);
            for line in [line1, line2, line3] {
                fill(ui, line, c.border);
            }

            region(ui, top, |ui| {
                bar(ui, toolbar, Color32::TRANSPARENT, 12.0, |ui| {
                    let mut summary = plural(count, "conflict");
                    if count > 0 {
                        summary.push_str(&format!(" · {} unresolved", state.unresolved()));
                    }
                    text(ui, summary, size, c.text);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if btn(ui, look, "Use their file", true) {
                            cx.send(resolve(Some("theirs")));
                        }
                        if btn(ui, look, "Use our file", true) {
                            cx.send(resolve(Some("ours")));
                        }
                        if count > 0 {
                            if btn(ui, look, "All theirs", true) {
                                cx.send(Message::ConflictChooseAll(Choice::Theirs));
                            }
                            if btn(ui, look, "All ours", true) {
                                cx.send(Message::ConflictChooseAll(Choice::Ours));
                            }
                        }
                    });
                });
            });

            region(ui, chunks_area, |ui| {
                scroll("conflict_chunks").show(ui, |ui| {
                Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = vec2(6.0, 10.0);
                    if count == 0 {
                        let note = if data.working.is_none() {
                            "The file was deleted on one side. Keep one side, or mark it as resolved to keep the current state."
                        } else {
                            "No conflict markers found (binary file, or the markers were already removed). Edit the result below if needed, then mark the file as resolved."
                        };
                        muted(ui, look, note, size);
                        if btn(ui, look, "Mark as resolved", true) {
                            cx.send(resolve(None));
                        }
                    }
                    let mut index = 0;
                    for (at, chunk) in data.chunks.iter().enumerate() {
                        let ConflictChunk::Conflict { ours, theirs, .. } = chunk else {
                            continue;
                        };
                        let choice = state.choices.get(index).copied().flatten();
                        let which = index;
                        index += 1;
                        let before = at.checked_sub(1).and_then(|before| data.chunks.get(before));
                        conflict_card(ui, cx, data, which, choice, before, ours, theirs);
                    }
                });
            });
            });

            region(ui, result_area, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        ui.horizontal(|ui| {
                            text(ui, "Result", size, c.text);
                            if state.edited {
                                muted(ui, look, "· edited by hand", size);
                            }
                            if has_conflict_markers(&state.result) {
                                text(ui, "· contains conflict markers", size, c.warning);
                            }
                        });
                        let height = (result_area.height() - 40.0).max(40.0);
                        if let Some(edited) = area_edit(
                            ui,
                            "conflict_result",
                            &state.result,
                            "",
                            height,
                            look.mono(12.5),
                        ) {
                            cx.send(Message::ConflictEdit(edited));
                        }
                    });
            });

            region(ui, footer_area, |ui| {
                bar(ui, footer, Color32::TRANSPARENT, 12.0, |ui| {
                    muted(
                        ui,
                        look,
                        "Saving writes the result to the file and stages it.",
                        size,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if primary(ui, look, "Save & mark resolved", data.working.is_some()) {
                            cx.send(Message::ConflictSave);
                        }
                        if btn(ui, look, "Cancel", true) {
                            cx.send(Message::OverlayClose);
                        }
                    });
                });
            });
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn conflict_card(
    ui: &mut Ui,
    cx: &Cx,
    data: &gitcraft_core::model::ConflictVersions,
    which: usize,
    choice: Option<Choice>,
    before: Option<&ConflictChunk>,
    ours: &[String],
    theirs: &[String],
) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    card(look).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = vec2(4.0, 6.0);
        ui.horizontal(|ui| {
            text(ui, format!("Conflict {}", which + 1), size, c.text);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (label, option) in [
                    ("Theirs + ours", Choice::TheirsOurs),
                    ("Ours + theirs", Choice::OursTheirs),
                    ("Theirs", Choice::Theirs),
                    ("Ours", Choice::Ours),
                ] {
                    if toggle(ui, look, label, choice == Some(option)) {
                        cx.send(Message::ConflictChoose(which, option));
                    }
                }
            });
        });
        if let Some(ConflictChunk::Common { lines }) = before {
            let tail: Vec<String> = lines.iter().rev().take(3).rev().cloned().collect();
            if !tail.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(tail.join("\n"))
                            .size(12.5)
                            .monospace()
                            .color(c.text_muted),
                    )
                    .truncate(),
                );
            }
        }
        let taken = |ours_side: bool| match choice {
            Some(Choice::Ours) => ours_side,
            Some(Choice::Theirs) => !ours_side,
            Some(_) => true,
            None => false,
        };
        ui.columns(2, |columns| {
            for (column, label, lines, tint, picked) in [
                (0, &data.ours_label, ours, c.conflict_ours, taken(true)),
                (
                    1,
                    &data.theirs_label,
                    theirs,
                    c.conflict_theirs,
                    taken(false),
                ),
            ] {
                let ui = &mut columns[column];
                Frame::new()
                    .fill(tint)
                    .stroke(Stroke::new(1.0, if picked { c.accent } else { c.border }))
                    .corner_radius(6)
                    .inner_margin(Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 3.0;
                        muted(ui, look, label.as_str(), size - 1.0);
                        let joined = lines.join("\n");
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(if joined.is_empty() {
                                    "(empty)".to_owned()
                                } else {
                                    joined
                                })
                                .size(12.5)
                                .monospace(),
                            )
                            .wrap(),
                        );
                    });
            }
        });
    });
}

fn rebase(ctx: &Context, cx: &Cx, state: &RebaseState) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let branch = cx
        .app
        .current()
        .and_then(|tab| tab.current_branch())
        .unwrap_or("HEAD");
    let subtitle = format!(
        "Rewrite {branch} on top of {}. Commits are applied from top to bottom.",
        state.base_label
    );
    dialog(
        ctx,
        cx,
        "Interactive rebase",
        &subtitle,
        None,
        |ui, rect| {
            let problem = state.problem().filter(|_| !state.loading);
            let legend_height = 3.0 * 20.0 + 18.0;
            let footer = 46.0;
            let (list, rest) = split_y(
                rect,
                (rect.height() - legend_height - footer - 2.0).max(40.0),
            );
            let (line1, rest) = split_y(rest, 1.0);
            let (legend, rest) = split_y(rest, legend_height);
            let (line2, footer_area) = split_y(rest, 1.0);
            fill(ui, line1, c.border);
            fill(ui, line2, c.border);

            region(ui, list, |ui| {
                if state.loading {
                    placeholder(ui, look, "Loading commits…");
                } else if let Some(error) = &state.error {
                    placeholder(ui, look, error);
                } else if state.steps.is_empty() {
                    placeholder(ui, look, "No commits between the base and HEAD.");
                } else {
                    scroll("rebase_steps").show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        for (index, step) in state.steps.iter().enumerate() {
                            rebase_row(ui, cx, state, index, step);
                        }
                    });
                }
            });

            // The keys, in two columns, painted at fixed positions so that nothing is cut off.
            let column = legend.width() / 2.0;
            for (index, (action, key, hint)) in REBASE_ACTIONS.into_iter().enumerate() {
                let at = Pos2::new(
                    legend.left() + 14.0 + (index / 3) as f32 * column,
                    legend.top() + 16.0 + (index % 3) as f32 * 20.0,
                );
                let width = paint_text(
                    ui.painter(),
                    at,
                    &key.to_uppercase(),
                    look.mono(size - 1.0),
                    c.accent,
                );
                paint_text(
                    ui.painter(),
                    at + vec2(width + 10.0, 0.0),
                    &format!("{action}: {hint}"),
                    look.ui(size - 1.0),
                    c.text_muted,
                );
            }

            region(ui, footer_area, |ui| {
                bar(ui, footer, Color32::TRANSPARENT, 12.0, |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if let Some(on) = check(
                        ui,
                        state.autostash,
                        "Stash local changes automatically",
                        true,
                    ) {
                        cx.send(Message::RebaseAutostash(on));
                    }
                    ui.label("").on_hover_text(
                        "Uncommitted changes are stashed before the rebase and put back after it.",
                    );
                    text(ui, problem.unwrap_or(""), size, c.danger);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ready = problem.is_none() && !state.loading;
                        if button_tip(
                            ui,
                            look,
                            "Start rebase",
                            Kind::Primary,
                            ready,
                            problem.unwrap_or("Rewrite the history as planned above"),
                        ) {
                            cx.send(Message::RebaseStart);
                        }
                        if button_tip(
                            ui,
                            look,
                            "Cancel",
                            Kind::Normal,
                            true,
                            "Close without changing anything",
                        ) {
                            cx.send(Message::OverlayClose);
                        }
                        if button_tip(
                            ui,
                            look,
                            "Reset",
                            Kind::Normal,
                            !state.unchanged(),
                            "Restore the original order and pick every commit",
                        ) {
                            cx.send(Message::RebaseReset);
                        }
                    });
                });
            });
        },
    );
}

fn rebase_row(
    ui: &mut Ui,
    cx: &Cx,
    state: &RebaseState,
    index: usize,
    step: &crate::app::RebaseStepState,
) {
    let look = cx.look();
    let c = &look.c;
    let size = look.text_size;
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), REBASE_ROW_HEIGHT),
        Sense::click(),
    );
    if state.focused == index {
        fill(ui, rect, c.selection);
    }
    let mid = rect.center().y;

    // The grip: drag it up or down to move the commit.
    let grip_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 22.0, mid),
        vec2(20.0, REBASE_ROW_HEIGHT),
    );
    let grip = ui
        .interact(grip_rect, response.id.with("grip"), Sense::drag())
        .on_hover_text("Drag up or down to move this commit (or Alt + ↑ / ↓)");
    icon::paint(
        ui,
        "grip",
        Rect::from_center_size(grip_rect.center(), vec2(14.0, 14.0)),
        c.text_muted,
    );
    let drag_key = response.id.with("dragged");
    if grip.hovered() || grip.dragged() {
        ui.ctx().set_cursor_icon(if grip.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    if grip.drag_started() {
        cx.send(Message::RebaseFocus(index));
    }
    if grip.dragged() {
        ui.data_mut(|data| {
            let total = data.get_temp::<f32>(drag_key).unwrap_or(0.0) + grip.drag_delta().y;
            data.insert_temp(drag_key, total);
        });
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            4.0,
            Stroke::new(1.5, c.accent),
            StrokeKind::Inside,
        );
    }
    if grip.drag_stopped() {
        let total = ui.data_mut(|data| data.remove_temp::<f32>(drag_key).unwrap_or(0.0));
        let rows = (total / REBASE_ROW_HEIGHT).round() as isize;
        if rows != 0 {
            cx.send(Message::RebaseMove(index, rows));
        }
    }

    // The actions.
    let mut x = rect.left() + 40.0;
    for (action, key, hint) in REBASE_ACTIONS {
        let label = key.to_uppercase();
        let width = text_width(ui, &label, &look.mono(size - 2.0)) + 12.0;
        let button = Rect::from_min_size(Pos2::new(x, mid - 10.0), vec2(width, 20.0));
        let on = step.action == action;
        let response = ui
            .interact(button, response.id.with(("action", action)), Sense::click())
            .on_hover_text(format!("{}: {hint}\nShortcut: {key}", capitalised(action)));
        let fill_color = if on {
            c.accent
        } else if response.hovered() {
            c.hover_strong
        } else {
            c.panel
        };
        ui.painter().rect_filled(button, 5.0, fill_color);
        if !on {
            ui.painter()
                .rect_stroke(button, 5.0, Stroke::new(1.0, c.border), StrokeKind::Inside);
        }
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            &label,
            look.mono(size - 2.0),
            if on { Color32::WHITE } else { c.text },
        );
        if response.clicked() {
            cx.send(Message::RebaseSetAction(index, action));
        }
        x += width + 2.0;
    }
    x += 10.0;
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    paint_text(
        &painter,
        Pos2::new(x, mid),
        short_sha(&step.commit.sha),
        look.mono(size),
        c.text,
    );
    ui.interact(
        Rect::from_min_size(Pos2::new(x, rect.top()), vec2(70.0, rect.height())),
        response.id.with("sha"),
        Sense::hover(),
    )
    .on_hover_text(step.commit.sha.as_str());
    x += 74.0;

    let right = rect.right() - 12.0 - 2.0 * 26.0 - 2.0 * 8.0;
    let author_x = right - 140.0;
    let subject_width = (author_x - 8.0 - x).max(40.0);
    let subject_area =
        Rect::from_min_size(Pos2::new(x, rect.top()), vec2(subject_width, rect.height()));
    if step.action != "reword" {
        ui.interact(subject_area, response.id.with("subject"), Sense::hover())
            .on_hover_text(step.commit.subject.as_str());
    }
    ui.interact(
        Rect::from_min_size(Pos2::new(author_x, rect.top()), vec2(140.0, rect.height())),
        response.id.with("author"),
        Sense::hover(),
    )
    .on_hover_text(format!("{} <{}>", step.commit.author, step.commit.email));
    if step.action == "reword" {
        region(
            ui,
            Rect::from_center_size(subject_area.center(), vec2(subject_width, 26.0)),
            |ui| {
                let (_, changed) = line_edit(
                    ui,
                    ("rebase_message", index),
                    &step.message,
                    "Commit message",
                    subject_width,
                    look.ui(size),
                );
                if let Some(message) = changed {
                    cx.send(Message::RebaseMessage(index, message));
                }
            },
        );
    } else {
        let color = if step.action == "drop" {
            c.text_muted
        } else {
            c.text
        };
        paint_text(
            &painter.with_clip_rect(subject_area),
            Pos2::new(x, mid),
            &step.commit.subject,
            look.ui(size),
            color,
        );
    }
    paint_text(
        &painter.with_clip_rect(Rect::from_min_size(
            Pos2::new(author_x, rect.top()),
            vec2(140.0, rect.height()),
        )),
        Pos2::new(author_x, mid),
        &step.commit.author,
        look.ui(size),
        c.text,
    );
    let ups = Rect::from_center_size(Pos2::new(right + 21.0, mid), vec2(26.0, 26.0));
    let downs = Rect::from_center_size(Pos2::new(right + 21.0 + 34.0, mid), vec2(26.0, 26.0));
    for (area, name, delta, enabled) in [
        (ups, "arrow-up", -1, index > 0),
        (downs, "arrow-down", 1, index + 1 < state.steps.len()),
    ] {
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui
            .interact(area, response.id.with(name), sense)
            .on_hover_text(if delta < 0 {
                "Move this commit up (Alt + ↑)"
            } else {
                "Move this commit down (Alt + ↓)"
            });
        super::widgets::icon_in(ui, look, &response, area, name, 14.0, enabled);
        if enabled && response.clicked() {
            cx.send(Message::RebaseMove(index, delta));
        }
    }
    if response.clicked() {
        cx.send(Message::RebaseFocus(index));
    }
}

fn capitalised(word: &str) -> String {
    let mut letters = word.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

const THEMES: [(&str, &str); 3] = [
    ("system", "Follow system"),
    ("light", "Light"),
    ("dark", "Dark"),
];

const SHORTCUTS: [(&str, &str); 8] = [
    ("Ctrl/⌘ + 1 / 2", "All commits / Local changes"),
    ("Ctrl/⌘ + F", "Search commits"),
    ("Ctrl/⌘ + T or O", "Open repository"),
    ("Ctrl/⌘ + W", "Close tab"),
    ("Ctrl/⌘ + Enter", "Commit"),
    ("Ctrl/⌘ + + / − / 0", "Zoom"),
    ("F5 / Ctrl/⌘ + R", "Refresh repository"),
    ("↑ ↓ PgUp PgDn", "Move through commits or files"),
];

/// A titled group of settings.
fn group(ui: &mut Ui, cx: &Cx, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .size(15.0)
            .strong()
            .color(cx.c().text),
    );
    ui.add_space(2.0);
    add(ui);
}

/// Settings: a small window of its own, with egui's native controls in aligned rows.
fn settings(ctx: &Context, cx: &Cx) {
    use egui::{DragValue, Grid, RichText};
    let look = cx.look();
    let c = &look.c;
    let values = &cx.app.settings;
    let window = cx.app.window;
    let response = egui::Modal::new(Id::unique("settings"))
        .backdrop_color(c.backdrop)
        .frame(floating(look, 12).inner_margin(Margin::symmetric(24, 20)))
        .show(ctx, |ui| {
            ui.set_width(460.0);
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").size(22.0).color(c.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, look, "Done", true) {
                        cx.send(Message::OverlayClose);
                    }
                });
            });
            ui.add_space(4.0);
            ui.separator();
            // Always show the bar, so that it is clear there is more below.
            let bars = &mut ui.spacing_mut().scroll;
            bars.dormant_handle_opacity = bars.active_handle_opacity;
            bars.dormant_background_opacity = bars.active_background_opacity;
            scroll("settings")
                .auto_shrink([false, true])
                .max_height((window.y * 0.85 - 110.0).max(160.0))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    group(ui, cx, "Appearance", |ui| {
                        Grid::new("appearance")
                            .num_columns(2)
                            .spacing([24.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Theme");
                                let options: Vec<(String, String)> = THEMES
                                    .iter()
                                    .map(|(value, label)| {
                                        ((*value).to_owned(), (*label).to_owned())
                                    })
                                    .collect();
                                if let Some(chosen) =
                                    choice(ui, "theme", &options, &values.theme, 170.0)
                                {
                                    cx.send(Message::SetTheme(chosen));
                                }
                                ui.end_row();
                            });
                    });
                    group(ui, cx, "Diffs", |ui| {
                        Grid::new("diffs")
                            .num_columns(2)
                            .spacing([24.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Layout");
                                if let Some(chosen) = segmented(
                                    ui,
                                    look,
                                    ["Unified", "Side by side"],
                                    usize::from(values.diff_split),
                                ) {
                                    cx.send(Message::SetSplit(chosen == 1));
                                }
                                ui.end_row();
                                ui.label("Context lines");
                                let mut lines = values.diff_context;
                                ui.add(DragValue::new(&mut lines).range(0..=100));
                                if lines != values.diff_context {
                                    cx.send(Message::SetContext(lines.to_string()));
                                }
                                ui.end_row();
                            });
                        if let Some(on) = check(
                            ui,
                            values.ignore_whitespace,
                            "Ignore whitespace changes",
                            true,
                        ) {
                            cx.send(Message::SetIgnoreWhitespace(on));
                        }
                        if let Some(on) =
                            check(ui, values.syntax_highlight, "Syntax highlighting", true)
                        {
                            cx.send(Message::SetSyntax(on));
                        }
                    });
                    group(ui, cx, "Repository", |ui| {
                        Grid::new("repository")
                            .num_columns(2)
                            .spacing([24.0, 8.0])
                            .show(ui, |ui| {
                                ui.label("Commits loaded at a time");
                                let mut limit = values.log_limit;
                                ui.add(DragValue::new(&mut limit).range(100..=100_000).speed(50));
                                if limit != values.log_limit {
                                    cx.send(Message::SetLogLimit(limit.to_string()));
                                }
                                ui.end_row();
                            });
                        if let Some(on) = check(
                            ui,
                            values.fetch_on_open,
                            "Fetch when opening a repository",
                            true,
                        ) {
                            cx.send(Message::SetFetchOnOpen(on));
                        }
                        if let Some(on) = check(
                            ui,
                            values.commit_graph,
                            "Keep a commit-graph file for faster history",
                            true,
                        ) {
                            cx.send(Message::SetCommitGraph(on));
                        }
                    });
                    group(ui, cx, "Keyboard shortcuts", |ui| {
                        Grid::new("shortcuts")
                            .num_columns(2)
                            .spacing([24.0, 6.0])
                            .show(ui, |ui| {
                                for (keys, what) in SHORTCUTS {
                                    ui.label(RichText::new(keys).monospace().size(12.0));
                                    muted(ui, look, what, look.text_size);
                                    ui.end_row();
                                }
                            });
                    });
                });
        });
    if response.backdrop_response.clicked() {
        cx.send(Message::OverlayClose);
    }
}
