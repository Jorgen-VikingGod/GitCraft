// Starts the desktop app in development mode (`npm start [repo]`).
// Clears ELECTRON_RUN_AS_NODE, which some hosts (e.g. VS Code extensions) set
// and which would otherwise make Electron behave like plain Node.
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const electron = require('electron'); // path to the binary when required from Node
const desktop = join(dirname(fileURLToPath(import.meta.url)), '..');

const env = { ...process.env };
delete env.ELECTRON_RUN_AS_NODE;

spawn(electron, ['.', ...process.argv.slice(2)], { cwd: desktop, env, stdio: 'inherit' }).on('exit', (code) =>
  process.exit(code ?? 0),
);
