#!/usr/bin/env python3
"""Takes the README screenshots again, the same way every time.

For each picture it builds Aurora afresh (make-demo-repo.py), starts GitCraft on it with a
settings folder of its own, opens what the picture shows (GITCRAFT_SHOT_OPEN) and lets the app
save its window (GITCRAFT_SHOT). The pictures land in docs/screenshots/.

    python3 docs/screenshots/take-screenshots.py                  # build, then take them all
    python3 docs/screenshots/take-screenshots.py history-dark     # only these
    python3 docs/screenshots/take-screenshots.py --no-build --out /tmp/shots
    python3 docs/screenshots/take-screenshots.py --list

When the README gets a new picture, add it to SHOTS below (and GITCRAFT_SHOT_OPEN in gui.rs if
it needs something opened), so the next run takes it the same way.

The window opens at its default size (1280 x 800). The README's pictures are 1920 x 1200, so
take them on a screen scaled to 150 %; the script warns when a picture comes out another size.
Pillow, if installed, saves them as RGB without the alpha channel, which keeps them smaller.
"""

import argparse
import os
import shutil
import subprocess
import sys
import struct

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HERE = os.path.join(ROOT, "docs", "screenshots")
WORK = os.path.join(ROOT, "target", "screenshots")
REPO = os.path.join(WORK, "aurora")
EXPECTED_SIZE = (1920, 1200)

# name: the file in docs/screenshots (without .png)
# theme: "dark" or "light"
# open: GITCRAFT_SHOT_OPEN, or None for the history as it opens
# env: more environment for the app; "{base}" becomes the commit feature/solar-power started from
SHOTS = [
    {"name": "history-dark", "theme": "dark", "open": None},
    {"name": "history-light", "theme": "light", "open": None},
    {"name": "local-changes", "theme": "light", "open": "changes"},
    {
        "name": "interactive-rebase",
        "theme": "dark",
        "open": "rebase",
        "env": {
            # The six commits of feature/solar-power, with one of each action.
            "GITCRAFT_SHOT_REBASE_BASE": "{base}",
            "GITCRAFT_SHOT_REBASE_ACTIONS": "pick,reword,squash,edit,drop,pick",
        },
    },
    {"name": "commit-menu", "theme": "dark", "open": "branch-menu"},
    {"name": "settings", "theme": "dark", "open": "settings"},
]


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def git(*args):
    return run("git", "-C", REPO, *args, capture_output=True, text=True).stdout.strip()


def build():
    print("Building the release binary …")
    run("cargo", "build", "--release", "-p", "gitcraft-ui", cwd=ROOT)


def binary():
    name = "gitcraft.exe" if os.name == "nt" else "gitcraft"
    path = os.path.join(ROOT, "target", "release", name)
    if not os.path.exists(path):
        sys.exit(f"{path} is missing: run without --no-build first.")
    return path


def make_repo():
    run(sys.executable, os.path.join(HERE, "make-demo-repo.py"), REPO,
        stdout=subprocess.DEVNULL)


def png_size(path):
    with open(path, "rb") as file:
        head = file.read(24)
    return struct.unpack(">II", head[16:24])


def save(picture, target):
    """Copies the picture into place, as RGB when Pillow is there."""
    try:
        from PIL import Image
    except ImportError:
        shutil.copyfile(picture, target)
        return
    Image.open(picture).convert("RGB").save(target, optimize=True)


def take(shot, app, out):
    make_repo()
    base = git("merge-base", "HEAD", "main")
    config = os.path.join(WORK, "config", shot["name"])
    shutil.rmtree(config, ignore_errors=True)
    os.makedirs(config)
    with open(os.path.join(config, "config.toml"), "w", encoding="utf-8") as file:
        file.write(f'[general]\nlanguage = "auto"\n\n[appearance]\ntheme = "{shot["theme"]}"\n')
    raw = os.path.join(WORK, f"{shot['name']}.raw.png")
    if os.path.exists(raw):
        os.remove(raw)
    env = dict(os.environ)
    # Settings from the caller's shell must not change the pictures.
    for name in ("GITCRAFT_SHOT_OPEN", "GITCRAFT_SHOT_REBASE_BASE",
                 "GITCRAFT_SHOT_REBASE_ACTIONS", "GITCRAFT_SYSTEM_FRAME", "CRAFT_TITLEBAR"):
        env.pop(name, None)
    env.update({
        "GITCRAFT_CONFIG_DIR": config,
        "GITCRAFT_SHOT": raw,
        "GITCRAFT_TITLEBAR": "app",
        "LANG": "en_US.UTF-8",
        "LC_ALL": "en_US.UTF-8",
        "LANGUAGE": "en",
    })
    if shot.get("open"):
        env["GITCRAFT_SHOT_OPEN"] = shot["open"]
    for name, value in shot.get("env", {}).items():
        env[name] = value.replace("{base}", base)
    try:
        subprocess.run([app, REPO], env=env, timeout=90, check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except subprocess.TimeoutExpired:
        print(f"  {shot['name']}: the app didn't finish within 90 s")
        return False
    if not os.path.exists(raw):
        print(f"  {shot['name']}: no picture was saved")
        return False
    target = os.path.join(out, f"{shot['name']}.png")
    save(raw, target)
    os.remove(raw)
    size = png_size(target)
    note = "" if size == EXPECTED_SIZE else (
        f"  (expected {EXPECTED_SIZE[0]} x {EXPECTED_SIZE[1]}: is the screen scaled to 150 %?)")
    print(f"  {os.path.relpath(target, ROOT)}  {size[0]} x {size[1]}{note}")
    return True


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("names", nargs="*", help="only take these pictures")
    parser.add_argument("--no-build", action="store_true", help="use the existing release binary")
    parser.add_argument("--out", default=HERE, help="where to put the pictures")
    parser.add_argument("--list", action="store_true", help="list the pictures and exit")
    args = parser.parse_args()

    if args.list:
        for shot in SHOTS:
            print(f"{shot['name']:20} {shot['theme']:6} {shot.get('open') or '-'}")
        return
    known = {shot["name"] for shot in SHOTS}
    unknown = [name for name in args.names if name not in known]
    if unknown:
        sys.exit(f"Unknown picture: {', '.join(unknown)}. Known: {', '.join(sorted(known))}")
    chosen = [shot for shot in SHOTS if not args.names or shot["name"] in args.names]

    if not args.no_build:
        build()
    app = binary()
    os.makedirs(args.out, exist_ok=True)
    print("Taking pictures …")
    failed = [shot["name"] for shot in chosen if not take(shot, app, args.out)]
    if failed:
        sys.exit(f"Failed: {', '.join(failed)}")


if __name__ == "__main__":
    main()
