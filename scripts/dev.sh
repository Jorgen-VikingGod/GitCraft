#!/usr/bin/env bash
# Development mode: backend with auto-reload on :7420 plus the Angular dev
# server on :4200 (which proxies /api to the backend).
# Then open http://localhost:4200/?token=dev
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
export GITCRAFT_TOKEN="${GITCRAFT_TOKEN:-dev}"

if [ ! -x "$root/backend/.venv/bin/uvicorn" ]; then
  echo "Backend virtualenv missing. Run: cd backend && python3 -m venv .venv && .venv/bin/pip install -e '.[dev]'" >&2
  exit 1
fi

(cd "$root/backend" && exec .venv/bin/uvicorn gitcraft.app:dev_app --factory --reload \
  --host 127.0.0.1 --port 7420) &
backend=$!
trap 'kill "$backend" 2>/dev/null' EXIT

echo "GitCraft dev: open http://localhost:4200/?token=$GITCRAFT_TOKEN"
cd "$root/frontend"
npx ng serve
