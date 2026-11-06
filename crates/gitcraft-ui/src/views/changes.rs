//! Local changes: file lists, commit box and the diff with line staging.

use egui::{Align, Layout, Margin, Ui, vec2};
use gitcraft_core::model::FileChange;

use super::widgets::{
    GRIP, area_edit, bar, btn, centered, check, danger_text, file_row, fill, ghost, heading,
    line_edit, muted, placeholder, primary, region, scroll, split_x, split_y, splitter, text,
};
use super::{Cx, diff, icon};
use crate::actions::{Action, MenuRequest};
use crate::app::{Area, Message, PaneKey, RepoTab, Splitter};
use crate::command::Command;
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
                egui::RichText::new(empty)
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
    scroll(("changes", &tab.path, area as u8)).show(ui, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        for file in files {
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
        }
    });
}

fn commit_box(ui: &mut Ui, cx: &Cx, tab: &RepoTab) {
    let look = cx.look();
    let c = &look.c;
    let staged = tab.status.as_ref().map_or(0, |status| status.staged.len());
    let has_head = tab
        .status
        .as_ref()
        .is_some_and(|status| status.branch.oid.is_some());
    let label = if tab.committing {
        "Committing…".to_owned()
    } else if tab.amend {
        "Amend commit".to_owned()
    } else if staged > 0 {
        format!("Commit {}", plural(staged, "file"))
    } else {
        "Commit".to_owned()
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
                if let Some(amend) = check(ui, tab.amend, "Amend last commit", has_head) {
                    cx.send(Message::AmendToggled(amend));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if primary(ui, look, &label, tab.can_commit()) {
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
                    format!("CONFLICTS ({})", conflicts.len()),
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
        header(ui, cx, &format!("Unstaged ({})", unstaged.len()), |ui| {
            if unstaged.is_empty() {
                return;
            }
            if btn(ui, look, "Stage all", true) {
                cx.send(Message::Run(Command::StageAll));
            }
            if ghost(ui, look, "Discard all", true) {
                let every = unstaged.iter().map(|file| file.path.clone()).collect();
                cx.send(Message::Action(Action::Discard(every)));
            }
        });
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
        header(ui, cx, &format!("Staged ({})", staged.len()), |ui| {
            if !staged.is_empty() && btn(ui, look, "Unstage all", true) {
                cx.send(Message::Run(Command::UnstageAll));
            }
        });
    });
    cut(ui, weight(2.0), &|ui| {
        list(ui, cx, tab, Area::Staged, &staged, "Nothing staged yet")
    });
    cut(ui, 1.0, &|ui| fill(ui, ui.max_rect(), c.border));

    region(ui, bottom, |ui| commit_box(ui, cx, tab));
}

fn detail(ui: &mut Ui, cx: &Cx, tab: &RepoTab, width: f32) {
    let look = cx.look();
    let c = &look.c;
    match &tab.focus {
        Some((Area::Conflicts, path)) => {
            fill(ui, ui.max_rect(), c.bg);
            let resolve = |side: Option<&str>| {
                Message::Action(Action::ConflictResolve {
                    path: path.clone(),
                    side: side.map(str::to_owned),
                })
            };
            let rect = ui.max_rect();
            centered(ui, rect, 150.0, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 12.0);
                icon::icon(ui, "alert", 32.0, c.danger);
                text(ui, format!("{path} has conflicts"), 15.0, c.text);
                muted(
                    ui,
                    look,
                    "Resolve the conflicting sections, then the file is marked as resolved.",
                    13.0,
                );
                ui.horizontal(|ui| {
                    if primary(ui, look, "Open merge tool", true) {
                        cx.send(Message::OpenConflict(path.clone()));
                    }
                    if btn(ui, look, "Use ours", true) {
                        cx.send(resolve(Some("ours")));
                    }
                    if btn(ui, look, "Use theirs", true) {
                        cx.send(resolve(Some("theirs")));
                    }
                    if btn(ui, look, "Mark as resolved", true) {
                        cx.send(resolve(None));
                    }
                });
            });
        }
        Some((area, path)) => {
            let unstaged = *area == Area::Unstaged;
            let actions = |ui: &mut Ui| {
                if unstaged {
                    if btn(ui, look, "Stage file", true) {
                        cx.send(Message::Run(Command::Stage {
                            paths: vec![path.clone()],
                        }));
                    }
                    if danger_text(ui, look, "Discard file", true) {
                        cx.send(Message::Action(Action::Discard(vec![path.clone()])));
                    }
                } else if btn(ui, look, "Unstage file", true) {
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
                "Select a file to see its changes"
            } else {
                "Working tree is clean"
            };
            placeholder(ui, look, note);
        }
    }
}
