//! The window: runs the app's tasks, turns egui's input into messages and draws each frame.
//!
//! This is the only place that knows both egui and the async runtime. Everything else
//! either draws (`views`) or decides (`app`).

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use egui::{Event, ViewportCommand};
use futures::StreamExt;
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use crate::app::{App, Effect, Message, Pick, WindowAction};
use crate::appearance::Desktop;
use crate::task::Task;
use crate::{instance, views, window};

pub struct Shell {
    app: App,
    runtime: Runtime,
    inbox: Receiver<Message>,
    outbox: Sender<Message>,
    ctx: egui::Context,
    watchers: HashMap<String, JoinHandle<()>>,
    /// File dialogs that never block the window (craft-dialogs).
    dialogs: craft_dialogs::Dialogs<Pick>,
    desktop: Desktop,
    /// What the window currently shows, to send changes only.
    shown: Shown,
    /// `GITCRAFT_SHOT=file.png` saves a picture of the window and quits, for documentation.
    shot: Option<Shot>,
    /// When to see next whether a repository is due for its background fetch.
    next_fetch_check: std::time::Instant,
    /// What the craft-ui runtime is told before each frame (`craft.rs`).
    runtime_input: crate::craft::Runtime,
    /// The control server, when `GITCRAFT_CONTROL_PORT` asks for one (craft-automation).
    control: Option<crate::control::Control>,
}

struct Shot {
    file: std::path::PathBuf,
    frames: u32,
    asked: bool,
    /// `GITCRAFT_SHOT_REBASE_ACTIONS`: the action of each step, set once the rebase editor has
    /// loaded its commits.
    rebase_plan: Option<Vec<&'static str>>,
}

/// `GITCRAFT_SHOT_REBASE_ACTIONS=pick,reword,squash,…`: unknown names are left as pick.
fn shot_rebase_plan() -> Option<Vec<&'static str>> {
    let list = std::env::var("GITCRAFT_SHOT_REBASE_ACTIONS").ok()?;
    Some(list.split(',').map(rebase_action).collect())
}

/// A rebase step's action by name; an unknown name is pick.
fn rebase_action(name: &str) -> &'static str {
    let name = name.trim();
    ["pick", "reword", "edit", "squash", "fixup", "drop"]
        .into_iter()
        .find(|each| *each == name)
        .unwrap_or("pick")
}

#[derive(Default, PartialEq)]
struct Shown {
    title: String,
    dark: Option<bool>,
    system_dark: Option<bool>,
    measured: bool,
}

impl Shell {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        repo: Option<String>,
        frame: craft_window::policy::Frame,
        desktop: Desktop,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // The app owns zoom (Ctrl +, Ctrl -), through its settings; the craft-ui runtime applies
        // it as the interface scale.
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("the async runtime starts");
        let (outbox, inbox) = channel();
        let system_dark = desktop.dark(&cc.egui_ctx);
        let (mut app, task) = App::boot(repo, system_dark);
        app.frame = frame;
        let ctx = cc.egui_ctx.clone();
        let dialogs = craft_dialogs::Dialogs::new(craft_dialogs::Wake::new(move || {
            ctx.request_repaint();
        }));
        let shell = Self {
            app,
            runtime,
            inbox,
            outbox,
            ctx: cc.egui_ctx.clone(),
            watchers: HashMap::new(),
            dialogs,
            desktop,
            shown: Shown {
                system_dark: Some(system_dark),
                ..Shown::default()
            },
            next_fetch_check: std::time::Instant::now(),
            runtime_input: crate::craft::Runtime::default(),
            control: crate::control::Control::start(&cc.egui_ctx),
            shot: std::env::var_os("GITCRAFT_SHOT").map(|file| Shot {
                file: file.into(),
                frames: 0,
                asked: false,
                rebase_plan: shot_rebase_plan(),
            }),
        };
        shell.dispatch(task);
        shell.listen_for_launches();
        shell
    }

    /// Later launches hand their repository over to this window.
    fn listen_for_launches(&self) {
        let (outbox, ctx) = (self.outbox.clone(), self.ctx.clone());
        self.runtime.spawn(instance::listen(move |path| {
            let sent = outbox.send(Message::Instance(path)).is_ok();
            ctx.request_repaint();
            sent
        }));
    }

    /// Starts a task; its messages arrive in the inbox, and wake the window up.
    fn dispatch(&self, task: Task<Message>) {
        let Some(mut stream) = task.into_stream() else {
            return;
        };
        let (outbox, ctx) = (self.outbox.clone(), self.ctx.clone());
        self.runtime.spawn(async move {
            while let Some(message) = stream.next().await {
                // git prints progress many times a second; a line shown 30 times a second is enough.
                let progress = matches!(message, Message::Progress(..));
                if outbox.send(message).is_err() {
                    break;
                }
                if progress {
                    ctx.request_repaint_after(Duration::from_millis(33));
                } else {
                    ctx.request_repaint();
                }
            }
        });
    }

    fn handle(&mut self, message: Message) {
        if matches!(message, Message::Instance(_)) {
            self.ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        let task = self.app.update(message);
        for effect in std::mem::take(&mut self.app.effects) {
            match effect {
                Effect::Copy(text) => self.ctx.copy_text(text),
                Effect::Quit => self.ctx.send_viewport_cmd(ViewportCommand::Close),
                Effect::Ask(pick) => self.dialogs.ask(pick.dialog(), pick),
                Effect::Window(action) => {
                    let (maximized, fullscreen) = (self.app.maximized, self.app.fullscreen);
                    self.ctx.send_viewport_cmd(match action {
                        WindowAction::Minimize => ViewportCommand::Minimized(true),
                        WindowAction::ToggleMaximize => ViewportCommand::Maximized(!maximized),
                        WindowAction::ToggleFullscreen => ViewportCommand::Fullscreen(!fullscreen),
                        WindowAction::Drag => ViewportCommand::StartDrag,
                        WindowAction::Resize(edge) => ViewportCommand::BeginResize(edge),
                    });
                }
            }
        }
        self.dispatch(task);
    }

    /// Keeps one watcher per open repository.
    fn sync_watchers(&mut self) {
        self.watchers.retain(|path, handle| {
            let open = self.app.tabs.iter().any(|tab| tab.path == *path);
            if !open {
                handle.abort();
            }
            open
        });
        for tab in &self.app.tabs {
            if self.watchers.contains_key(&tab.path) {
                continue;
            }
            let (outbox, ctx) = (self.outbox.clone(), self.ctx.clone());
            let handle = self
                .runtime
                .spawn(crate::app::watch(tab.path.clone(), move |message| {
                    let sent = outbox.send(message).is_ok();
                    ctx.request_repaint();
                    sent
                }));
            self.watchers.insert(tab.path.clone(), handle);
        }
    }

    /// Reads what the window reports and tells the app about changes.
    fn read_input(&mut self, ctx: &egui::Context) {
        let size = ctx.content_rect().size();
        let (events, modifiers, pointer) = ctx.input(|input| {
            (
                input.events.clone(),
                input.modifiers,
                input.pointer.latest_pos(),
            )
        });
        let system_dark = self.desktop.dark(ctx);
        self.app.modifiers = modifiers;
        (self.app.maximized, self.app.fullscreen) = ctx.input(|input| {
            let viewport = input.viewport();
            (
                viewport.maximized.unwrap_or(false),
                viewport.fullscreen.unwrap_or(false),
            )
        });
        if let Some(position) = pointer {
            self.app.cursor = position;
        }
        if size != self.app.window {
            self.handle(Message::WindowResized(size));
        }
        if Some(system_dark) != self.shown.system_dark {
            self.shown.system_dark = Some(system_dark);
            self.handle(Message::SystemTheme(system_dark));
        }
        // Answers of file dialogs closed since the last frame.
        for (pick, answer) in self.dialogs.take_answers() {
            match answer {
                craft_dialogs::Answer::Paths(mut paths) if !paths.is_empty() => {
                    self.handle(Message::Picked(pick, paths.swap_remove(0)));
                }
                craft_dialogs::Answer::Unavailable(why) => {
                    self.handle(Message::DialogFailed(why));
                }
                _ => {}
            }
        }
        // Typing goes to the field that has the keyboard. Shortcuts with Ctrl/⌘, Escape
        // and F5 and F11 still work from fields.
        let typing = ctx.egui_wants_keyboard_input();
        for event in events {
            let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            // Ctrl+A in a field selects its text rather than the files, and Ctrl+Z / Ctrl+Y
            // undo typing there rather than the last git operation.
            let field_key = matches!(key, egui::Key::A | egui::Key::Z | egui::Key::Y);
            let global = (modifiers.command && !(typing && field_key))
                || matches!(key, egui::Key::Escape | egui::Key::F5 | egui::Key::F11);
            if global || !typing {
                self.handle(Message::Key(key, modifiers));
            }
        }
    }

    /// Asks for the picture once the first repository has had time to load.
    fn take_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        // GITCRAFT_SHOT_OPEN=menu|pull|changes|split|settings|rebase|delete|checkout|discards|clone|lfs|about|branch-menu|settings:<page> also opens that, to see how it looks.
        if shot.frames == 20
            && let Some(what) = std::env::var_os("GITCRAFT_SHOT_OPEN")
        {
            if let Some(message) = what.to_str().and_then(|what| self.open_message(what)) {
                self.handle(message);
            }
            return;
        }
        // Once the rebase editor has its commits, give them the planned actions and highlight
        // the first one again.
        if shot.rebase_plan.is_some()
            && let Some(crate::app::Overlay::Rebase(state)) = &self.app.overlay
            && !state.loading
            && !state.steps.is_empty()
        {
            let plan = shot.rebase_plan.take().unwrap_or_default();
            for (index, action) in plan.into_iter().enumerate() {
                self.handle(Message::RebaseSetAction(index, action));
            }
            self.handle(Message::RebaseFocus(0));
            ctx.request_repaint();
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        let planned = shot.rebase_plan.is_none()
            || std::env::var("GITCRAFT_SHOT_OPEN").as_deref() != Ok("rebase");
        if shot.frames > 60
            && planned
            && !shot.asked
            && self.app.current().is_some_and(|tab| !tab.loading)
        {
            shot.asked = true;
            let (file, close) = (shot.file.clone(), ctx.clone());
            ctx.request_screenshot(move |image| {
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Some(picture) =
                    image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes)
                {
                    let _ = picture.save(&file);
                }
                close.send_viewport_cmd(ViewportCommand::Close);
            });
        }
    }

    /// Runs what scripts asked for through the control server.
    fn run_control(&mut self, ctx: &egui::Context) {
        let Some(control) = &mut self.control else {
            return;
        };
        for wish in control.run(ctx, &self.app) {
            match wish {
                crate::control::Wish::Send(messages) => {
                    for message in messages {
                        self.handle(message);
                    }
                }
                crate::control::Wish::Open(what) => {
                    if let Some(message) = self.open_message(&what) {
                        self.handle(message);
                    }
                }
                crate::control::Wish::RebaseActions(actions) => {
                    for (index, action) in actions.iter().enumerate() {
                        self.handle(Message::RebaseSetAction(index, rebase_action(action)));
                    }
                    self.handle(Message::RebaseFocus(0));
                }
            }
        }
    }

    /// What `GITCRAFT_SHOT_OPEN` (and the control server's `ui.open`) opens:
    /// menu|pull|changes|split|settings|rebase|delete|checkout|discards|clone|lfs|about|branch-menu|settings:<page>.
    fn open_message(&mut self, what: &str) -> Option<Message> {
        Some(match what {
            "menu" => {
                self.app.cursor = egui::Pos2::new(420.0, 110.0);
                Message::OpenMenu(crate::actions::MenuRequest::Repository)
            }
            // The menu of the current branch on its commit, as from a right-click on its label.
            "branch-menu" => {
                let tab = self.app.current()?;
                let (sha, branch) = (tab.refs.head.clone()?, tab.current_branch()?);
                let focus = Some(crate::actions::RefTarget::Local(branch.to_owned()));
                self.app.cursor = egui::Pos2::new(420.0, 110.0);
                Message::OpenMenu(crate::actions::MenuRequest::Commit { sha, focus })
            }
            "pull" => Message::Action(crate::actions::Action::Pull),
            "changes" => Message::SetView(crate::app::View::Changes),
            "split" => Message::SetSplit(true),
            "settings" => Message::OpenSettings,
            // settings:git and so on open that page.
            page if page.starts_with("settings:") => {
                let wanted = &page["settings:".len()..];
                if let Some(found) = crate::app::SettingsPage::ALL
                    .into_iter()
                    .find(|each| format!("{each:?}").eq_ignore_ascii_case(wanted))
                {
                    self.handle(Message::SettingsPage(found));
                }
                Message::OpenSettings
            }
            "discards" => Message::OpenDiscards,
            "lfs" => Message::OpenLfsLocks,
            "about" => Message::OpenAbout,
            "clone" => Message::Action(crate::actions::Action::Clone),
            // GITCRAFT_SHOT_REBASE_BASE=<sha> rebases onto that commit instead of the root.
            "rebase" => Message::OpenRebase(
                std::env::var("GITCRAFT_SHOT_REBASE_BASE")
                    .ok()
                    .filter(|sha| !sha.is_empty()),
            ),
            // rebase:<sha> rebases onto that commit.
            plan if plan.starts_with("rebase:") => {
                Message::OpenRebase(Some(plan["rebase:".len()..].to_owned()))
            }
            "delete" => Message::Action(crate::actions::Action::DeleteBranch(
                "rust-migration".to_owned(),
            )),
            // With uncommitted changes, this asks what to do with them first.
            "checkout" => Message::Run(crate::command::Command::Checkout {
                reference: "main".to_owned(),
                kind: "local",
            }),
            _ => return None,
        })
    }

    /// Applies what changed in the app's look, title and zoom to the window.
    fn sync_window(&mut self, ctx: &egui::Context) {
        if !self.shown.measured {
            self.app.look.measure(ctx);
            self.shown.measured = true;
            self.shown.dark = None;
        }
        if self.shown.dark != Some(self.app.look.dark) {
            self.app.look.apply(ctx);
            self.shown.dark = Some(self.app.look.dark);
        }
        let title = self.app.title();
        if title != self.shown.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.shown.title = title;
        }
    }
}

impl eframe::App for Shell {
    /// Runs before `ui`, also while the window is minimized: a close goes through the platform's
    /// own quit where it has one (craft-quit).
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Here rather than in `ui`, which a minimized window may not run: the settings are
        // saved before the window goes.
        // The runtime's close path (craft-ui): the settings are saved first, then the platform's
        // own quit takes the window where it has one (craft-quit).
        self.run_control(ctx);
        craft_ui::shell::embed::logic(ctx, || {
            self.handle(Message::CloseRequested);
            craft_ui::shell::embed::CloseDecision::Close
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        window::save();
    }

    /// While taking a picture (`GITCRAFT_SHOT`), the mouse is left out: wherever it happens to
    /// rest, it would otherwise light up a row or open a submenu in the picture.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // What the runtime gathered last frame (the menu entries chosen), and what it needs for
        // this one.
        let mut chosen = Vec::new();
        craft_ui::shell::embed::before_input(ctx, |output| {
            chosen = crate::craft::Runtime::fired(&self.app, &output);
            self.runtime_input.frame_input(&mut self.app)
        });
        for message in chosen {
            self.handle(message);
        }
        if self.shot.is_some() {
            raw_input.events.retain(|event| {
                !matches!(
                    event,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::MouseWheel { .. }
                )
            });
            raw_input.events.push(egui::Event::PointerGone);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        craft_ui::shell::embed::before_ui(ui.ctx(), frame);
        let ctx = ui.ctx().clone();
        while let Ok(message) = self.inbox.try_recv() {
            self.handle(message);
        }
        self.read_input(&ctx);
        self.sync_window(&ctx);
        self.sync_watchers();
        self.take_shot(&ctx);
        window::track(&ctx);

        let messages = views::root(ui, &self.app);
        let busy = !messages.is_empty();
        for message in messages {
            self.handle(message);
        }
        if !self.app.toasts.is_empty() {
            self.handle(Message::Tick);
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        // Background fetches: the app decides which repositories are due; this only wakes it.
        if self.app.settings.history.auto_fetch_minutes > 0 {
            let now = std::time::Instant::now();
            if now >= self.next_fetch_check {
                self.handle(Message::AutoFetchTick);
                self.next_fetch_check = now + Duration::from_secs(30);
            }
            ctx.request_repaint_after(self.next_fetch_check.saturating_duration_since(now));
        }
        // An Escape that no field or menu took reaches the window as a cancel (craft-ui).
        let modifiers = self.app.modifiers;
        let mut cancelled = false;
        craft_ui::runtime::keys::with_window_intents(&ctx, false, |intents| {
            cancelled = intents.take(craft_ui::runtime::keys::Intent::Cancel);
            cancelled
        });
        if cancelled {
            self.handle(Message::Key(egui::Key::Escape, modifiers));
        }
        if busy {
            ctx.request_repaint();
        }
        if let Some(control) = &mut self.control {
            control.after_ui(&ctx);
        }
        // Last, so a dialog asked for anywhere this frame shows now, over this window.
        self.dialogs.show_pending(Some(&*frame));
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if window::rounded_for(&self.app.frame) {
            // The window is transparent outside its rounded corners.
            return [0.0; 4];
        }
        self.app.look.c.bg.to_normalized_gamma_f32()
    }
}
