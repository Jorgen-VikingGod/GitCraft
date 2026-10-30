//! Temporary stand-in for the Python server, with the same HTTP and WebSocket contract, so that
//! the golden tests and the Angular UI can run against `gitcraft-core`. Deleted at cutover.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use gitcraft_core::model::RebaseStep;
use gitcraft_core::runner::Progress;
use gitcraft_core::store::RepoStore;
use gitcraft_core::watcher::{self, WatchEvent};
use gitcraft_core::{Error, editor, ops, paths, queries, repos, runner};
use serde_json::{Value, json};
use tokio::sync::broadcast;

struct App {
    token: String,
    store: Mutex<RepoStore>,
    hub: broadcast::Sender<Value>,
}

type Shared = Arc<App>;

enum Failure {
    Core(Error),
    Status(u16, &'static str),
    Bad(String),
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Core(error)
    }
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Self::Core(error.into())
    }
}

fn json_response(status: u16, body: &Value) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        body.to_string(),
    )
        .into_response()
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        match self {
            Self::Core(Error::Git(error)) => json_response(
                400,
                &json!({"error": error.to_string(), "command": error.command(), "status_code": 400}),
            ),
            Self::Core(Error::Invalid(message)) => {
                json_response(400, &json!({"error": message, "status_code": 400}))
            }
            Self::Core(Error::Validation(message)) | Self::Bad(message) => json_response(
                400,
                &json!({"error": format!("400: {message}"), "status_code": 400}),
            ),
            Self::Status(code, detail) => {
                json_response(code, &json!({"detail": detail, "status_code": code}))
            }
        }
    }
}

type Answer = Result<Value, Failure>;

struct Req<'a> {
    query: &'a HashMap<String, String>,
    body: &'a Value,
}

impl Req<'_> {
    fn q(&self, key: &str) -> Result<&str, Failure> {
        self.query
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| Failure::Bad(format!("Missing required query parameter '{key}'")))
    }
    fn q_opt(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(String::as_str)
    }
    fn q_int(&self, key: &str, default: i64) -> Result<i64, Failure> {
        match self.query.get(key) {
            Some(value) => value
                .parse()
                .map_err(|_| Failure::Bad(format!("Invalid value for {key}"))),
            None => Ok(default),
        }
    }
    fn q_bool(&self, key: &str, default: bool) -> bool {
        self.query.get(key).map_or(default, |value| {
            matches!(value.as_str(), "true" | "True" | "1")
        })
    }
    fn s(&self, key: &str) -> Result<&str, Failure> {
        self.body
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| Failure::Bad("Validation failed".to_owned()))
    }
    fn s_opt(&self, key: &str) -> Option<&str> {
        self.body.get(key).and_then(Value::as_str)
    }
    fn b(&self, key: &str, default: bool) -> bool {
        self.body
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }
    fn list(&self, key: &str) -> Result<Vec<String>, Failure> {
        let items = self
            .body
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| Failure::Bad("Validation failed".to_owned()))?;
        Ok(items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect())
    }
}

fn progress(app: &Shared, op_id: Option<&str>, repo: Option<&str>) -> Option<Progress> {
    let op_id = op_id?.to_owned();
    let repo = repo.map(str::to_owned);
    let hub = app.hub.clone();
    Some(Arc::new(move |line: String| {
        let _ = hub.send(json!({"type": "progress", "opId": op_id, "repo": repo, "line": line}));
    }))
}

fn v<T: serde::Serialize>(value: T) -> Answer {
    serde_json::to_value(value).map_err(|error| Failure::Bad(error.to_string()))
}

fn repo_dir(app: &Shared, req: &Req<'_>) -> Result<String, Failure> {
    let path = paths::resolve_str(req.q("repo")?);
    if !app.store.lock().unwrap().is_known(&path) {
        return Err(Failure::Status(
            403,
            "Repository has not been opened in GitCraft",
        ));
    }
    if !std::path::Path::new(&path).is_dir() {
        return Err(Failure::Status(404, "Repository no longer exists"));
    }
    Ok(path)
}

fn add_repo(app: &Shared, path: &str) -> Answer {
    v(app.store.lock().unwrap().add(path)?)
}

#[allow(clippy::too_many_lines)]
async fn dispatch(
    app: &Shared,
    method: &Method,
    path: &str,
    req: Req<'_>,
) -> Result<Reply, Failure> {
    // Routes without a repository.
    if method == Method::GET {
        match path {
            "/health" => return Ok(json!({"status": "ok", "version": "0.1.0"}).into()),
            "/repos" => return Ok(v(app.store.lock().unwrap().list())?.into()),
            "/fs" => {
                return Ok(v(repos::list_dir(
                    req.q_opt("path"),
                    req.q_bool("hidden", false),
                )?)?
                .into());
            }
            _ => {}
        }
    } else {
        match path {
            "/repos/open" => {
                let top = repos::locate(req.s("path")?).await?;
                return Ok(add_repo(app, &top)?.into());
            }
            "/repos/forget" => {
                app.store.lock().unwrap().remove(req.s("path")?)?;
                return Ok(Value::Null.into());
            }
            "/repos/init" => {
                let created = ops::init(&paths::expand(req.s("path")?)).await?;
                return Ok(add_repo(app, &created)?.into());
            }
            "/repos/clone" => {
                let report = progress(app, req.s_opt("opId"), None);
                let url = req.s("url")?.trim();
                let parent = paths::expand(req.s("parent")?);
                let created = ops::clone(&parent, url, req.s_opt("name"), report.as_ref()).await?;
                return Ok(add_repo(app, &created)?.into());
            }
            _ => {}
        }
    }

    let repo = repo_dir(app, &req)?;
    let repo = repo.as_str();
    let context = req.q_int("context", 3)?;
    let answer = if method == Method::GET {
        match path {
            "/status" => v(queries::status(repo).await?),
            "/refs" => v(queries::refs(repo).await?),
            "/log" => {
                let limit = req.q_int("limit", 1500)?.clamp(1, 200_000) as usize;
                v(queries::log(repo, limit, req.q_bool("all", true), req.q_opt("ref")).await?)
            }
            "/commit" => v(queries::commit_detail(repo, req.q("sha")?).await?),
            "/commit/head-message" => Ok(json!({"message": queries::head_message(repo).await?})),
            "/commit/template" => Ok(json!({"template": queries::commit_template(repo).await?})),
            "/diff/commit" => v(queries::commit_diff(
                repo,
                req.q("sha")?,
                req.q("path")?,
                req.q_opt("orig_path"),
                context,
                req.q_bool("ignore_ws", false),
            )
            .await?),
            "/diff/working" => v(queries::working_diff(
                repo,
                req.q("path")?,
                req.q_bool("staged", false),
                context,
                req.q_bool("ignore_ws", false),
                req.q_opt("orig_path"),
            )
            .await?),
            "/diff/compare" => v(queries::range_diff(
                repo,
                req.q("base")?,
                req.q("target")?,
                req.q("path")?,
                context,
            )
            .await?),
            "/search" => {
                let q = req.q("q")?.trim();
                if q.is_empty() {
                    Ok(json!([]))
                } else {
                    v(
                        queries::search(repo, q, req.q_opt("mode").unwrap_or("message"), 500)
                            .await?,
                    )
                }
            }
            "/file-history" => v(queries::file_history(repo, req.q("path")?, 1000).await?),
            "/blame" => v(queries::blame(repo, req.q("path")?, req.q_opt("rev")).await?),
            "/rebase/commits" => v(queries::rebase_candidates(repo, req.q_opt("base")).await?),
            "/reflog" => {
                let limit = req.q_int("limit", 300)?.clamp(1, 5000) as usize;
                v(queries::reflog(repo, req.q_opt("ref").unwrap_or("HEAD"), limit).await?)
            }
            "/compare" => v(queries::compare(repo, req.q("base")?, req.q("target")?).await?),
            "/stash/files" => v(queries::stash_files(repo, req.q("name")?).await?),
            "/stash/diff" => {
                v(queries::stash_diff(repo, req.q("name")?, req.q("path")?, context).await?)
            }
            "/conflict" => v(ops::conflict_versions(repo, req.q("path")?).await?),
            "/patch" => return Ok(Reply::Patch(ops::format_patch(repo, req.q("sha")?).await?)),
            _ => return Err(Failure::Status(404, "Not Found")),
        }
    } else {
        let report = progress(app, req.s_opt("opId"), Some(repo));
        let report = report.as_ref();
        match path {
            "/stage" => {
                ops::stage_files(repo, &req.list("paths")?).await?;
                Ok(Value::Null)
            }
            "/stage/all" => {
                ops::stage_all(repo).await?;
                Ok(Value::Null)
            }
            "/unstage" => {
                ops::unstage_files(repo, &req.list("paths")?).await?;
                Ok(Value::Null)
            }
            "/unstage/all" => {
                ops::unstage_all(repo).await?;
                Ok(Value::Null)
            }
            "/discard" => {
                ops::discard_files(repo, &req.list("paths")?).await?;
                Ok(Value::Null)
            }
            "/selection" => {
                let raw = req
                    .body
                    .get("selection")
                    .and_then(Value::as_object)
                    .ok_or_else(|| Failure::Bad("Validation failed".to_owned()))?;
                let mut selection = BTreeMap::new();
                for (hunk, lines) in raw {
                    let hunk: usize = hunk
                        .parse()
                        .map_err(|_| Failure::Bad("Validation failed".to_owned()))?;
                    let chosen = lines.as_array().map(|lines| {
                        lines
                            .iter()
                            .filter_map(Value::as_u64)
                            .map(|l| l as usize)
                            .collect::<BTreeSet<_>>()
                    });
                    selection.insert(hunk, chosen);
                }
                let context = req.body.get("context").and_then(Value::as_i64).unwrap_or(3);
                ops::apply_selection(repo, req.s("path")?, req.s("mode")?, &selection, context)
                    .await?;
                Ok(Value::Null)
            }
            "/commit" => v(ops::commit(
                repo,
                req.s("message")?,
                req.b("amend", false),
                req.b("signOff", false),
            )
            .await?),
            "/checkout" => {
                v(
                    ops::checkout(repo, req.s("ref")?, req.s_opt("kind").unwrap_or("local"))
                        .await?,
                )
            }
            "/branch" => v(ops::create_branch(
                repo,
                req.s("name")?,
                req.s_opt("start"),
                req.b("checkout", true),
            )
            .await?),
            "/branch/rename" => v(ops::rename_branch(repo, req.s("old")?, req.s("new")?).await?),
            "/branch/delete" => {
                v(ops::delete_branch(repo, req.s("name")?, req.b("force", false)).await?)
            }
            "/branch/delete-remote" => {
                v(
                    ops::delete_remote_branch(repo, req.s("remote")?, req.s("branch")?, report)
                        .await?,
                )
            }
            "/branch/upstream" => {
                v(ops::set_upstream(repo, req.s("branch")?, req.s_opt("upstream")).await?)
            }
            "/branch/fast-forward" => {
                v(ops::fast_forward(repo, req.s("branch")?, req.s_opt("target")).await?)
            }
            "/tag" => v(ops::create_tag(
                repo,
                req.s("name")?,
                req.s_opt("target").unwrap_or("HEAD"),
                req.s_opt("message"),
            )
            .await?),
            "/tag/delete" => v(ops::delete_tag(repo, req.s("name")?).await?),
            "/tag/push" => v(ops::push_tag(
                repo,
                req.s("remote")?,
                req.s("name")?,
                req.b("delete", false),
                report,
            )
            .await?),
            "/remote" => v(ops::add_remote(repo, req.s("name")?, req.s("url")?).await?),
            "/remote/remove" => v(ops::remove_remote(repo, req.s("name")?).await?),
            "/remote/edit" => {
                v(
                    ops::edit_remote(repo, req.s("name")?, req.s_opt("newName"), req.s_opt("url"))
                        .await?,
                )
            }
            "/remote/prune" => v(ops::prune_remote(repo, req.s("name")?, report).await?),
            "/fetch" => v(ops::fetch(
                repo,
                req.s_opt("remote"),
                req.b("prune", true),
                req.b("tags", false),
                report,
            )
            .await?),
            "/pull" => v(ops::pull(
                repo,
                req.s_opt("remote"),
                req.s_opt("branch"),
                req.s_opt("mode").unwrap_or("merge"),
                req.b("autostash", false),
                report,
            )
            .await?),
            "/push" => v(ops::push(
                repo,
                req.s("remote")?,
                req.s_opt("branch"),
                req.b("setUpstream", false),
                req.b("force", false),
                req.b("tags", false),
                report,
            )
            .await?),
            "/merge" => {
                v(ops::merge(repo, req.s("ref")?, req.s_opt("mode").unwrap_or("default")).await?)
            }
            "/rebase" => v(ops::rebase(repo, req.s("onto")?, req.b("autostash", false)).await?),
            "/rebase/interactive" => {
                let steps: Vec<RebaseStep> =
                    serde_json::from_value(req.body.get("steps").cloned().unwrap_or(Value::Null))
                        .map_err(|_| Failure::Bad("Validation failed".to_owned()))?;
                v(ops::interactive_rebase(
                    repo,
                    req.s_opt("base"),
                    &steps,
                    req.b("autostash", false),
                )
                .await?)
            }
            "/cherry-pick" => v(ops::cherry_pick(repo, &req.list("shas")?).await?),
            "/revert" => v(ops::revert(
                repo,
                req.s("sha")?,
                req.body.get("mainline").and_then(Value::as_i64),
            )
            .await?),
            "/reset" => v(
                ops::reset(repo, req.s("target")?, req.s_opt("mode").unwrap_or("mixed")).await?,
            ),
            "/operation/continue" => v(ops::continue_operation(repo).await?),
            "/operation/abort" => v(ops::abort_operation(repo).await?),
            "/operation/skip" => v(ops::skip_operation(repo).await?),
            "/stash" => {
                let list = req
                    .body
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    });
                v(ops::stash_push(
                    repo,
                    req.s_opt("message"),
                    req.b("includeUntracked", true),
                    req.b("stagedOnly", false),
                    req.b("keepIndex", false),
                    list.as_deref(),
                )
                .await?)
            }
            "/stash/apply" => v(ops::stash_apply(repo, req.s("name")?, req.b("pop", false)).await?),
            "/stash/drop" => v(ops::stash_drop(repo, req.s("name")?).await?),
            "/conflict/resolve" => v(ops::resolve_conflict(
                repo,
                req.s("path")?,
                req.s_opt("content"),
                req.s_opt("side"),
            )
            .await?),
            "/checkout-file" => v(ops::checkout_file(repo, req.s("sha")?, req.s("path")?).await?),
            "/ignore" => v(ops::add_to_gitignore(repo, req.s("pattern")?).await?),
            "/clean" => v(ops::clean(
                repo,
                req.b("directories", true),
                req.b("ignored", false),
                req.b("dryRun", true),
            )
            .await?),
            "/patch/apply" => {
                v(
                    ops::apply_patch(repo, req.s("patch")?, req.s_opt("mode").unwrap_or("am"))
                        .await?,
                )
            }
            "/submodules/update" => v(ops::update_submodules(repo, report).await?),
            _ => return Err(Failure::Status(404, "Not Found")),
        }
    };
    Ok(answer?.into())
}

enum Reply {
    Json(Value),
    Patch(String),
}

impl From<Value> for Reply {
    fn from(value: Value) -> Self {
        Self::Json(value)
    }
}

fn authorised(app: &App, headers: &HeaderMap, query: &HashMap<String, String>) -> bool {
    let supplied = headers
        .get("x-gitcraft-token")
        .and_then(|value| value.to_str().ok())
        .or_else(|| query.get("token").map(String::as_str));
    supplied == Some(app.token.as_str())
}

const UNAUTHORISED: Failure = Failure::Status(401, "Missing or invalid GitCraft session token");

async fn api(
    State(app): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    body: Bytes,
) -> Response {
    if !authorised(&app, &headers, &query) {
        return UNAUTHORISED.into_response();
    }
    let parsed = if body.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    };
    let path = uri.path().strip_prefix("/api").unwrap_or(uri.path());
    let path = if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    };
    let status = if method == Method::POST { 201 } else { 200 };
    match dispatch(
        &app,
        &method,
        path,
        Req {
            query: &query,
            body: &parsed,
        },
    )
    .await
    {
        Ok(Reply::Json(value)) => json_response(status, &value),
        Ok(Reply::Patch(text)) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/x-patch")],
            text,
        )
            .into_response(),
        Err(failure) => failure.into_response(),
    }
}

async fn events(
    State(app): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !authorised(&app, &headers, &query) {
        return UNAUTHORISED.into_response();
    }
    upgrade.on_upgrade(move |socket| run_socket(socket, app))
}

async fn run_socket(mut socket: WebSocket, app: Shared) {
    let mut hub = app.hub.subscribe();
    let (sender, mut own) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let mut watchers: HashMap<String, watcher::Watcher> = HashMap::new();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(Message::Text(text))) = incoming else {
                    if matches!(incoming, Some(Ok(_))) { continue; }
                    break;
                };
                let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                if message.get("type").and_then(Value::as_str) != Some("watch") { continue; }
                let wanted: BTreeSet<String> = message.get("repos").and_then(Value::as_array).into_iter().flatten()
                    .filter_map(Value::as_str).filter(|repo| app.store.lock().unwrap().is_known(repo)).map(str::to_owned).collect();
                watchers.retain(|repo, _| wanted.contains(repo));
                for repo in wanted {
                    if watchers.contains_key(&repo) { continue; }
                    let sink = sender.clone();
                    let name = repo.clone();
                    let started = watcher::watch(&repo, move |event| {
                        let _ = sink.send(match event {
                            WatchEvent::Changed { git, worktree } => json!({"type": "changed", "repo": name, "git": git, "worktree": worktree}),
                            WatchEvent::Error(message) => json!({"type": "watch-error", "repo": name, "message": message}),
                        });
                    }).await;
                    match started {
                        Ok(handle) => { watchers.insert(repo, handle); }
                        Err(error) => { let _ = sender.send(json!({"type": "watch-error", "repo": repo, "message": error.to_string()})); }
                    }
                }
            }
            Some(message) = own.recv() => {
                if socket.send(Message::text(message.to_string())).await.is_err() { break; }
            }
            Ok(message) = hub.recv() => {
                if socket.send(Message::text(message.to_string())).await.is_err() { break; }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    editor::run_if_requested();
    let (mut host, mut port, mut stdin_exit, mut open) =
        ("127.0.0.1".to_owned(), 7420u16, false, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--host" => host = args.next().unwrap_or(host),
            "--port" => port = args.next().and_then(|p| p.parse().ok()).unwrap_or(port),
            "--no-browser" => {}
            "--exit-on-stdin-close" => stdin_exit = true,
            "--version" => {
                println!("0.1.0");
                return;
            }
            path => open = Some(path.to_owned()),
        }
    }
    if let Err(message) = runner::check_git() {
        eprintln!("error: {message}");
        std::process::exit(3);
    }
    if stdin_exit {
        std::thread::spawn(|| {
            let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
            std::process::exit(0);
        });
    }
    let token = std::env::var("GITCRAFT_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("{:x}", std::process::id() as u64 * 0x9E37_79B9));
    let mut store = RepoStore::open();
    let listener = match tokio::net::TcpListener::bind((host.as_str(), port)).await {
        Ok(listener) => listener,
        Err(_) => tokio::net::TcpListener::bind((host.as_str(), 0))
            .await
            .expect("no free port"),
    };
    let bound = listener.local_addr().expect("bound address").port();
    let mut url = format!("http://{host}:{bound}/?token={token}");
    if let Some(path) = open {
        match repos::locate(&path).await {
            Ok(top) => {
                let _ = store.add(&top);
                url.push_str(&format!("&open={top}"));
            }
            Err(_) => eprintln!("warning: {path} is not a git repository"),
        }
    }
    let app: Shared = Arc::new(App {
        token,
        store: Mutex::new(store),
        hub: broadcast::channel(1024).0,
    });
    let static_dir = std::env::var_os("GITCRAFT_STATIC")
        .map_or_else(|| PathBuf::from("backend/gitcraft/static"), PathBuf::from);
    let router = Router::new()
        .route("/api/ws", get(events))
        .route("/api/{*rest}", any(api))
        .fallback_service(tower_http::services::ServeDir::new(static_dir))
        .with_state(app);
    println!("GitCraft 0.1.0 running at {url}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
    axum::serve(listener, router).await.expect("server runs");
}
