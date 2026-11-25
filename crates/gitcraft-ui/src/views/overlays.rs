//! Full-window tools: reflog, blame, file history, conflict resolver, interactive rebase, settings.

use egui::{
    Align, Align2, Color32, Context, Frame, Id, Layout, Margin, Pos2, Rect, Sense, Stroke,
    StrokeKind, Ui, vec2,
};
use gitcraft_core::model::{
    BlameLine, CleanupCandidate, Commit, ConflictChunk, DiscardBackup, LfsLock, ReflogEntry,
};

use super::widgets::{
    Kind, accent, area_edit, bar, btn, button_tip, card, check, choice, fill, floating, line_edit,
    muted, paint_text, placeholder, region, scroll, segmented, split_x, split_y, text, toggle,
};
use super::{Cx, diff, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{
    BLAME_ROW_HEIGHT, ConflictState, DiffPane, Message, Overlay, PaneKey, REBASE_ROW_HEIGHT,
    RebaseState, SettingsPage,
};
use crate::command::Command;
use crate::i18n::{tr, trf};
use crate::logic::{Choice, full_date, has_conflict_markers, plural, short_date, short_sha};

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
                        text(ui, tr(title), 22.0, c.text);
                        if !subtitle.is_empty() {
                            // As given: it may be a path, which must not be translated.
                            muted(ui, look, subtitle, look.text_size);
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if accent(ui, look, "Close", true) {
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
        Overlay::About { git } => about(ctx, cx, git.as_ref()),
        Overlay::Reflog {
            all,
            entries,
            loading,
            error,
        } => reflog(ctx, cx, *all, entries, *loading, error.as_deref()),
        Overlay::Discards {
            entries,
            loading,
            error,
        } => discards(ctx, cx, entries, *loading, error.as_deref()),
        Overlay::LfsLocks {
            locks,
            loading,
            error,
            ..
        } => lfs_locks(ctx, cx, locks, *loading, error.as_deref()),
        Overlay::Blame {
            path,
            rev,
            lines,
            loading,
            error,
            hovered,
            ignore_revs,
            ignored,
        } => blame(
            ctx,
            cx,
            path,
            rev.as_deref(),
            lines,
            *loading,
            error.as_deref(),
            hovered.as_deref(),
            (*ignore_revs, *ignored),
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
        Overlay::Cleanup {
            candidates,
            chosen,
            remote_too,
            loading,
            error,
        } => cleanup(
            ctx,
            cx,
            candidates,
            chosen,
            *remote_too,
            *loading,
            error.as_deref(),
        ),
        Overlay::RangeDiff {
            old,
            new,
            text,
            loading,
            error,
        } => range_diff(ctx, cx, old, new, text, *loading, error.as_deref()),
        Overlay::Rebase(state) => rebase(ctx, cx, state),
    }
}

fn reflog(
    ctx: &Context,
    cx: &Cx,
    all: bool,
    entries: &[ReflogEntry],
    loading: bool,
    error: Option<&str>,
) {
    let look = cx.look();
    let extra = |ui: &mut Ui| {
        if let Some(all) = check(ui, all, "All branches", true) {
            cx.send(Message::ReflogAll(all));
        }
    };
    let subtitle = if all {
        tr(
            "Every position HEAD and each branch have been at. Right-click an entry for all commit actions.",
        )
    } else {
        tr("Every position HEAD has been at. Right-click an entry for all commit actions.")
    };
    dialog(ctx, cx, "Reflog", subtitle, Some(&extra), |ui, _| {
        if loading {
            placeholder(ui, look, "Loading…");
        } else if let Some(error) = error {
            placeholder(ui, look, error);
        } else if entries.is_empty() {
            placeholder(ui, look, "The reflog is empty");
        } else {
            let scrolled = scroll("reflog").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for entry in entries {
                    reflog_row(ui, cx, entry);
                }
            });
            super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn cleanup(
    ctx: &Context,
    cx: &Cx,
    candidates: &[CleanupCandidate],
    chosen: &[String],
    remote_too: bool,
    loading: bool,
    error: Option<&str>,
) {
    let look = cx.look();
    let c = &look.c;
    dialog(
        ctx,
        cx,
        "Clean up branches",
        tr("Local branches that look done with. Merged ones are ticked; look at the others first."),
        None,
        |ui, rect| {
            if loading {
                placeholder(ui, look, "Loading…");
                return;
            }
            if let Some(error) = error {
                placeholder(ui, look, error);
                return;
            }
            if candidates.is_empty() {
                placeholder(ui, look, "No branches to clean up");
                return;
            }
            let (list, footer) = split_y(rect, (rect.height() - 52.0).max(40.0));
            region(ui, list, |ui| {
                let scrolled = scroll("cleanup").show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(14, 6))
                        .show(ui, |ui| {
                            for candidate in candidates {
                                ui.horizontal(|ui| {
                                    let on = chosen.contains(&candidate.name);
                                    if check(ui, on, "", true).is_some() {
                                        cx.send(Message::CleanupToggle(candidate.name.clone()));
                                    }
                                    super::widgets::mono(
                                        ui,
                                        &candidate.name,
                                        look.text_size,
                                        c.text,
                                    );
                                    let (reason, color) = match candidate.reason.as_str() {
                                        "merged" => ("Merged", c.success),
                                        "squashed" => (
                                            "Its changes are in already (squashed or rebased)",
                                            c.warning,
                                        ),
                                        _ => ("Its upstream was deleted", c.text_muted),
                                    };
                                    text(ui, tr(reason), 12.0, color);
                                    muted(ui, look, short_date(candidate.time), 12.0);
                                    if let Some(upstream) = &candidate.upstream {
                                        muted(ui, look, format!("→ {upstream}"), 12.0);
                                    }
                                });
                            }
                        });
                });
                super::widgets::scroll_shadows(ui, look, &scrolled);
            });
            region(ui, footer, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if let Some(on) = check(
                                ui,
                                remote_too,
                                "Also delete their branches on the remote",
                                true,
                            ) {
                                cx.send(Message::CleanupRemote(on));
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let label = trf(
                                    "Delete {branches}…",
                                    &[("branches", &plural(chosen.len(), "branch"))],
                                );
                                if accent(ui, look, &label, !chosen.is_empty()) {
                                    cx.send(Message::CleanupStart);
                                }
                            });
                        });
                    });
            });
        },
    );
}

fn range_diff(
    ctx: &Context,
    cx: &Cx,
    old: &str,
    new: &str,
    content: &str,
    loading: bool,
    error: Option<&str>,
) {
    let look = cx.look();
    let c = &look.c;
    let subtitle = format!("{old}  →  {new}");
    dialog(
        ctx,
        cx,
        "Compare commit ranges",
        &subtitle,
        None,
        |ui, _| {
            if loading {
                placeholder(ui, look, "Loading…");
            } else if let Some(error) = error {
                placeholder(ui, look, error);
            } else if content.trim().is_empty() {
                placeholder(ui, look, "Both ranges hold the same commits");
            } else {
                let scrolled = scroll("range_diff").show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for line in content.lines() {
                                // Pair lines read "1:  abc = 1:  def subject"; = same, ! changed,
                                // < only in the old range, > only in the new one.
                                let inner = line.trim_start();
                                let color = if line.starts_with(' ') && inner.starts_with('+') {
                                    c.diff_add_text
                                } else if line.starts_with(' ') && inner.starts_with('-') {
                                    c.diff_del_text
                                } else if line.contains(" ! ") {
                                    c.warning
                                } else if line.contains(" < ") {
                                    c.danger
                                } else if line.contains(" > ") {
                                    c.success
                                } else if line.contains(" = ") {
                                    c.text_muted
                                } else {
                                    c.text
                                };
                                super::widgets::mono(ui, line, look.text_size, color);
                            }
                        });
                });
                super::widgets::scroll_shadows(ui, look, &scrolled);
            }
        },
    );
}

fn discards(ctx: &Context, cx: &Cx, entries: &[DiscardBackup], loading: bool, error: Option<&str>) {
    let look = cx.look();
    dialog(
        ctx,
        cx,
        "Recover discarded changes",
        tr(
            "Discarded changes are kept for 30 days (the last 50). Putting them back never overwrites what you changed since.",
        ),
        None,
        |ui, _| {
            if loading {
                placeholder(ui, look, "Loading…");
            } else if let Some(error) = error {
                placeholder(ui, look, error);
            } else if entries.is_empty() {
                placeholder(ui, look, "No discarded changes are kept");
            } else {
                let scrolled = scroll("discards").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for entry in entries {
                        discard_row(ui, cx, entry);
                    }
                });
                super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
            }
        },
    );
}

fn lfs_locks(ctx: &Context, cx: &Cx, locks: &[LfsLock], loading: bool, error: Option<&str>) {
    let look = cx.look();
    let reload = |ui: &mut Ui| {
        if btn(ui, look, "Refresh", !loading) {
            cx.send(Message::OpenLfsLocks);
        }
    };
    dialog(
        ctx,
        cx,
        "LFS locks",
        tr(
            "Files locked on the LFS server. Only whoever holds a lock can push changes to that file.",
        ),
        Some(&reload),
        |ui, _| {
            if let Some(error) = error {
                placeholder(ui, look, error);
            } else if locks.is_empty() {
                placeholder(
                    ui,
                    look,
                    if loading {
                        "Loading…"
                    } else {
                        "No files are locked"
                    },
                );
            } else {
                let scrolled = scroll("lfs-locks").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    for lock in locks {
                        lfs_lock_row(ui, cx, lock);
                    }
                });
                super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
            }
        },
    );
}

fn lfs_lock_row(ui: &mut Ui, cx: &Cx, lock: &LfsLock) {
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    if response.hovered() {
        fill(ui, rect, c.hover);
    }
    let mid = rect.center().y;
    let size = look.text_size;
    let buttons = 150.0;
    let owner_w = 220.0;
    let x = rect.left() + 12.0;
    let path_w = (rect.right() - buttons - owner_w - 24.0 - x).max(80.0);
    let painter = ui.painter().with_clip_rect(Rect::from_min_size(
        Pos2::new(x, rect.top()),
        vec2(path_w, rect.height()),
    ));
    paint_text(
        &painter,
        Pos2::new(x, mid),
        &lock.path,
        look.mono(size),
        c.text,
    );
    let owner = if lock.ours {
        trf("{owner} (you)", &[("owner", &lock.owner)])
    } else {
        lock.owner.clone()
    };
    // The server's time, such as 2026-01-02T03:04:05Z: the day is enough here.
    let day = lock.locked_at.get(..10).unwrap_or(&lock.locked_at);
    let owner_x = x + path_w + 12.0;
    let painter = ui.painter().with_clip_rect(Rect::from_min_size(
        Pos2::new(owner_x, rect.top()),
        vec2(owner_w, rect.height()),
    ));
    paint_text(
        &painter,
        Pos2::new(owner_x, mid),
        &format!("{owner}  {day}"),
        look.ui(size),
        c.text_muted,
    );
    let slot = Rect::from_min_max(
        Pos2::new(rect.right() - buttons - 8.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    region(ui, slot, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if lock.ours {
                if btn(ui, look, "Unlock", true) {
                    cx.send(Message::Run(Command::LfsUnlock {
                        path: lock.path.clone(),
                        id: Some(lock.id.clone()),
                        force: false,
                    }));
                }
            } else if btn(ui, look, "Force unlock…", true) {
                cx.send(Message::Action(Action::LfsForceUnlock {
                    path: lock.path.clone(),
                    id: lock.id.clone(),
                    owner: lock.owner.clone(),
                }));
            }
        });
    });
}

fn discard_row(ui: &mut Ui, cx: &Cx, entry: &DiscardBackup) {
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    if response.hovered() {
        fill(ui, rect, c.hover);
    }
    let mid = rect.center().y;
    let size = look.text_size;
    let buttons = 170.0;
    let mut x = rect.left() + 12.0;
    paint_text(
        ui.painter(),
        Pos2::new(x, mid),
        &short_date(entry.time),
        look.ui(size),
        c.text_muted,
    );
    x += 120.0;
    let files_w = (rect.right() - buttons - 16.0 - x).max(80.0);
    let painter = ui.painter().with_clip_rect(Rect::from_min_size(
        Pos2::new(x, rect.top()),
        vec2(files_w, rect.height()),
    ));
    let files = match &entry.paths[..] {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => trf(
            "{path} and {more}",
            &[("path", first), ("more", &plural(rest.len(), "more file"))],
        ),
    };
    paint_text(&painter, Pos2::new(x, mid), &files, look.mono(size), c.text);
    let slot = Rect::from_min_max(
        Pos2::new(rect.right() - buttons - 8.0, rect.top()),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    region(ui, slot, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if btn(ui, look, "Delete…", true) {
                cx.send(Message::Action(Action::DeleteDiscard(entry.name.clone())));
            }
            if btn(ui, look, "Restore", true) {
                cx.send(Message::Run(Command::RestoreDiscard {
                    name: entry.name.clone(),
                }));
            }
        });
    });
    response.on_hover_text(entry.paths.join("\n"));
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
    (ignore_revs, ignored): (bool, Option<bool>),
) {
    let look = cx.look();
    let subtitle = match rev {
        Some(rev) => format!("{path} @ {}", short_sha(rev)),
        None => trf(
            "{path} · the working folder, uncommitted lines included",
            &[("path", path)],
        ),
    };
    let extra = |ui: &mut Ui| {
        if btn(ui, look, "File history", true) {
            cx.send(Message::OpenFileHistory(path.to_owned()));
        }
        // Offered where the repository lists commits to skip (formatting and the like).
        if ignored.is_some()
            && let Some(on) = check(
                ui,
                ignore_revs,
                "Skip commits in .git-blame-ignore-revs",
                true,
            )
        {
            cx.send(Message::BlameIgnoreRevs(on));
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
        let scrolled = scroll("blame").show_viewport(ui, |ui, viewport| {
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
        super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
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
            let scrolled = scroll("file_history").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                if loading {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        muted(ui, look, tr("Loading…"), look.text_size);
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
            super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
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
            &trf("renamed from {orig}", &[("orig", orig)]),
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
                        summary.push_str(" · ");
                        summary.push_str(&trf(
                            "{count} unresolved",
                            &[("count", &state.unresolved().to_string())],
                        ));
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
                let scrolled = scroll("conflict_chunks").show(ui, |ui| {
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
                super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
            });

            region(ui, result_area, |ui| {
                Frame::new()
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        ui.horizontal(|ui| {
                            text(ui, tr("Result"), size, c.text);
                            if state.edited {
                                muted(ui, look, format!("· {}", tr("edited by hand")), size);
                            }
                            if has_conflict_markers(&state.result) {
                                text(
                                    ui,
                                    format!("· {}", tr("contains conflict markers")),
                                    size,
                                    c.warning,
                                );
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
                        if super::widgets::button(
                            ui,
                            look,
                            "Save & mark resolved",
                            Kind::Accent,
                            data.working.is_some(),
                        ) {
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
            text(
                ui,
                trf("Conflict {number}", &[("number", &(which + 1).to_string())]),
                size,
                c.text,
            );
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
                                    tr("(empty)").to_owned()
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
    let subtitle = if state.replay {
        trf(
            "Apply these commits on top of {branch}, from top to bottom.",
            &[("branch", branch)],
        )
    } else {
        trf(
            "Rewrite {branch} on top of {base}. Commits are applied from top to bottom.",
            &[("branch", branch), ("base", &state.base_label)],
        )
    };
    dialog(
        ctx,
        cx,
        if state.replay {
            "Interactive cherry-pick"
        } else {
            "Interactive rebase"
        },
        &subtitle,
        None,
        |ui, rect| {
            let problem = state.problem().filter(|_| !state.loading);
            let footer = 46.0;
            // The list gets a bit more than half; the highlighted commit's details the rest.
            let lists = rect.height() - footer - 2.0;
            let (list, rest) = split_y(rect, (lists * 0.55).max(120.0).min(lists - 60.0));
            let (line1, rest) = split_y(rest, 1.0);
            let (details, rest) = split_y(rest, (rest.height() - footer - 1.0).max(0.0));
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
                    let scrolled = scroll("rebase_steps").show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        for (index, step) in state.steps.iter().enumerate() {
                            rebase_row(ui, cx, state, index, step);
                        }
                    });
                    super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
                }
            });
            fill(ui, details, c.panel_2);
            region(ui, details, |ui| rebase_details(ui, cx, state));

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
                        let (start, what) = if state.replay {
                            ("Start cherry-pick", "Apply the commits as planned above")
                        } else {
                            ("Start rebase", "Rewrite the history as planned above")
                        };
                        if button_tip(
                            ui,
                            look,
                            start,
                            Kind::Accent,
                            ready,
                            problem.unwrap_or(what),
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
                        let markers = state.steps.iter().any(|step| {
                            ["fixup! ", "squash! ", "amend! "]
                                .iter()
                                .any(|prefix| step.commit.subject.starts_with(prefix))
                        });
                        if button_tip(
                            ui,
                            look,
                            "Autosquash",
                            Kind::Normal,
                            markers,
                            "Move fixup! and squash! commits under the commit they name",
                        ) {
                            cx.send(Message::RebaseAutosquash);
                        }
                        if button_tip(
                            ui,
                            look,
                            "Run after…",
                            Kind::Normal,
                            !state.steps.is_empty(),
                            "Run a command after the highlighted commit, such as the tests",
                        ) {
                            cx.send(Message::Action(Action::RebaseExec(state.focused)));
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
        .on_hover_text(tr("Drag up or down to move this commit (or Alt + ↑ / ↓)"));
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

    // The action, as a dropdown.
    let mut x = rect.left() + 40.0;
    let picker = Rect::from_min_size(Pos2::new(x, mid - 13.0), vec2(ACTION_WIDTH, 26.0));
    rebase_action_picker(ui, cx, index, step.action, picker, response.id);
    x += ACTION_WIDTH;
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
    let date_x = right - DATE_WIDTH;
    let author_x = date_x - AUTHOR_WIDTH;
    // Squash and fixup meld into the commit above: an arrow says so.
    let melds = matches!(step.action, "squash" | "fixup");
    if melds {
        icon::paint(
            ui,
            "arrow-up",
            Rect::from_center_size(Pos2::new(x + 7.0, mid), vec2(13.0, 13.0)),
            c.text_muted,
        );
        ui.interact(
            Rect::from_center_size(Pos2::new(x + 7.0, mid), vec2(16.0, rect.height())),
            response.id.with("melds"),
            Sense::hover(),
        )
        .on_hover_text(tr("Melds into the commit above"));
        x += 20.0;
    }
    let subject_width = (author_x - 8.0 - x).max(40.0);
    let subject_area =
        Rect::from_min_size(Pos2::new(x, rect.top()), vec2(subject_width, rect.height()));
    if step.action != "reword" {
        ui.interact(subject_area, response.id.with("subject"), Sense::hover())
            .on_hover_text(step.commit.subject.as_str());
    }
    ui.interact(
        Rect::from_min_size(
            Pos2::new(author_x, rect.top()),
            vec2(AUTHOR_WIDTH, rect.height()),
        ),
        response.id.with("author"),
        Sense::hover(),
    )
    .on_hover_text(format!("{} <{}>", step.commit.author, step.commit.email));
    ui.interact(
        Rect::from_min_size(
            Pos2::new(date_x, rect.top()),
            vec2(DATE_WIDTH, rect.height()),
        ),
        response.id.with("date"),
        Sense::hover(),
    )
    .on_hover_text(full_date(step.commit.time));
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
        let dropped = step.action == "drop";
        let color = if dropped || melds {
            c.text_muted
        } else {
            c.text
        };
        let width = paint_text(
            &painter.with_clip_rect(subject_area),
            Pos2::new(x, mid),
            &step.commit.subject,
            look.ui(size),
            color,
        );
        // A dropped commit is crossed out.
        if dropped {
            painter.with_clip_rect(subject_area).line_segment(
                [
                    Pos2::new(x, mid + 1.0),
                    Pos2::new(x + width.min(subject_width), mid + 1.0),
                ],
                Stroke::new(1.0, c.text_muted),
            );
        }
        if let Some(command) = &step.exec {
            paint_text(
                &painter.with_clip_rect(subject_area),
                Pos2::new(x + width + 12.0, mid),
                &trf("then runs: {command}", &[("command", command)]),
                look.mono(size - 1.0),
                c.text_muted,
            );
        }
    }
    let faded = if step.action == "drop" {
        c.text_faint
    } else {
        c.text_muted
    };
    paint_text(
        &painter.with_clip_rect(Rect::from_min_size(
            Pos2::new(author_x, rect.top()),
            vec2(AUTHOR_WIDTH - 10.0, rect.height()),
        )),
        Pos2::new(author_x, mid),
        &step.commit.author,
        look.ui(size),
        faded,
    );
    paint_text(
        &painter.with_clip_rect(Rect::from_min_size(
            Pos2::new(date_x, rect.top()),
            vec2(DATE_WIDTH - 8.0, rect.height()),
        )),
        Pos2::new(date_x, mid),
        &short_date(step.commit.time),
        look.ui(size),
        faded,
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
            .on_hover_text(tr(if delta < 0 {
                "Move this commit up (Alt + ↑)"
            } else {
                "Move this commit down (Alt + ↓)"
            }));
        super::widgets::icon_in(ui, look, &response, area, name, 14.0, enabled);
        if enabled && response.clicked() {
            cx.send(Message::RebaseMove(index, delta));
        }
    }
    if response.clicked() {
        cx.send(Message::RebaseFocus(index));
    }
}

const ACTION_WIDTH: f32 = 124.0;
const AUTHOR_WIDTH: f32 = 170.0;
const DATE_WIDTH: f32 = 130.0;

/// An action's name as shown, and the colour of its dot.
fn rebase_action_look(cx: &Cx, action: &str) -> (&'static str, Color32) {
    let c = cx.c();
    match action {
        "pick" => ("Pick", c.success),
        "reword" => ("Reword", c.warning),
        "edit" => ("Edit", c.accent),
        "squash" => ("Squash", c.text_muted),
        "fixup" => ("Fixup", c.text_muted),
        _ => ("Drop", c.danger),
    }
}

/// The dropdown of a row: a dot in the action's colour and its name; the list gives every
/// action with what it does and its key.
fn rebase_action_picker(ui: &mut Ui, cx: &Cx, index: usize, action: &str, rect: Rect, row: Id) {
    let look = cx.look();
    let c = &look.c;
    let response = ui.interact(rect, row.with("action"), Sense::click());
    let fill_color = if response.hovered() {
        c.hover_strong
    } else {
        c.panel
    };
    ui.painter().rect_filled(rect, 5.0, fill_color);
    ui.painter()
        .rect_stroke(rect, 5.0, Stroke::new(1.0, c.border), StrokeKind::Inside);
    let (name, dot) = rebase_action_look(cx, action);
    ui.painter()
        .circle_filled(Pos2::new(rect.left() + 13.0, rect.center().y), 4.5, dot);
    paint_text(
        ui.painter(),
        Pos2::new(rect.left() + 25.0, rect.center().y),
        tr(name),
        look.ui(look.text_size),
        c.text,
    );
    icon::paint(
        ui,
        "chevron-down",
        Rect::from_center_size(
            Pos2::new(rect.right() - 13.0, rect.center().y),
            vec2(12.0, 12.0),
        ),
        c.text_muted,
    );
    if response.clicked() {
        cx.send(Message::RebaseFocus(index));
    }
    egui::Popup::menu(&response)
        .frame(floating(look, 8).inner_margin(Margin::same(4)))
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = crate::theme::DROPDOWN_GAP;
            for (each, key, hint) in REBASE_ACTIONS {
                let (label, color) = rebase_action_look(cx, each);
                let (row, chosen) = ui.allocate_exact_size(vec2(300.0, 30.0), Sense::click());
                if each == action {
                    ui.painter().rect_filled(row, 4.0, c.selection);
                } else if chosen.hovered() {
                    super::widgets::hover_fill(ui, look, row, 4.0);
                }
                let mid = row.center().y;
                ui.painter()
                    .circle_filled(Pos2::new(row.left() + 13.0, mid), 4.5, color);
                let text_color = if each == action {
                    c.selection_text
                } else {
                    c.text
                };
                paint_text(
                    ui.painter(),
                    Pos2::new(row.left() + 26.0, mid - 7.0),
                    tr(label),
                    look.ui(look.text_size),
                    text_color,
                );
                paint_text(
                    ui.painter(),
                    Pos2::new(row.left() + 26.0, mid + 7.0),
                    tr(hint),
                    look.ui(look.text_size - 2.5),
                    c.text_muted,
                );
                ui.painter().text(
                    Pos2::new(row.right() - 10.0, mid),
                    Align2::RIGHT_CENTER,
                    key,
                    look.mono(look.text_size - 1.0),
                    c.text_muted,
                );
                if chosen.clicked() {
                    cx.send(Message::RebaseSetAction(index, each));
                    ui.close();
                }
            }
        });
}

/// The highlighted commit of the rebase editor: who wrote and committed it, its message and
/// the files it changes.
fn rebase_details(ui: &mut Ui, cx: &Cx, state: &RebaseState) {
    use egui::{Label, RichText};
    let look = cx.look();
    let c = &look.c;
    let Some((_, detail)) = &state.detail else {
        placeholder(ui, look, "Select a commit to see its details");
        return;
    };
    let detail = match detail {
        None => {
            placeholder(ui, look, "Loading…");
            return;
        }
        Some(Err(error)) => {
            placeholder(ui, look, error);
            return;
        }
        Some(Ok(detail)) => detail,
    };
    let area = ui.max_rect();
    let (info, files_area) = split_x(area, (area.width() * 0.58).max(280.0));
    let (line, files_area) = split_x(files_area, 1.0);
    fill(ui, line, c.border);
    region(ui, info, |ui| {
        let scrolled = scroll("rebase_detail_info").show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin::symmetric(20, 14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
                    let person = |ui: &mut Ui, title: &str, name: &str, email: &str, time: i64| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            super::details::avatar(ui, cx, name);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                muted(ui, look, tr(title), 11.0);
                                ui.horizontal(|ui| {
                                    text(ui, name, 13.0, c.text);
                                    muted(ui, look, format!("<{email}>"), 12.0);
                                });
                                muted(ui, look, full_date(time), 12.0);
                            });
                        });
                    };
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 32.0;
                        person(ui, "Author", &detail.author, &detail.email, detail.time);
                        if detail.committer != detail.author || detail.commit_time != detail.time {
                            person(
                                ui,
                                "Committer",
                                &detail.committer,
                                &detail.committer_email,
                                detail.commit_time,
                            );
                        }
                    });
                    super::details::fact(ui, cx, "SHA", |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(detail.sha.as_str())
                                    .size(12.0)
                                    .monospace()
                                    .color(c.text),
                            )
                            .selectable(true),
                        );
                    });
                    super::details::fact(
                        ui,
                        cx,
                        if detail.parents.len() > 1 {
                            "Parents"
                        } else {
                            "Parent"
                        },
                        |ui| {
                            let parents: Vec<&str> = detail
                                .parents
                                .iter()
                                .map(|parent| short_sha(parent))
                                .collect();
                            let shown = if parents.is_empty() {
                                tr("(root commit)").to_owned()
                            } else {
                                parents.join("  ")
                            };
                            super::widgets::mono(ui, shown, 12.0, c.text);
                        },
                    );
                    ui.add_space(4.0);
                    ui.add(
                        Label::new(
                            RichText::new(detail.message.trim_end())
                                .size(13.0)
                                .color(c.text),
                        )
                        .selectable(true)
                        .wrap(),
                    );
                });
        });
        super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
    });
    region(ui, files_area, |ui| {
        let (head, list) = split_y(files_area, 34.0);
        paint_text(
            ui.painter(),
            Pos2::new(head.left() + 16.0, head.center().y + 2.0),
            &trf(
                "{files} changed",
                &[("files", &plural(detail.files.len(), "file"))],
            )
            .to_uppercase(),
            look.ui(11.0),
            c.text_muted,
        );
        region(ui, list, |ui| {
            if detail.files.is_empty() {
                placeholder(ui, look, "This commit has no file changes");
                return;
            }
            let scrolled = scroll("rebase_detail_files").show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for file in &detail.files {
                    let (row, hover) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                    let mid = row.center().y;
                    ui.painter().text(
                        Pos2::new(row.left() + 20.0, mid),
                        Align2::CENTER_CENTER,
                        &file.status,
                        look.mono(12.0),
                        super::widgets::status_color(c, &file.status),
                    );
                    let painter = ui.painter().with_clip_rect(row.intersect(ui.clip_rect()));
                    let shown = match &file.orig_path {
                        Some(old) => format!("{old} → {}", file.path),
                        None => file.path.clone(),
                    };
                    paint_text(
                        &painter,
                        Pos2::new(row.left() + 36.0, mid),
                        &shown,
                        look.ui(12.5),
                        c.text,
                    );
                    hover.on_hover_text(shown);
                }
            });
            super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
        });
    });
}

const THEMES: [(&str, &str); 3] = [
    ("system", "Follow system"),
    ("light", "Light"),
    ("dark", "Dark"),
];

const SHORTCUTS: [(&str, &str); 10] = [
    ("Ctrl/⌘ + 1 / 2", "All commits / Local changes"),
    ("Ctrl/⌘ + Z / Shift + Z", "Undo / redo the last operation"),
    ("Ctrl/⌘ + F", "Search commits"),
    ("Ctrl/⌘ + T or O", "Open repository"),
    ("Ctrl/⌘ + W", "Close tab"),
    ("Ctrl/⌘ + Enter", "Commit"),
    ("Ctrl/⌘ + Shift + Enter", "Stage everything and commit"),
    ("Ctrl/⌘ + + / − / 0", "Zoom"),
    ("F5 / Ctrl/⌘ + R", "Refresh repository"),
    ("↑ ↓ PgUp PgDn", "Move through commits or files"),
];

/// A titled group of settings.
/// About GitCraft: its name and version, what it is and who made it, and where it keeps its
/// settings and which git it runs.
fn about(ctx: &Context, cx: &Cx, git: Option<&Result<String, String>>) {
    use egui::RichText;
    let look = cx.look();
    let c = &look.c;
    let response = egui::Modal::new(Id::unique("about"))
        .backdrop_color(c.backdrop)
        .frame(floating(look, 12).inner_margin(Margin::symmetric(24, 20)))
        .show(ctx, |ui| {
            ui.set_width(400.0_f32.min(cx.app.window.x - 80.0));
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            ui.horizontal(|ui| {
                icon::icon(ui, "merge", 22.0, c.accent);
                text(ui, tr("About GitCraft"), 18.0, c.text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if accent(ui, look, "Close", true) {
                        cx.send(Message::OverlayClose);
                    }
                });
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("GitCraft {}", env!("CARGO_PKG_VERSION")))
                    .size(15.0)
                    .strong()
                    .color(c.text),
            );
            ui.label(tr(
                "A graphical Git client for Windows, macOS and Linux, built on the git you have installed.",
            ));
            ui.label(
                RichText::new(tr("Author: Jürgen Könnemann"))
                    .strong()
                    .color(c.text),
            );
            muted(ui, look, tr("Open source under the MIT License."), look.text_size);
            ui.add_space(8.0);
            let detail = |ui: &mut Ui, line: String| {
                ui.add(
                    egui::Label::new(RichText::new(line).size(12.0).color(c.text_muted))
                        .selectable(true)
                        .wrap(),
                );
            };
            let program = match cx.app.settings.general.git_program.trim() {
                "" => tr("git on PATH").to_owned(),
                chosen => chosen.to_owned(),
            };
            let version = match git {
                None => tr("Loading…").to_owned(),
                Some(Ok(text)) => text.clone(),
                Some(Err(error)) => error.clone(),
            };
            detail(
                ui,
                trf(
                    "Git: {version} ({program})",
                    &[("version", &version), ("program", &program)],
                ),
            );
            detail(
                ui,
                trf(
                    "Settings folder: {path}",
                    &[(
                        "path",
                        &gitcraft_core::store::config_dir().display().to_string(),
                    )],
                ),
            );
        });
    if response.backdrop_response.clicked() {
        cx.send(Message::OverlayClose);
    }
}

/// Settings: its pages listed on the left, the chosen one on the right.
fn settings(ctx: &Context, cx: &Cx) {
    let look = cx.look();
    let c = &look.c;
    let window = cx.app.window;
    let page = cx.app.settings_page;
    super::settle(ctx, "settings_frames");
    let (width, height) = (
        (window.x - 60.0).clamp(300.0, 920.0),
        (window.y - 60.0).clamp(240.0, 680.0),
    );
    let response = egui::Modal::new(Id::unique("settings"))
        .backdrop_color(c.backdrop)
        .frame(floating(look, 12).inner_margin(Margin::ZERO))
        .show(ctx, |ui| {
            let (_, rect) = ui.allocate_space(vec2(width, height));
            let (head, rest) = split_y(rect, 64.0);
            region(ui, head, |ui| {
                bar(ui, 64.0, Color32::TRANSPARENT, 24.0, |ui| {
                    text(ui, tr("Settings"), 22.0, c.text);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if accent(ui, look, "Done", true) {
                            cx.send(Message::OverlayClose);
                        }
                    });
                });
            });
            let (line, rest) = split_y(rest, 1.0);
            fill(ui, line, c.border);
            let (nav, rest) = split_x(rest, 210.0_f32.min(rest.width() * 0.35));
            ui.painter().rect_filled(
                nav,
                egui::CornerRadius {
                    sw: 12,
                    ..Default::default()
                },
                c.panel_2,
            );
            let (line, content) = split_x(rest, 1.0);
            fill(ui, line, c.border);
            region(ui, nav.shrink2(vec2(10.0, 12.0)), |ui| {
                // In a short window the list scrolls, so every page stays reachable.
                scroll("settings_pages").show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
                    for each in SettingsPage::ALL {
                        if settings_tab(ui, cx, each.title(), each == page) {
                            cx.send(Message::SettingsPage(each));
                        }
                    }
                });
            });
            region(ui, content, |ui| {
                // Always show the bar, so that it is clear when there is more below.
                let bars = &mut ui.spacing_mut().scroll;
                bars.dormant_handle_opacity = bars.active_handle_opacity;
                bars.dormant_background_opacity = bars.active_background_opacity;
                // Both ways: a page wider than the window (long translated labels, a narrow
                // window) scrolls sideways rather than being cut off.
                let shown = scroll(("settings", page))
                    .hscroll(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        Frame::new()
                            .inner_margin(Margin {
                                left: 28,
                                right: 28,
                                top: 18,
                                bottom: 24,
                            })
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                                ui.label(
                                    egui::RichText::new(tr(page.title()))
                                        .size(18.0)
                                        .strong()
                                        .color(c.text),
                                );
                                ui.add_space(6.0);
                                match page {
                                    SettingsPage::Appearance => settings_appearance(ui, cx),
                                    SettingsPage::Diffs => settings_diffs(ui, cx),
                                    SettingsPage::Tools => settings_tools(ui, cx),
                                    SettingsPage::Repository => settings_repository(ui, cx),
                                    SettingsPage::Git => settings_git(ui, cx),
                                    SettingsPage::Shortcuts => settings_shortcuts(ui, cx),
                                }
                            });
                    });
                super::widgets::scroll_shadows(ui, cx.look(), &shown);
            });
        });
    if response.backdrop_response.clicked() {
        cx.send(Message::OverlayClose);
    }
}

/// One entry of the settings' page list.
fn settings_tab(ui: &mut Ui, cx: &Cx, title: &str, selected: bool) -> bool {
    let look = cx.look();
    let c = &look.c;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 6.0, c.selection);
    } else if response.hovered() {
        super::widgets::hover_fill(ui, look, rect, 6.0);
    }
    paint_text(
        ui.painter(),
        Pos2::new(rect.left() + 12.0, rect.center().y),
        tr(title),
        look.ui(look.text_size),
        if selected { c.selection_text } else { c.text },
    );
    response.clicked()
}

fn settings_appearance(ui: &mut Ui, cx: &Cx) {
    use egui::Grid;
    let values = &cx.app.settings;
    Grid::new("appearance")
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("Theme"));
            let options: Vec<(String, String)> = THEMES
                .iter()
                .map(|(value, label)| ((*value).to_owned(), tr(label).to_owned()))
                .collect();
            if let Some(chosen) = choice(ui, "theme", &options, &values.appearance.theme, 170.0) {
                cx.send(Message::SetTheme(chosen));
            }
            ui.end_row();
            ui.label(tr("Language"));
            let mut languages = vec![(
                crate::i18n::AUTO.to_owned(),
                tr("System language").to_owned(),
            )];
            languages.extend(
                crate::i18n::languages()
                    .into_iter()
                    .map(|(code, name)| (code.to_owned(), name.to_owned())),
            );
            if let Some(chosen) =
                choice(ui, "language", &languages, &values.general.language, 170.0)
            {
                cx.send(Message::SetLanguage(chosen));
            }
            ui.end_row();
        });
}

fn settings_diffs(ui: &mut Ui, cx: &Cx) {
    use egui::{DragValue, Grid};
    let look = cx.look();
    let values = &cx.app.settings;
    Grid::new("diffs")
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("Layout"));
            if let Some(chosen) = segmented(
                ui,
                look,
                ["Unified", "Side by side"],
                usize::from(values.diff.split),
            ) {
                cx.send(Message::SetSplit(chosen == 1));
            }
            ui.end_row();
            ui.label(tr("Context lines"));
            let mut lines = values.diff.context;
            ui.add(DragValue::new(&mut lines).range(0..=100));
            if lines != values.diff.context {
                cx.send(Message::SetContext(lines.to_string()));
            }
            ui.end_row();
        });
    if let Some(on) = check(
        ui,
        values.diff.ignore_whitespace,
        "Ignore whitespace changes",
        true,
    ) {
        cx.send(Message::SetIgnoreWhitespace(on));
    }
    if let Some(on) = check(
        ui,
        values.diff.syntax_highlight,
        "Syntax highlighting",
        true,
    ) {
        cx.send(Message::SetSyntax(on));
    }
}

fn settings_tools(ui: &mut Ui, cx: &Cx) {
    use egui::{Grid, RichText};
    let look = cx.look();
    let c = &look.c;
    let values = &cx.app.settings;
    let found = &cx.app.found_tools;
    // "None", what git found, and a saved choice git no longer finds.
    let options = |list: &[gitcraft_core::model::ExternalTool], chosen: &str| {
        let mut options = vec![(String::new(), tr("None").to_owned())];
        options.extend(
            list.iter()
                .map(|tool| (tool.name.clone(), tool.label.clone())),
        );
        if !chosen.is_empty() && !list.iter().any(|tool| tool.name == chosen) {
            options.push((
                chosen.to_owned(),
                trf("{tool} (not found)", &[("tool", chosen)]),
            ));
        }
        options
    };
    Grid::new("tools")
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("Diff tool"));
            let diff = options(&found.diff, &values.tools.diff);
            if let Some(chosen) = choice(ui, "diff_tool", &diff, &values.tools.diff, 170.0) {
                cx.send(Message::SetDiffTool(chosen));
            }
            ui.end_row();
            ui.label(tr("Merge tool"));
            let merge = options(&found.merge, &values.tools.merge);
            if let Some(chosen) = choice(ui, "merge_tool", &merge, &values.tools.merge, 170.0) {
                cx.send(Message::SetMergeTool(chosen));
            }
            ui.end_row();
        });
    let hint = if found.diff.is_empty() && found.merge.is_empty() {
        "No diff or merge tools were found. Install one, such as KDiff3 or Meld, or set one up in git's config (difftool.<name>.cmd)."
    } else {
        "Files open in the chosen tools from their right-click menus."
    };
    ui.add(egui::Label::new(RichText::new(tr(hint)).size(12.0).color(c.text_muted)).wrap());
}

fn settings_repository(ui: &mut Ui, cx: &Cx) {
    use egui::{DragValue, Grid};
    let values = &cx.app.settings;
    Grid::new("repository")
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("Commits loaded at a time"));
            let mut limit = values.history.log_limit;
            ui.add(DragValue::new(&mut limit).range(100..=100_000).speed(50));
            if limit != values.history.log_limit {
                cx.send(Message::SetLogLimit(limit.to_string()));
            }
            ui.end_row();
            ui.label(tr("Fetch in the background every (minutes, 0 = never)"));
            let mut minutes = values.history.auto_fetch_minutes;
            ui.add(DragValue::new(&mut minutes).range(0..=1440).speed(1));
            if minutes != values.history.auto_fetch_minutes {
                cx.send(Message::SetAutoFetch(minutes));
            }
            ui.end_row();
        });
    if let Some(on) = check(
        ui,
        values.history.fetch_on_open,
        "Fetch when opening a repository",
        true,
    ) {
        cx.send(Message::SetFetchOnOpen(on));
    }
    if let Some(on) = check(
        ui,
        values.history.show_stashes,
        "Show stashes in the history",
        true,
    ) {
        cx.send(Message::SetShowStashes(on));
    }
    if let Some(on) = check(
        ui,
        values.history.date_order,
        "Order the history by date instead of keeping branches together",
        true,
    ) {
        cx.send(Message::SetDateOrder(on));
    }
    if let Some(on) = check(
        ui,
        values.history.conflict_check,
        "Say beforehand which files a merge or rebase would conflict in",
        true,
    ) {
        cx.send(Message::SetConflictCheck(on));
    }
    if let Some(on) = check(
        ui,
        values.history.commit_graph,
        "Keep a commit-graph file for faster history",
        true,
    ) {
        cx.send(Message::SetCommitGraph(on));
    }
}

fn settings_git(ui: &mut Ui, cx: &Cx) {
    use egui::{Grid, RichText};
    let look = cx.look();
    let c = &look.c;
    // Wide enough for the longest hint, which would otherwise stretch its field past the
    // others: every field of the page ends at the same place.
    const FIELD: f32 = 260.0;
    // Narrower than this, the page scrolls sideways rather than squeezing its text fields
    // (which would give way while the dropdowns don't). Other pages have no such fields and
    // keep their own width.
    ui.set_min_width(600.0);
    let values = &cx.app.settings;
    let state = &cx.app.git_config;
    let has_repo = cx.app.current().is_some();
    ui.horizontal(|ui| {
        if let Some(chosen) = segmented(
            ui,
            look,
            ["Everywhere", "This repository"],
            usize::from(state.here),
        ) && (chosen == 0 || has_repo)
        {
            cx.send(Message::GitConfigScope(chosen == 1));
        }
    });
    let hint = if state.here {
        "Only for this repository; empty fields use the settings for everywhere."
    } else {
        "Your settings for every repository (git config --global)."
    };
    ui.add(egui::Label::new(RichText::new(tr(hint)).size(12.0).color(c.text_muted)).wrap());
    if state.loading {
        placeholder(ui, look, "Loading…");
        return;
    }
    Grid::new("git-config")
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            ui.label(tr("Git program"));
            let (response, changed) = line_edit(
                ui,
                "git-program",
                &state.program,
                "git on PATH",
                FIELD,
                look.ui(look.text_size),
            );
            // Checked and taken once typing is done, with what was typed in this very frame.
            let typed = changed.clone().unwrap_or_else(|| state.program.clone());
            if let Some(text) = changed {
                cx.send(Message::GitProgramInput(text));
            }
            if response.lost_focus() && typed != values.general.git_program {
                cx.send(Message::SetGitProgram(typed));
            }
            ui.end_row();
            for (key, label, kind) in crate::logic::GIT_SETTINGS {
                ui.label(tr(label));
                let value = state.shown(key);
                match kind {
                    crate::logic::SettingKind::Text(hint) => {
                        let (_, changed) = line_edit(
                            ui,
                            ("git-config", key),
                            value,
                            hint,
                            FIELD,
                            look.ui(look.text_size),
                        );
                        if let Some(text) = changed {
                            cx.send(Message::GitConfigEdit(key.to_owned(), text));
                        }
                    }
                    crate::logic::SettingKind::Choice(options) => {
                        let options: Vec<(String, String)> = options
                            .iter()
                            .map(|(value, label)| ((*value).to_owned(), tr(label).to_owned()))
                            .collect();
                        if let Some(chosen) =
                            choice(ui, ("git-config", key), &options, value, FIELD)
                        {
                            cx.send(Message::GitConfigEdit(key.to_owned(), chosen));
                        }
                    }
                }
                ui.end_row();
            }
        });
    if accent(ui, look, "Save git settings", !state.changed().is_empty()) {
        cx.send(Message::GitConfigSave);
    }
}

fn settings_shortcuts(ui: &mut Ui, cx: &Cx) {
    use egui::{Grid, RichText};
    let look = cx.look();
    Grid::new("shortcuts")
        .num_columns(2)
        .spacing([24.0, 6.0])
        .show(ui, |ui| {
            for (keys, what) in SHORTCUTS {
                ui.label(RichText::new(keys).monospace().size(12.0));
                muted(ui, look, tr(what), look.text_size);
                ui.end_row();
            }
        });
}
