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

use crate::app::{App, Effect, Message, WindowAction};
use crate::task::Task;
use crate::theme::Look;
use crate::{instance, views};

pub struct Shell {
    app: App,
    runtime: Runtime,
    inbox: Receiver<Message>,
    outbox: Sender<Message>,
    ctx: egui::Context,
    watchers: HashMap<String, JoinHandle<()>>,
    /// What the window currently shows, to send changes only.
    shown: Shown,
    /// `GITCRAFT_SHOT=file.png` saves a picture of the window and quits, for documentation.
    shot: Option<Shot>,
}

struct Shot {
    file: std::path::PathBuf,
    frames: u32,
    asked: bool,
}

#[derive(Default, PartialEq)]
struct Shown {
    title: String,
    dark: Option<bool>,
    zoom: f32,
    system_theme: Option<egui::Theme>,
    measured: bool,
}

impl Shell {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        repo: Option<String>,
        look: Look,
        client_decorations: bool,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        // The app owns zoom (Ctrl +, Ctrl -), through its settings.
        cc.egui_ctx
            .options_mut(|options| options.zoom_with_keyboard = false);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("the async runtime starts");
        let (outbox, inbox) = channel();
        let (mut app, task) = App::boot(repo, look);
        app.csd = client_decorations;
        let shell = Self {
            app,
            runtime,
            inbox,
            outbox,
            ctx: cc.egui_ctx.clone(),
            watchers: HashMap::new(),
            shown: Shown::default(),
            shot: std::env::var_os("GITCRAFT_SHOT").map(|file| Shot {
                file: file.into(),
                frames: 0,
                asked: false,
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
                if outbox.send(message).is_err() {
                    break;
                }
                ctx.request_repaint();
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
                Effect::Window(action) => {
                    let maximized = self.app.maximized;
                    self.ctx.send_viewport_cmd(match action {
                        WindowAction::Minimize => ViewportCommand::Minimized(true),
                        WindowAction::ToggleMaximize => ViewportCommand::Maximized(!maximized),
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
        let (events, modifiers, pointer, closing, system) = ctx.input(|input| {
            (
                input.events.clone(),
                input.modifiers,
                input.pointer.latest_pos(),
                input.viewport().close_requested(),
                input.raw.system_theme,
            )
        });
        self.app.modifiers = modifiers;
        self.app.maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        if let Some(position) = pointer {
            self.app.cursor = position;
        }
        if size != self.app.window {
            self.handle(Message::WindowResized(size));
        }
        if closing {
            self.handle(Message::CloseRequested);
        }
        if system != self.shown.system_theme {
            let first = self.shown.system_theme.is_none();
            self.shown.system_theme = system;
            if !first {
                self.handle(Message::ThemeChanged);
            }
        }
        // Typing goes to the field that has the keyboard. Shortcuts with Ctrl/⌘, Escape
        // and F5 still work from fields.
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
            let global = modifiers.command || matches!(key, egui::Key::Escape | egui::Key::F5);
            if global || !typing {
                self.handle(Message::Key(key, modifiers));
            }
        }
    }

    /// Asks for the picture once the first repository has had time to load.
    fn take_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        // GITCRAFT_SHOT_OPEN=menu|pull|changes|split|settings|rebase|delete also opens that, to see how it looks.
        if shot.frames == 20
            && let Some(what) = std::env::var_os("GITCRAFT_SHOT_OPEN")
        {
            let message = match what.to_str() {
                Some("menu") => {
                    self.app.cursor = egui::Pos2::new(420.0, 110.0);
                    Message::OpenMenu(crate::actions::MenuRequest::Repository)
                }
                Some("pull") => Message::Action(crate::actions::Action::Pull),
                Some("changes") => Message::SetView(crate::app::View::Changes),
                Some("split") => Message::SetSplit(true),
                Some("settings") => Message::OpenSettings,
                Some("rebase") => Message::OpenRebase(None),
                Some("delete") => Message::Action(crate::actions::Action::DeleteBranch(
                    "rust-migration".to_owned(),
                )),
                _ => return,
            };
            self.handle(message);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        if shot.frames > 60 && !shot.asked && self.app.current().is_some_and(|tab| !tab.loading) {
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
        if (self.shown.zoom - self.app.settings.zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(self.app.settings.zoom);
            self.shown.zoom = self.app.settings.zoom;
        }
    }
}

impl eframe::App for Shell {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        while let Ok(message) = self.inbox.try_recv() {
            self.handle(message);
        }
        self.read_input(&ctx);
        self.sync_window(&ctx);
        self.sync_watchers();
        self.take_shot(&ctx);

        let messages = views::root(ui, &self.app);
        let busy = !messages.is_empty();
        for message in messages {
            self.handle(message);
        }
        if !self.app.toasts.is_empty() {
            self.handle(Message::Tick);
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if busy {
            ctx.request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.app.csd {
            // The window is transparent outside the rounded corners of the title bar.
            return [0.0; 4];
        }
        self.app.look.c.bg.to_normalized_gamma_f32()
    }
}
