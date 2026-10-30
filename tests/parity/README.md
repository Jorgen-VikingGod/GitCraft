# Parity harness

Records how the Python server behaves, so the Rust port can be checked against it. It is temporary and goes away with the Python backend, apart from any goldens the Rust tests still use.

```bash
cd backend && uv sync && cd ..
PY=backend/.venv/bin/python

$PY tests/parity/parity.py check                       # replay against the Python server
$PY tests/parity/parity.py check --server "<command>"  # replay against another server
$PY tests/parity/parity.py check --only pull-merge     # one scenario or mutation case
$PY tests/parity/parity.py capture                     # rewrite the goldens (Python server only)
$PY tests/parity/baseline.py                           # timings on a synthetic repository
$PY tests/parity/baseline.py --repo <path>             # timings on a real repository
```

A server must accept `--no-browser --port 0`, read `GITCRAFT_TOKEN` and print `running at <url>`.

## Layout

| Path | Contents |
|---|---|
| `scenarios.py` | Repositories with pinned dates and identities, so object ids are stable |
| `cases.py` | One case per mutating behaviour, including the failures |
| `parity.py` | Capture, replay and comparison |
| `goldens/read/` | Every read route, swept across each scenario |
| `goldens/mutate/` | Responses plus the resulting repository state, read with git itself |
| `goldens/global.json`, `store.json` | Auth, repository validation, folder browser, recent repositories |
| `baseline.py`, `baseline-python.json` | Timings and the Python results to compare with |

## Things to know

- **Goldens record behaviour, not correctness.** Some capture existing bugs on purpose, such as `selection-context-0`, `selection-latin1`, `dash-ref-checkout` and `clean-spaced-name-real`. When the port fixes one (see the migration plan's list of deliberate changes), update that golden in the same change and say why.
- **Message text** (`error`, `message`, `detail`) is only compared on the git version and OS in `goldens/manifest.json`, because git's wording varies. Everything else is always compared.
- **Status-only cases** (`status_only`) compare just the HTTP status, where the body is specific to the framework.
- **`odd_names`** is skipped on Windows, which can't store those filenames.
- **WebSocket events** are checked for their contract only, not byte for byte. Install `websockets` to run this, otherwise it is skipped.
- **Environment:** the harness isolates git config, home and the GitCraft config dir in a temp folder, and never touches your real settings.
