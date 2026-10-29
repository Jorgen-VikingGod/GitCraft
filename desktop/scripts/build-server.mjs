// Builds the Angular UI and freezes the Python server with PyInstaller into
// backend/dist/gitcraft-server, which electron-builder ships as a resource.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const isWin = process.platform === 'win32';
const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const frontend = join(root, 'frontend');
const backend = join(root, 'backend');

function run(command, args, cwd) {
  console.log(`\n> ${command} ${args.join(' ')}   (in ${cwd})`);
  const result = spawnSync(command, args, { cwd, stdio: 'inherit', shell: isWin });
  if (result.status !== 0) {
    console.error(`Command failed: ${command} ${args.join(' ')}`);
    process.exit(result.status ?? 1);
  }
}

// 1. Frontend -> backend/gitcraft/static
if (!existsSync(join(frontend, 'node_modules'))) run('npm', ['ci'], frontend);
run('npm', ['run', 'build'], frontend);

// 2. Python environment with the bundling extra
const venvPython = isWin ? join(backend, '.venv', 'Scripts', 'python.exe') : join(backend, '.venv', 'bin', 'python');
if (!existsSync(venvPython)) run(isWin ? 'python' : 'python3', ['-m', 'venv', '.venv'], backend);
// Editable install: keeps the development environment pointing at the source tree.
run(venvPython, ['-m', 'pip', 'install', '--quiet', '-e', '.[dev,bundle]'], backend);

// 3. Freeze the server
run(
  venvPython,
  ['-m', 'PyInstaller', '--noconfirm', '--distpath', 'dist', '--workpath', 'build', join('packaging', 'gitcraft-server.spec')],
  backend,
);
console.log(`\nServer bundle ready: ${join(backend, 'dist', 'gitcraft-server')}`);
