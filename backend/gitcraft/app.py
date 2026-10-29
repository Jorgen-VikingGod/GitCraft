"""Litestar application and the `gitcraft` command line entry point."""

from __future__ import annotations

import argparse
import asyncio
import os
import secrets
import shutil
import socket
import sys
import threading
import webbrowser
from pathlib import Path
from urllib.parse import quote

from litestar import Litestar, Request, Response, Router, get
from litestar.connection import ASGIConnection
from litestar.di import Provide
from litestar.exceptions import NotAuthorizedException
from litestar.handlers.base import BaseRouteHandler
from litestar.static_files import create_static_files_router

from . import __version__
from .git.runner import GitError
from .routes import controllers
from .routes.common import resolve_repo
from .ws import events

STATIC_DIR = Path(__file__).with_name("static")
TOKEN_HEADER = "x-gitcraft-token"


def token_guard(connection: ASGIConnection, _: BaseRouteHandler) -> None:
    expected = connection.app.state.token
    if not expected:
        return
    supplied = connection.headers.get(TOKEN_HEADER) or connection.query_params.get("token") or ""
    if not secrets.compare_digest(supplied, expected):
        raise NotAuthorizedException("Missing or invalid GitCraft session token")


def git_error_handler(_: Request, exc: GitError) -> Response:
    return Response(
        {"error": str(exc), "command": "git " + " ".join(exc.git_args), "status_code": 400}, status_code=400
    )


def value_error_handler(_: Request, exc: ValueError) -> Response:
    return Response({"error": str(exc), "status_code": 400}, status_code=400)


@get("/health")
async def health() -> dict[str, str]:
    return {"status": "ok", "version": __version__}


def create_app(token: str | None = None, static_dir: Path | None = STATIC_DIR) -> Litestar:
    api = Router(
        path="/api",
        route_handlers=[health, events, *controllers],
        guards=[token_guard],
        dependencies={"repo_dir": Provide(resolve_repo, sync_to_thread=False)},
    )
    handlers: list = [api]
    if static_dir and (static_dir / "index.html").exists():
        handlers.append(create_static_files_router(path="/", directories=[static_dir], html_mode=True))
    app = Litestar(
        route_handlers=handlers,
        exception_handlers={GitError: git_error_handler, ValueError: value_error_handler},
    )
    app.state.token = token
    return app


def dev_app() -> Litestar:
    """Factory for `uvicorn --factory --reload` during development."""
    return create_app(token=os.environ.get("GITCRAFT_TOKEN", "dev"))


def _listen(host: str, preferred: int) -> socket.socket:
    """Bind and listen before announcing the URL, so early requests queue instead of failing."""
    for port in (preferred, 0):
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            sock.bind((host, port))
            sock.listen(128)
            return sock
        except OSError:
            sock.close()
    raise RuntimeError("No free port available")


def _exit_when_stdin_closes() -> None:
    try:
        while sys.stdin.buffer.read(1024):
            pass
    finally:
        os._exit(0)


def main(argv: list[str] | None = None) -> None:
    import uvicorn

    parser = argparse.ArgumentParser(prog="gitcraft", description="Open-source Git GUI")
    parser.add_argument("path", nargs="?", help="repository to open")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=int(os.environ.get("GITCRAFT_PORT", 7420)))
    parser.add_argument("--no-browser", action="store_true", help="do not open a browser window")
    parser.add_argument(
        "--exit-on-stdin-close",
        action="store_true",
        help="exit when stdin closes (used by the desktop app so the server never outlives it)",
    )
    parser.add_argument("--version", action="version", version=__version__)
    args = parser.parse_args(argv)

    if not shutil.which("git"):
        print("error: git was not found on PATH; install git and try again", file=sys.stderr)
        sys.exit(3)
    if args.exit_on_stdin_close:
        threading.Thread(target=_exit_when_stdin_closes, daemon=True).start()

    token = os.environ.get("GITCRAFT_TOKEN") or secrets.token_urlsafe(24)
    app = create_app(token=token)
    sock = _listen(args.host, args.port)
    port = sock.getsockname()[1]

    url = f"http://{args.host}:{port}/?token={token}"
    if args.path:
        from .git import queries
        from .repos_store import store

        try:
            top = asyncio.run(queries.toplevel(str(Path(args.path).expanduser().resolve())))
            store.add(top)
            url += "&open=" + quote(top)
        except GitError:
            print(f"warning: {args.path} is not a git repository", file=sys.stderr)

    if not (STATIC_DIR / "index.html").exists():
        print("warning: frontend not built; run `npm run build` in frontend/ (API only)", file=sys.stderr)
    print(f"GitCraft {__version__} running at {url}", flush=True)
    if not args.no_browser:
        threading.Timer(1.0, webbrowser.open, args=(url,)).start()
    uvicorn.Server(uvicorn.Config(app, log_level="warning")).run(sockets=[sock])


if __name__ == "__main__":
    main()
