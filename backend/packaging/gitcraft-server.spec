# PyInstaller spec: freezes the GitCraft server (incl. the built frontend) into
# backend/dist/gitcraft-server/ for the Electron desktop app.
#
#   cd backend && .venv/bin/pyinstaller --noconfirm packaging/gitcraft-server.spec

import os

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

backend = os.path.abspath(os.path.join(SPECPATH, ".."))
static = os.path.join(backend, "gitcraft", "static")
if not os.path.exists(os.path.join(static, "index.html")):
    raise SystemExit("Frontend not built: run `npm run build` in frontend/ first")

OPTIONAL = ("litestar.contrib", "litestar.plugins.sqlalchemy", "litestar.plugins.pydantic", "litestar.testing",
            "litestar.channels.backends.redis", "litestar.stores.redis", "litestar.stores.valkey",
            "litestar.middleware.session.server_side", "litestar.template", "litestar.plugins.htmx",
            "litestar.plugins.prometheus", "litestar.plugins.attrs", "litestar.plugins.flash",
            "litestar.plugins.problem_details", "litestar.cli")

hidden = (
    collect_submodules("litestar", filter=lambda name: not name.startswith(OPTIONAL))
    + collect_submodules("uvicorn")
    + collect_submodules("gitcraft")
)

a = Analysis(
    [os.path.join(SPECPATH, "server_entry.py")],
    pathex=[backend],
    hiddenimports=hidden,
    datas=[(static, "gitcraft/static")] + collect_data_files("litestar", excludes=["**/*.pyi"]),
    excludes=["tkinter", "pytest", "PyQt6", "IPython", "sqlalchemy", "jinja2", "mako", "redis"],
    noarchive=False,
)
pyz = PYZ(a.pure)
exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="gitcraft-server",
    console=True,  # stdout/stdin are piped by Electron; no window is shown (windowsHide)
    upx=False,
)
coll = COLLECT(exe, a.binaries, a.datas, name="gitcraft-server", upx=False)
