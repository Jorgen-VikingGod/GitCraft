#!/usr/bin/env python3
"""Builds the made-up repository the README screenshots are taken of.

Aurora is a fictional weather-station app: about 160 commits by five invented authors over
three months, feature branches merged back with merge commits (several at once), eight
release tags, branches still open, a remote with branches of its own, a stash, a current
branch a few commits ahead of its remote, and uncommitted changes (one staged, one not).
Nothing in it is real.

    python3 docs/screenshots/make-demo-repo.py target/screenshots/aurora

The folder is deleted first if it exists. The history is the same on every run.
"""

import os
import random
import shutil
import subprocess
import sys
from datetime import datetime, timedelta, timezone

AUTHORS = [
    ("Mira Lindqvist", "mira@example.com"),
    ("Tomás Ferreira", "tomas@example.com"),
    ("Aiko Tanaka", "aiko@example.com"),
    ("Jonas Weber", "jonas@example.com"),
    ("Priya Raman", "priya@example.com"),
]

# Feature branches: name, author index, and the commits on them.
FEATURES = [
    ("feature/sensor-drivers", 0, [
        "Add a driver for the BME280 sensor",
        "Read humidity alongside temperature",
        "Retry a sensor read once after a timeout",
        "Document the sensor wiring",
    ]),
    ("feature/storage", 1, [
        "Store readings in SQLite",
        "Add a migration for the readings table",
        "Keep readings for 90 days by default",
    ]),
    ("feature/dashboard", 2, [
        "Add the dashboard page",
        "Draw the temperature chart",
        "Show the last update time",
        "Refresh the dashboard every minute",
        "Make the chart readable on small screens",
    ]),
    ("feature/alerts", 3, [
        "Add alert rules for frost and heat",
        "Send alerts by email",
        "Let an alert be snoozed for a day",
    ]),
    ("fix/clock-drift", 4, [
        "Sync the clock over NTP at start-up",
        "Log how far the clock drifted",
    ]),
    ("feature/export-csv", 1, [
        "Export readings as CSV",
        "Choose the date range of an export",
        "Name export files after the station",
    ]),
    ("feature/wind", 0, [
        "Add a driver for the anemometer",
        "Average wind speed over ten minutes",
        "Show wind direction on the dashboard",
        "Calibrate the wind vane from the settings",
    ]),
    ("feature/translations", 2, [
        "Prepare the dashboard for translations",
        "Add a German translation",
        "Add a Japanese translation",
    ]),
    ("fix/rain-gauge", 4, [
        "Count rain gauge tips without bouncing",
        "Reset the daily rain total at midnight",
    ]),
    ("feature/api", 3, [
        "Add a read-only HTTP API",
        "Serve the latest reading as JSON",
        "Page through older readings",
        "Describe the API in the README",
    ]),
]

# Earlier features, for the releases before 1.0.
EARLY = [
    ("feature/config", 1, ["Read settings from aurora.toml", "Validate the settings", "Reload settings on SIGHUP"]),
    ("feature/units", 0, ["Show temperatures in °C or °F", "Convert pressure to hPa or inHg"]),
    ("feature/history-view", 2, ["Add a page with the last 30 days", "Zoom into a day", "Compare two days"]),
    ("fix/dst", 4, ["Store readings in UTC", "Show local time across daylight saving changes"]),
    ("feature/backup", 3, ["Back up the database every night", "Keep the last seven backups", "Restore a backup from the command line"]),
    ("feature/themes", 2, ["Add a dark theme", "Follow the system's theme"]),
]

# Small maintenance commits from the long stretch before 1.0, each different.
PACKAGES = ["requests", "pytest", "sqlalchemy", "jinja2", "uvicorn", "pydantic", "ruff", "httpx"]
MAINTENANCE = iter(
    [f"Bump {package} to {major}.{minor}" for minor in range(4, 9) for major, package
     in zip([2, 8, 2, 3, 0, 2, 0, 0], PACKAGES)]
    + [f"Fix #{number}: {what}" for number, what in zip(range(12, 60, 3), [
        "readings stop after a day", "chart shows the wrong week", "export misses the last row",
        "crash without a network", "alert sent twice", "wrong humidity on cold mornings",
        "dashboard blank in Safari", "time zone ignored in exports", "database locked at start-up",
        "rain total doubled after a restart", "wind gusts capped at 99", "settings page too wide",
        "API returns readings out of order", "service fails to restart", "log file never shrinks",
        "sensor names lost on upgrade",
    ])]
)


# Commits straight on main between the merges, used in this order and never twice.
CHORES = iter([
    "Run the tests on every push",
    "Pin the Python version",
    "Fix a typo in the README",
    "Tidy up the logging",
    "Clarify the installation steps",
    "Speed up the test suite",
    "Rename the config file to aurora.toml",
    "Lower the default polling interval",
    "Fix flaky timing in a test",
    "Update the dependencies",
    "Add a changelog",
    "Explain the config options in the README",
    "Log the station name at start-up",
    "Check the config file at start-up",
    "Move the defaults into one place",
    "Add a systemd service file",
    "Describe running Aurora on a Raspberry Pi",
    "Drop support for Python 3.9",
    "Format the code with a single style",
    "Report the version with --version",
    "Show a clear error when no sensor answers",
    "Write the log to a file as well",
    "Rotate the log file every week",
    "Add a contributors section to the README",
    "Cache the dashboard's static files",
    "Update the dependencies again",
    "Fix the units in the README's example",
    "Reduce memory use of the chart",
    "Run the tests on Python 3.13 too",
    "Add an example config for two stations",
    "Clean up unused imports",
    "Release notes for 1.1",
    "Handle a missing config file gracefully",
    "Mention the API in the changelog",
    "Bump the version to 1.2.0.dev0",
    "Retry the database connection at start-up",
    "Make the log messages shorter",
    "Fix the dashboard's favicon",
    "Explain how to report a bug",
    "Check the sensor wiring in the self-test",
])


def chore():
    commit(next(CHORES), rng.randrange(len(AUTHORS)), CHORE_FILES)


# Commits on main touch only these; each branch has files of its own, so merges never conflict.
CHORE_FILES = ["README.md", "pyproject.toml", ".github/workflows/tests.yml", "docs/setup.md"]


def branch_files(branch):
    """The files a branch works on: its module and its tests."""
    slug = branch.split("/", 1)[1].replace("-", "_")
    return [f"aurora/{slug}.py", f"tests/test_{slug}.py"]


rng = random.Random(7)
clock = datetime(2026, 5, 4, 9, 0, tzinfo=timezone.utc)


def git(*args, env=None):
    subprocess.run(["git", *args], cwd=repo, check=True, env=env,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def tick(hours=None):
    global clock
    clock += timedelta(hours=hours if hours is not None else rng.uniform(2, 30))
    return clock.isoformat()


def change(path, message):
    """A few lines that look like the file's kind of content, for the diffs."""
    if path.endswith(".py"):
        words = [w for w in message.lower().replace("'", "").split() if w.isalnum()][:4]
        name = "_".join(words) or "step"
        limit = rng.choice([3, 10, 30, 60, 90])
        return (
            f"\n\ndef {name}(station, readings):\n"
            f'    """{message}."""\n'
            f"    recent = [r for r in readings if r.age_minutes < {limit}]\n"
            f"    if not recent:\n"
            f"        station.log.warning(\"no readings in the last {limit} minutes\")\n"
            f"        return None\n"
            f"    return sum(r.value for r in recent) / len(recent)\n"
        )
    if path.endswith(".md"):
        return f"\n- {message}.\n"
    return f"# {message}\n"


def commit(message, author, files, body=None, every=False):
    name, email = AUTHORS[author]
    for path in files if every else rng.sample(files, rng.randint(1, len(files))):
        full = os.path.join(repo, path)
        os.makedirs(os.path.dirname(full) or repo, exist_ok=True)
        with open(full, "a", encoding="utf-8") as out:
            out.write(change(path, message))
        git("add", path)
    when = tick()
    env = dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email, GIT_AUTHOR_DATE=when,
               GIT_COMMITTER_NAME=name, GIT_COMMITTER_EMAIL=email, GIT_COMMITTER_DATE=when)
    git("commit", "-q", "-m", message, *(["-m", body] if body else []), env=env)


def merge(branch, author):
    name, email = AUTHORS[author]
    when = tick(rng.uniform(1, 6))
    env = dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email, GIT_AUTHOR_DATE=when,
               GIT_COMMITTER_NAME=name, GIT_COMMITTER_EMAIL=email, GIT_COMMITTER_DATE=when)
    git("merge", "--no-ff", "-q", "-m", f"Merge branch '{branch}'", branch, env=env)


repo = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/screenshots/aurora")
remote = repo + "-origin.git"
for path in (repo, remote):
    shutil.rmtree(path, ignore_errors=True)
os.makedirs(repo)
git("init", "-q", "-b", "main")
git("config", "user.name", AUTHORS[0][0])
git("config", "user.email", AUTHORS[0][1])
git("config", "commit.gpgsign", "false")
git("config", "core.autocrlf", "false")

commit("Start Aurora, a home weather station", 0, ["README.md", "pyproject.toml"], every=True)
commit("Add the project layout", 0, ["aurora/__init__.py", "aurora.toml", "docs/setup.md"],
       every=True)

def start(*names):
    for name in names:
        git("branch", name, "main")


def work(name, author, message):
    git("switch", "-q", name)
    topic = name.split("/", 1)[1].replace("-", " ")
    kind = "fix" if name.startswith("fix/") else "work"
    commit(message, author, branch_files(name), f"Part of the {topic} {kind}, on {name}.")
    git("switch", "-q", "main")


def finish(name, author):
    merge(name, author)
    git("branch", "-q", "-d", name)


FEATURE = {name: (author, messages) for name, author, messages in FEATURES}

# The first releases: features two at a time, the second starting before the first is
# merged, so the graph has lanes side by side.
for pair, tag in [(FEATURES[0:2], "v0.1.0"), (FEATURES[2:4], "v0.2.0"), (FEATURES[4:6], "v0.3.0")]:
    start(*(name for name, _, _ in pair))
    for step in range(max(len(messages) for _, _, messages in pair)):
        for name, author, messages in pair:
            if step < len(messages):
                work(name, author, messages[step])
        if step == 1:
            chore()
    for name, author, _ in pair:
        finish(name, author)
    for _ in range(rng.randint(2, 3)):
        chore()
    git("tag", tag)

# Three more releases, then the long stretch of maintenance before 1.0.
for pair, tag in [(EARLY[0:2], "v0.4.0"), (EARLY[2:4], "v0.5.0"), (EARLY[4:6], "v0.6.0")]:
    start(*(name for name, _, _ in pair))
    for step in range(max(len(messages) for _, _, messages in pair)):
        for name, author, messages in pair:
            if step < len(messages):
                work(name, author, messages[step])
    for name, author, _ in pair:
        finish(name, author)
    for _ in range(rng.randint(1, 2)):
        chore()
    git("tag", tag)
for message in MAINTENANCE:
    commit(message, rng.randrange(len(AUTHORS)), CHORE_FILES)
for _ in range(6):
    chore()
git("tag", "v1.0.0")

# A remote with main and the tags, and a branch only it has.
subprocess.run(["git", "init", "-q", "--bare", remote], check=True)
git("remote", "add", "origin", remote)
git("push", "-q", "origin", "main", "--tags")
git("branch", "-q", "--set-upstream-to=origin/main", "main")
git("switch", "-q", "-c", "feature/forecast")
for message in ["Fetch the forecast from the weather service", "Show tomorrow's forecast"]:
    commit(message, 2, branch_files("feature/forecast"),
           "Part of the forecast work, on feature/forecast.")
git("push", "-q", "origin", "feature/forecast")
git("switch", "-q", "main")
git("branch", "-q", "-D", "feature/forecast")

# The busy weeks before 1.1: several branches at once, some merged, some still open.
start("feature/wind", "feature/translations", "fix/rain-gauge", "feature/api",
      "fix/sensor-timeout")
queues = {name: list(FEATURE[name][1]) for name in
          ["feature/wind", "feature/translations", "fix/rain-gauge", "feature/api"]}
queues["fix/sensor-timeout"] = ["Give up on a sensor after three timeouts"]
authors = {name: FEATURE.get(name, (4, None))[0] for name in queues}
round_ = 0
while any(queues.values()):
    for name, queue in queues.items():
        if queue and rng.random() < 0.8:
            work(name, authors[name], queue.pop(0))
    if round_ % 2 == 0:
        chore()
    if round_ == 3:
        finish("fix/rain-gauge", 4)
        queues.pop("fix/rain-gauge")
        # Someone tried a change on main and put it aside.
        with open(os.path.join(repo, "aurora.toml"), "a", encoding="utf-8") as out:
            out.write("poll_seconds = 30\n")
        when = tick()
        name, email = AUTHORS[3]
        git("stash", "push", "-q", "-m", "Try a shorter polling interval",
            env=dict(os.environ, GIT_AUTHOR_NAME=name, GIT_AUTHOR_EMAIL=email,
                     GIT_AUTHOR_DATE=when, GIT_COMMITTER_NAME=name,
                     GIT_COMMITTER_EMAIL=email, GIT_COMMITTER_DATE=when))
    round_ += 1
finish("feature/translations", 2)
chore()
finish("feature/wind", 0)
git("tag", "v1.1.0")
git("push", "-q", "origin", "main", "--tags")
git("push", "-q", "origin", "feature/api")

# The current branch, started after 1.1: pushed half-way, then worked on, so it is ahead of
# its remote branch. Main gets one more commit meanwhile.
SOLAR = [
    "Read the battery voltage",
    "Sleep between readings when the battery is low",
    "Report the battery level on the dashboard",
    "Add a low-battery alert",
    "Show the solar panel's output",
    "Warn before the battery runs out",
]
git("switch", "-q", "-c", "feature/solar-power")
for message in SOLAR:
    commit(message, 3, branch_files("feature/solar-power"),
           "Part of the solar power work, on feature/solar-power.")
    if message == SOLAR[2]:
        git("push", "-q", "-u", "origin", "feature/solar-power")
    if message == SOLAR[3]:
        git("switch", "-q", "main")
        chore()
        git("switch", "-q", "feature/solar-power")

# Uncommitted work: one change staged, one not.
with open(os.path.join(repo, "aurora/solar_power.py"), "a", encoding="utf-8") as out:
    out.write(change("x.py", "Estimate the hours of battery left"))
git("add", "aurora/solar_power.py")
with open(os.path.join(repo, "README.md"), "a", encoding="utf-8") as out:
    out.write(change("README.md", "Describe the solar panel's wiring"))
print(repo)
