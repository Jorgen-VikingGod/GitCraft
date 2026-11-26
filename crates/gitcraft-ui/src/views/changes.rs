//! Local changes: file lists, commit box and the diff with line staging.

use egui::{Align, Layout, Margin, Ui, vec2};
use gitcraft_core::model::{FileChange, Operation};

use super::widgets::{
    FILE_ROW, GRIP, accent, area_edit, bar, btn, check, danger, file_row, fill, heading, line_edit,
    muted, placeholder, region, rows, scroll, split_x, split_y, splitter, text, warning,
};
use super::{Cx, diff, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{Area, Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
use crate::i18n::{tr, trf};
use crate::logic::plural;

const SUBJECT_LIMIT: usize = 72;
const HEADER: f32 = 32.0;
const COMMIT_BOX: f32 = 196.0;

fn list(ui: &mut Ui, cx: &Cx, tab: &RepoTab, area: Area, files: &[FileChange], empty: &str) {
    let look = cx.look();
    if files.is_empty() {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(
                egui::RichText::new(tr(empty))
                    .size(13.0)
                    .italics()
                    .color(look.c.text_muted),
            );
        });
        return;
    }
    let picked = if tab.picked.0 == area {
        tab.picked.1.as_slice()
    } else {
        &[]
    };
    let (symbol, quick): (&str, fn(String) -> Message) = match area {
        Area::Unstaged => ("plus", |path| {
            Message::Run(Command::Stage { paths: vec![path] })
        }),
        Area::Staged => ("minus", |path| {
            Message::Run(Command::Unstage { paths: vec![path] })
        }),
        Area::Conflicts => ("edit", Message::OpenConflict),
    };
    // A row picked with the keyboard may be out of view, and then it isn't drawn to scroll itself.
    let last_picked = egui::Id::unique(("changes_picked", &tab.path, area as u8));
    let newest = picked.last();
    let before = ui.data(|data| data.get_temp::<Option<String>>(last_picked));
    let reveal = if before.as_ref().map(Option::as_ref) == Some(newest) {
        None
    } else {
        ui.data_mut(|data| data.insert_temp(last_picked, newest.cloned()));
        newest.and_then(|path| files.iter().position(|file| &file.path == path))
    };
    let scrolled = rows(
        ui,
        scroll(("changes", &tab.path, area as u8)),
        FILE_ROW,
        files.len(),
        reveal,
        |ui, index| {
            let file = &files[index];
            let path = file.path.clone();
            let row = file_row(ui, cx, file, picked.contains(&path), Some(symbol));
            if row.action {
                cx.send(quick(path));
            } else if row.row.double_clicked() {
                cx.send(Message::FileActivated(area, path));
            } else if row.row.clicked() {
                cx.send(Message::FilePicked(area, path));
            } else if row.row.secondary_clicked() {
                cx.send(Message::OpenMenu(MenuRequest::File {
                    area: Some(area),
                    path,
                    rev: None,
                }));
            }
        },
    );
    super::widgets::scroll_shadows(ui, cx.look(), &scrolled);
}

fn commit_box(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let staged = tab.status.as_ref().map_or(0, |status| status.staged.len());
    let has_head = tab
        .status
        .as_ref()
        .is_some_and(|status| status.branch.oid.is_some());
    let merging = tab
        .status
        .as_ref()
        .is_some_and(|status| matches!(status.operation, Some(Operation::Merge { .. })));
    let label = if tab.committing {
        tr("Committing…").to_owned()
    } else if merging {
        tr("Commit merge").to_owned()
    } else if tab.amend {
        tr("Amend commit").to_owned()
    } else if staged > 0 {
        trf("Commit {files}", &[("files", &plural(staged, "file"))])
    } else {
        tr("Commit").to_owned()
    };
    let length = tab.summary.chars().count();
    let count_color = if length > SUBJECT_LIMIT {
        c.warning
    } else {
        c.text_faint
    };
    egui::Frame::new()
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.horizontal(|ui| {
                let count_width = 30.0;
                let (response, changed) = line_edit(
                    ui,
                    ("summary", &tab.path),
                    &tab.summary,
                    "Commit summary",
                    ui.available_width() - count_width - 6.0,
                    look.ui(13.0),
                );
                if let Some(summary) = changed {
                    cx.send(Message::SummaryInput(summary));
                }
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    cx.send(Message::CommitNow);
                }
                text(ui, length.to_string(), 11.0, count_color);
            });
            if let Some(body) = area_edit(
                ui,
                ("body", &tab.path),
                &tab.body,
                "Description",
                80.0,
                look.ui(13.0),
            ) {
                cx.send(Message::BodyInput(body));
            }
            ui.horizontal(|ui| {
                // "Commit first" from the checkout dialog: say what follows, and let it be called off.
                if let Some(Command::Checkout { reference, kind }) = &tab.after_commit {
                    let name = if *kind == "commit" {
                        crate::logic::short_sha(reference)
                    } else {
                        reference
                    };
                    muted(
                        ui,
                        look,
                        trf("Then {name} is checked out", &[("name", name)]),
                        12.0,
                    );
                    if super::widgets::icon_button(ui, look, "x", 12.0, true) {
                        cx.send(Message::CommitFirstCancel);
                    }
                } else {
                    if let Some(amend) = check(ui, tab.amend, "Amend last commit", has_head) {
                        cx.send(Message::AmendToggled(amend));
                    }
                    let prefs = &cx.app.settings.commit;
                    let active = [prefs.sign_off, prefs.skip_hooks, prefs.push_after]
                        .into_iter()
                        .filter(|on| *on)
                        .count()
                        + crate::logic::co_author_trailers(&tab.co_authors).len();
                    let label = if active == 0 {
                        tr("Options").to_owned()
                    } else {
                        trf("Options ({count})", &[("count", &active.to_string())])
                    };
                    if btn(ui, look, &label, true) {
                        cx.send(Message::OpenMenu(MenuRequest::CommitOptions));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if accent(ui, look, &label, tab.can_commit()) {
                        cx.send(Message::CommitNow);
                    }
                });
            });
        });
}

fn header(ui: &mut Ui, cx: &Cx, title: &str, buttons: impl FnOnce(&mut Ui)) {
    let look = cx.look();
    bar(ui, HEADER, egui::Color32::TRANSPARENT, 8.0, |ui| {
        ui.add_space(6.0);
        heading(ui, look, title);
        ui.with_layout(Layout::right_to_left(Align::Center), buttons);
    });
}

pub fn view(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let area = ui.max_rect();
    let width = cx
        .app
        .settings
        .layout
        .changes_width
        .min(area.width() - 200.0)
        .max(200.0);
    let (left, rest) = split_x(area, width);
    let (grip, right) = split_x(rest, GRIP);
    region(ui, left, |ui| {
        fill(ui, left, c.panel);
        lists(ui, cx, tab);
    });
    splitter(ui, cx, grip, Splitter::Changes);
    region(ui, right, |ui| detail(ui, cx, tab, right.width()));
}

fn lists(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let conflicts = tab.files_in(Area::Conflicts);
    let unstaged = tab.files_in(Area::Unstaged);
    let staged = tab.files_in(Area::Staged);
    let area = ui.max_rect();
    let (top, bottom) = split_y(area, (area.height() - COMMIT_BOX).max(0.0));

    // Header, a divider and a share of the room for each list.
    let sections = 2 + usize::from(!conflicts.is_empty());
    let fixed = sections as f32 * (HEADER + 1.0);
    let room = (top.height() - fixed).max(0.0);
    let weight = |n: f32| room * n / (if conflicts.is_empty() { 4.0 } else { 5.0 });
    let mut rest = top;
    let mut cut = |ui: &mut Ui, share: f32, content: &dyn Fn(&mut Ui)| {
        let (piece, below) = split_y(rest, share);
        rest = below;
        region(ui, piece, |ui| content(ui));
    };

    if !conflicts.is_empty() {
        cut(ui, HEADER, &|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
            bar(ui, HEADER, egui::Color32::TRANSPARENT, 14.0, |ui| {
                icon::icon(ui, "alert", 13.0, c.danger);
                text(
                    ui,
                    trf(
                        "Conflicts ({count})",
                        &[("count", &conflicts.len().to_string())],
                    )
                    .to_uppercase(),
                    11.0,
                    c.danger,
                );
            });
        });
        cut(ui, weight(1.0), &|ui| {
            list(ui, cx, tab, Area::Conflicts, &conflicts, "")
        });
        cut(ui, 1.0, &|ui| fill(ui, ui.max_rect(), c.border));
    }

    cut(ui, HEADER, &|ui| {
        header(
            ui,
            cx,
            &trf(
                "Unstaged ({count})",
                &[("count", &unstaged.len().to_string())],
            ),
            |ui| {
                if unstaged.is_empty() {
                    return;
                }
                if accent(ui, look, "Stage all", true) {
                    cx.send(Message::Run(Command::StageAll));
                }
                if danger(ui, look, "Discard all", true) {
                    let every = unstaged.iter().map(|file| file.path.clone()).collect();
                    cx.send(Message::Action(Action::Discard(every)));
                }
            },
        );
    });
    cut(ui, weight(2.0), &|ui| {
        list(
            ui,
            cx,
            tab,
            Area::Unstaged,
            &unstaged,
            "No unstaged changes",
        )
    });
    cut(ui, 1.0, &|ui| fill(ui, ui.max_rect(), c.border));
    cut(ui, HEADER, &|ui| {
        header(
            ui,
            cx,
            &trf("Staged ({count})", &[("count", &staged.len().to_string())]),
            |ui| {
                if !staged.is_empty() && warning(ui, look, "Unstage all", true) {
                    cx.send(Message::Run(Command::UnstageAll));
                }
            },
        );
    });
    cut(ui, weight(2.0), &|ui| {
        list(ui, cx, tab, Area::Staged, &staged, "Nothing staged yet")
    });
    cut(ui, 1.0, &|ui| fill(ui, ui.max_rect(), c.border));

    let operation = tab
        .status
        .as_ref()
        .and_then(|status| status.operation.as_ref());
    if crate::logic::commit_box_gives_way(operation) {
        region(ui, bottom, |ui| operation_note(ui, cx, tab));
    } else {
        region(ui, bottom, |ui| commit_box(ui, cx, tab));
    }
}

/// In place of the commit box while a rebase, cherry-pick, revert or patch is in progress:
/// what continuing will commit. Those commit what they replay, with its own message.
fn operation_note(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let Some(status) = tab.status.as_ref() else {
        return;
    };
    let Some(operation) = status.operation.as_ref() else {
        return;
    };
    let theirs = status
        .sides
        .as_ref()
        .map(|sides| sides.theirs.as_str())
        .filter(|theirs| !theirs.is_empty());
    let note = match (operation, theirs) {
        (Operation::Rebase { .. }, Some(commit)) => trf(
            "When you continue, {commit} is committed again with its own message, and the rebase goes on with the next commit.",
            &[("commit", commit)],
        ),
        (Operation::Rebase { .. }, None) => {
            tr("When you continue, the rebase goes on with the next commit.").to_owned()
        }
        (Operation::CherryPick { .. }, Some(commit)) => trf(
            "When you continue, {commit} is committed with its own message.",
            &[("commit", commit)],
        ),
        (Operation::Revert { .. }, Some(commit)) => trf(
            "When you continue, the revert of {commit} is committed.",
            &[("commit", commit)],
        ),
        _ => tr("When you continue, the change is committed with its own message.").to_owned(),
    };
    egui::Frame::new()
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.horizontal(|ui| {
                icon::icon(ui, "merge", 15.0, c.warning);
                text(
                    ui,
                    crate::logic::operation_title(operation, status.sides.as_ref()),
                    13.0,
                    c.text,
                );
            });
            ui.add(
                egui::Label::new(egui::RichText::new(note).size(12.5).color(c.text_muted)).wrap(),
            );
            if !status.conflicts.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(tr(
                            "Resolve the conflicted files above first; Continue is in the bar at the top.",
                        ))
                        .size(12.5)
                        .color(c.text_muted),
                    )
                    .wrap(),
                );
            }
        });
}

/// A conflicted file: what each side did, how to resolve it, and the conflicting parts.
fn conflict_page(ui: &mut Ui, cx: &Cx, tab: &RepoTab, path: &str) {
    use gitcraft_core::model::ConflictChunk;
    let look = cx.look();
    let c = &look.c;
    let status = tab.status.as_ref();
    let code = status
        .and_then(|status| status.conflicts.iter().find(|file| file.path == path))
        .map_or("UU", |file| file.status.as_str());
    let (ours_did, theirs_did, summary) = crate::logic::conflict_kind(code);
    let (ours_label, theirs_label) = crate::logic::side_labels(
        status.and_then(|status| status.operation.as_ref()),
        status.and_then(|status| status.sides.as_ref()),
    );
    let resolve = |side: Option<&str>| {
        Message::Action(Action::ConflictResolve {
            path: path.to_owned(),
            side: side.map(str::to_owned),
        })
    };
    let scrolled = scroll(("conflict_page", path)).show(ui, |ui| {
        egui::Frame::new()
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
                ui.horizontal(|ui| {
                    icon::icon(ui, "alert", 22.0, c.danger);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        text(ui, trf("{path} has conflicts", &[("path", path)]), 15.0, c.text);
                        muted(ui, look, tr(summary), 13.0);
                    });
                });
                // The two sides, each with what it did and a way to take it as it is.
                ui.columns(2, |columns| {
                    for (column, label, did, tint, side) in [
                        (0, &ours_label, ours_did, c.conflict_ours, "ours"),
                        (1, &theirs_label, theirs_did, c.conflict_theirs, "theirs"),
                    ] {
                        let ui = &mut columns[column];
                        egui::Frame::new()
                            .fill(tint)
                            .stroke(egui::Stroke::new(1.0, c.border))
                            .corner_radius(6)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 6.0;
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(label.as_str()).size(13.0).color(c.text),
                                    )
                                    .wrap(),
                                );
                                muted(ui, look, tr(did), 12.5);
                                if btn(ui, look, "Use this side", true) {
                                    cx.send(resolve(Some(side)));
                                }
                            });
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if accent(ui, look, "Resolve in GitCraft", true) {
                        cx.send(Message::OpenConflict(path.to_owned()));
                    }
                    let tool = &cx.app.settings.tools.merge;
                    if !tool.is_empty() {
                        let label = cx.app.found_tools.label(true, tool);
                        if btn(ui, look, &trf("Open in {tool}", &[("tool", &label)]), true) {
                            cx.send(Message::OpenMergeTool(path.to_owned()));
                        }
                    }
                    if btn(ui, look, "Mark as resolved", true) {
                        cx.send(resolve(None));
                    }
                });
                // The conflicting parts, as the file has them now.
                let data = tab
                    .conflict_preview
                    .as_ref()
                    .filter(|(shown, _)| shown == path)
                    .and_then(|(_, data)| data.as_ref());
                let versions = match data {
                    None => {
                        muted(ui, look, tr("Loading…"), 12.5);
                        return;
                    }
                    Some(Err(error)) => {
                        muted(ui, look, error.as_str(), 12.5);
                        return;
                    }
                    Some(Ok(versions)) => versions,
                };
                let parts: Vec<(usize, &ConflictChunk)> = versions
                    .chunks
                    .iter()
                    .enumerate()
                    .filter(|(_, chunk)| matches!(chunk, ConflictChunk::Conflict { .. }))
                    .collect();
                if parts.is_empty() {
                    let note = if versions.working.is_none() {
                        "The file was deleted on one side. Keep one side, or mark it as resolved to keep the current state."
                    } else {
                        "No conflict markers found (binary file, or the markers were already removed). Edit the result below if needed, then mark the file as resolved."
                    };
                    muted(ui, look, tr(note), 12.5);
                    return;
                }
                heading(ui, look, &plural(parts.len(), "conflict"));
                for (number, (at, chunk)) in parts.iter().enumerate() {
                    let ConflictChunk::Conflict { ours, theirs, .. } = chunk else {
                        continue;
                    };
                    conflict_part(ui, cx, number, versions, *at, ours, theirs, (&ours_label, &theirs_label));
                }
            });
    });
    super::widgets::scroll_shadows(ui, look, &scrolled);
}

/// One conflicting part, read-only: the lines before it, then both sides next to each other.
#[allow(clippy::too_many_arguments)]
fn conflict_part(
    ui: &mut Ui,
    cx: &Cx,
    number: usize,
    versions: &gitcraft_core::model::ConflictVersions,
    at: usize,
    ours: &[String],
    theirs: &[String],
    (ours_label, theirs_label): (&str, &str),
) {
    use gitcraft_core::model::ConflictChunk;
    let look = cx.look();
    let c = &look.c;
    let empty = [tr("(empty)").to_owned()];
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, c.card_line))
        .corner_radius(6)
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            text(
                ui,
                trf(
                    "Conflict {number}",
                    &[("number", &(number + 1).to_string())],
                ),
                13.0,
                c.text,
            );
            let before = at
                .checked_sub(1)
                .and_then(|before| versions.chunks.get(before));
            if let Some(ConflictChunk::Common { lines }) = before {
                let tail: Vec<String> = lines.iter().rev().take(2).rev().cloned().collect();
                if !tail.is_empty() {
                    super::widgets::code_block(
                        ui,
                        look,
                        ("conflict-context", number),
                        &tail,
                        c.text_muted,
                    );
                }
            }
            ui.columns(2, |columns| {
                for (column, label, lines, tint) in [
                    (0, ours_label, ours, c.conflict_ours),
                    (1, theirs_label, theirs, c.conflict_theirs),
                ] {
                    let ui = &mut columns[column];
                    egui::Frame::new()
                        .fill(tint)
                        .corner_radius(4)
                        .inner_margin(Margin::same(6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 3.0;
                            muted(ui, look, label, 11.5);
                            super::widgets::code_block(
                                ui,
                                look,
                                ("conflict-side", number, column),
                                if lines.is_empty() { &empty } else { lines },
                                c.text,
                            );
                        });
                }
            });
        });
}

fn detail(ui: &mut Ui, cx: &Cx, tab: &RepoTab, width: f32) {
    let look = cx.look();
    let c = &look.c;
    match &tab.focus {
        Some((Area::Conflicts, path)) => {
            fill(ui, ui.max_rect(), c.bg);
            conflict_page(ui, cx, tab, path);
        }
        Some((area, path)) => {
            let unstaged = *area == Area::Unstaged;
            let actions = |ui: &mut Ui| {
                if unstaged {
                    if accent(ui, look, "Stage file", true) {
                        cx.send(Message::Run(Command::Stage {
                            paths: vec![path.clone()],
                        }));
                    }
                    if danger(ui, look, "Discard file", true) {
                        cx.send(Message::Action(Action::Discard(vec![path.clone()])));
                    }
                } else if warning(ui, look, "Unstage file", true) {
                    cx.send(Message::Run(Command::Unstage {
                        paths: vec![path.clone()],
                    }));
                }
            };
            diff::view(
                ui,
                cx,
                &tab.changes_pane,
                PaneKey::Changes,
                Some(if unstaged { "stage" } else { "unstage" }),
                "Select a file to see its changes",
                Some(&actions),
                width,
            );
        }
        None => {
            fill(ui, ui.max_rect(), c.bg);
            let note = if tab.change_count() > 0 {
                tr("Select a file to see its changes")
            } else {
                tr("Working tree is clean")
            };
            placeholder(ui, look, note);
        }
    }
}
