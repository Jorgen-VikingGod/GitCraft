//! Driving the open window from scripts and tests (craft-automation): a local control server that
//! is off unless `GITCRAFT_CONTROL_PORT` is set (`0` picks a free port). A client connects to
//! `127.0.0.1:<port>`, authenticates with the token (`GITCRAFT_CONTROL_TOKEN`, or the file
//! `GITCRAFT_CONTROL_TOKEN_FILE`; without either a token is made and printed) and sends one JSON
//! request per line, `{"id", "method", "params"}`.
//!
//! The methods:
//!
//! - craft-automation's own: `ui.click`, `ui.move`, `ui.wheel`, `ui.key`, `ui.focus`, `ui.release`,
//!   `ui.ime` (input, answered once the window has handled it), `ui.find` and `ui.inspect` (the
//!   window's accessibility tree) and `ui.screenshot {path?, region?}`. A picture with a `path`
//!   goes into `GITCRAFT_CONTROL_DIR` (else `automation` in the config folder) and nowhere else.
//! - GitCraft's: `app.state` (what the window shows), `app.open {path}` (a repository),
//!   `ui.open {what}` (what `GITCRAFT_SHOT_OPEN` opens: `settings`, `settings:git`, `changes`,
//!   `rebase`…), `ui.set {theme?, zoom?}`, `ui.resize {width, height}`, `ui.menu.list`,
//!   `ui.menu.invoke {path: ["Repository", "Fetch"]}`, `rebase.actions {actions: [...]}` and
//!   `app.quit`.

use std::path::{Component, Path, PathBuf};

use craft_automation::args::{ControlOptions, EnvNames};
use craft_automation::control::{ControlChannel, ControlRequest, Reply};
use craft_automation::screenshot::{ScreenshotParams, settle_ms};
use craft_automation::server::{self, Listening};
use craft_ui::runtime::motion;
use craft_ui::runtime::windows::WindowHost;
use egui::{ViewportCommand, ViewportId};
use serde_json::{Value, json};

use crate::app::{App, Message, Overlay};
use crate::i18n::tr;
use crate::menu::MenuItem;

/// The running server and its requests.
pub struct Control {
    channel: ControlChannel,
    /// Keeps the server running; dropping it stops it.
    _listening: Listening,
    /// Where pictures with a `path` go.
    out: PathBuf,
    next_token: u64,
}

/// What a request asks of the window, beyond what `control.rs` answers itself.
pub enum Wish {
    /// Messages for the app, in order.
    Send(Vec<Message>),
    /// Opens what `GITCRAFT_SHOT_OPEN` would.
    Open(String),
    /// Gives the steps of the open rebase these actions.
    RebaseActions(Vec<String>),
}

impl Control {
    /// Starts the server when the environment asks for it.
    pub fn start(ctx: &egui::Context) -> Option<Control> {
        let names = EnvNames::for_app("GITCRAFT");
        let options = match ControlOptions::from_env(&names) {
            Ok(options) => options,
            Err(error) => {
                eprintln!("gitcraft: error: {error}");
                return None;
            }
        };
        let port = options.port?;
        let token = match options.server_token() {
            Ok(token) => token,
            Err(error) => {
                eprintln!("gitcraft: cannot configure control authentication: {error}");
                return None;
            }
        };
        eprintln!("gitcraft: {}", options.token_notice(&token));
        let (requests, listening) = match server::listen(port, token, WindowHost::of(ctx)) {
            Ok(started) => started,
            Err(error) => {
                eprintln!("gitcraft: the control server couldn't start: {error}");
                return None;
            }
        };
        eprintln!("gitcraft: control server listening on {}", listening.addr);
        let out = std::env::var_os("GITCRAFT_CONTROL_DIR").map_or_else(
            || gitcraft_core::store::config_dir().join("automation"),
            PathBuf::from,
        );
        Some(Control {
            channel: ControlChannel::new(requests),
            _listening: listening,
            out,
            next_token: 0,
        })
    }

    /// In the app's logic, before its UI: answers finished pictures, then runs the requests
    /// that arrived. What needs the app comes back as wishes, in order.
    pub fn run(&mut self, ctx: &egui::Context, app: &App) -> Vec<Wish> {
        let host = WindowHost::of(ctx);
        self.channel.captures.note_windows(&host, None);
        let finished = self.channel.captures.drain(motion::now(), None);
        let out = self.out.clone();
        self.channel.finish_captures(finished, &mut |path, bytes| {
            write_picture(&out, path, bytes)
        });
        let settle = settle_ms(ctx.global_style().animation_time);
        let mut wishes = Vec::new();
        while let Some(request) = self.channel.next_request() {
            let reply = self.answer(ctx, &host, app, &request, &mut wishes);
            self.channel.settle(request, reply, None, settle);
        }
        if self.channel.captures.any_pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        wishes
    }

    /// At the end of the UI pass: counts it for settling and takes the pictures that are due;
    /// then answers the input that has been handled.
    pub fn after_ui(&mut self, ctx: &egui::Context) {
        let host = WindowHost::of(ctx);
        if !ctx.will_discard() {
            self.channel.captures.note_windows(&host, None);
            self.channel.captures.note_pass();
            let finished = self.channel.captures.issue(&host, motion::now(), None);
            let out = self.out.clone();
            self.channel.finish_captures(finished, &mut |path, bytes| {
                write_picture(&out, path, bytes)
            });
        }
        self.channel.poll_input(&host);
    }

    fn answer(
        &mut self,
        ctx: &egui::Context,
        host: &WindowHost,
        app: &App,
        request: &ControlRequest,
        wishes: &mut Vec<Wish>,
    ) -> Reply {
        let params = &request.params;
        let viewport = ViewportId::ROOT;
        // No document canvas: a document point is never on screen.
        let map = |_: usize, _: egui::Pos2| None;
        if let Some(queued) =
            craft_automation::synthetic::input_method(&request.method, params, host, viewport, &map)
        {
            return match queued {
                Ok(()) => Reply::AfterInput,
                Err(error) => Reply::from_result(Err(error)),
            };
        }
        if let Some(found) =
            craft_automation::access::access_method(&request.method, params, host, viewport)
        {
            return Reply::from_result(found);
        }
        let done = |wish: Wish, wishes: &mut Vec<Wish>| {
            wishes.push(wish);
            ctx.request_repaint();
            Reply::from_result(Ok(Value::Null))
        };
        match request.method.as_str() {
            "ui.inspect" => {
                Reply::from_result(craft_automation::access::inspect(host, viewport, params))
            }
            "ui.screenshot" => match ScreenshotParams::from_params(params) {
                Ok(shot) => {
                    self.next_token += 1;
                    ctx.request_repaint();
                    Reply::screenshot(self.next_token, shot, viewport)
                }
                Err(error) => Reply::from_result(Err(error)),
            },
            "ui.resize" => {
                let size = (number(params, "width"), number(params, "height"));
                let (Some(width), Some(height)) = size else {
                    return Reply::from_result(Err("ui.resize needs width and height".into()));
                };
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(width, height)));
                Reply::from_result(Ok(Value::Null))
            }
            "ui.set" => {
                let mut messages = Vec::new();
                if let Some(theme) = params.get("theme").and_then(Value::as_str) {
                    if !["system", "light", "dark"].contains(&theme) {
                        return Reply::from_result(Err(format!(
                            "unknown theme `{theme}`: system, light or dark"
                        )));
                    }
                    messages.push(Message::SetTheme(theme.to_owned()));
                }
                if let Some(zoom) = number(params, "zoom") {
                    // `Zoom(0.0)` is actual size; any other factor multiplies the current zoom.
                    messages.push(Message::Zoom(0.0));
                    if (zoom - 1.0).abs() > f32::EPSILON {
                        messages.push(Message::Zoom(zoom));
                    }
                }
                done(Wish::Send(messages), wishes)
            }
            "ui.open" => match params.get("what").and_then(Value::as_str) {
                Some(what) => done(Wish::Open(what.to_owned()), wishes),
                None => Reply::from_result(Err("ui.open needs `what`".into())),
            },
            "app.open" => match params.get("path").and_then(Value::as_str) {
                Some(path) => done(Wish::Send(vec![Message::OpenPath(path.to_owned())]), wishes),
                None => Reply::from_result(Err("app.open needs `path`".into())),
            },
            "app.state" => Reply::from_result(Ok(state(app))),
            "app.quit" => done(Wish::Send(vec![Message::Quit]), wishes),
            "ui.menu.list" => Reply::from_result(Ok(menu_list(&app.bar_menus()))),
            "ui.menu.invoke" => {
                let path: Vec<&str> = params
                    .get("path")
                    .and_then(Value::as_array)
                    .map(|steps| steps.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                match menu_message(&app.bar_menus(), &path) {
                    Ok(message) => done(
                        Wish::Send(vec![Message::FromMenu(Box::new(message))]),
                        wishes,
                    ),
                    Err(error) => Reply::from_result(Err(error)),
                }
            }
            "rebase.actions" => {
                let actions = params
                    .get("actions")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                done(Wish::RebaseActions(actions), wishes)
            }
            other => Reply::from_result(Err(format!("unknown method {other}"))),
        }
    }
}

fn number(params: &Value, name: &str) -> Option<f32> {
    params.get(name).and_then(Value::as_f64).map(|v| v as f32)
}

/// Writes a picture under `out`, refusing any path that would leave it.
fn write_picture(out: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let relative = Path::new(path);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!(
            "`{path}` must be a plain path inside the automation folder"
        ));
    }
    let target = out.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    craft_atomic::atomic_write(&target, bytes).map_err(|error| error.to_string())
}

/// What the window shows, for a script to wait on and check.
fn state(app: &App) -> Value {
    let tab = app.current();
    let overlay = app.overlay.as_ref().map(|overlay| match overlay {
        Overlay::Settings => "settings",
        Overlay::About { .. } => "about",
        Overlay::Reflog { .. } => "reflog",
        Overlay::Discards { .. } => "discards",
        Overlay::LfsLocks { .. } => "lfs",
        Overlay::Blame { .. } => "blame",
        Overlay::FileHistory { .. } => "file-history",
        Overlay::Conflict(_) => "conflict",
        Overlay::Rebase(_) => "rebase",
        Overlay::Cleanup { .. } => "cleanup",
        Overlay::RangeDiff { .. } => "range-diff",
    });
    let rebase_loading = matches!(&app.overlay, Some(Overlay::Rebase(state)) if state.loading);
    json!({
        "title": app.title(),
        "tabs": app.tabs.iter().map(|tab| tab.path.clone()).collect::<Vec<_>>(),
        "active": app.active,
        "repository": tab.map(|tab| tab.path.clone()),
        "branch": tab.and_then(|tab| tab.current_branch()),
        "view": tab.map(|tab| format!("{:?}", tab.view).to_lowercase()),
        "loading": tab.is_some_and(|tab| tab.loading) || rebase_loading,
        "busy": app.busy.as_ref().map(|(_, label)| label.clone()),
        "overlay": overlay,
        "form": app.form.as_ref().map(|form| form.title.clone()),
        "menu": app.menu.is_some(),
        "toasts": app.toasts.iter().map(|toast| toast.message.clone()).collect::<Vec<_>>(),
        "theme": app.settings.appearance.theme,
        "dark": app.look.dark,
        "zoom": app.settings.appearance.zoom,
    })
}

/// The menu bar as nested lists of labels (as shown), with each entry's state.
fn menu_list(menus: &[(&'static str, Vec<MenuItem>)]) -> Value {
    fn items(list: &[MenuItem]) -> Vec<Value> {
        list.iter()
            .filter_map(|item| match item {
                MenuItem::Entry { label, message, .. } => {
                    Some(json!({"label": tr(label), "enabled": message.is_some()}))
                }
                MenuItem::Sub { label, children } => {
                    Some(json!({"label": tr(label), "items": items(children)}))
                }
                _ => None,
            })
            .collect()
    }
    Value::Array(
        menus
            .iter()
            .map(|(title, list)| json!({"label": tr(title), "items": items(list)}))
            .collect(),
    )
}

/// The message of the entry `path` names by its labels (as shown, or in English).
fn menu_message(menus: &[(&'static str, Vec<MenuItem>)], path: &[&str]) -> Result<Message, String> {
    let named = |label: &str, wanted: &str| label == wanted || tr(label) == wanted;
    let (first, rest) = path
        .split_first()
        .ok_or_else(|| "ui.menu.invoke needs a `path` of labels".to_owned())?;
    let mut list = &menus
        .iter()
        .find(|(title, _)| named(title, first))
        .ok_or_else(|| format!("no menu `{first}`"))?
        .1;
    for (at, step) in rest.iter().enumerate() {
        let last = at + 1 == rest.len();
        let found = list
            .iter()
            .find(|item| item.label().is_some_and(|label| named(label, step)))
            .ok_or_else(|| format!("no entry `{step}`"))?;
        match found {
            MenuItem::Sub { children, .. } if !last => list = children,
            MenuItem::Entry { message, .. } if last => {
                return message
                    .clone()
                    .ok_or_else(|| format!("`{step}` is disabled"));
            }
            _ => return Err(format!("`{step}` is not an entry here")),
        }
    }
    Err("ui.menu.invoke needs the menu and an entry".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_stay_in_the_automation_folder() {
        let out = tempfile::tempdir().unwrap();
        assert!(write_picture(out.path(), "a/b.png", b"png").is_ok());
        assert!(out.path().join("a/b.png").exists());
        for bad in ["../x.png", "/tmp/x.png", "a/../../x.png"] {
            assert!(write_picture(out.path(), bad, b"png").is_err(), "{bad}");
        }
    }

    #[test]
    fn a_menu_entry_is_found_by_its_labels() {
        let menus = vec![(
            "File",
            vec![
                MenuItem::entry("Open", Message::OpenDialog),
                MenuItem::sub(
                    "More",
                    vec![MenuItem::entry_if("Quit", false, Message::Quit)],
                ),
            ],
        )];
        assert!(matches!(
            menu_message(&menus, &["File", "Open"]),
            Ok(Message::OpenDialog)
        ));
        assert!(menu_message(&menus, &["File", "More", "Quit"]).is_err());
        assert!(menu_message(&menus, &["File", "More"]).is_err());
        assert!(menu_message(&menus, &["Edit", "Open"]).is_err());
    }
}
