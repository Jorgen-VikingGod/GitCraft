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

// 2. Python environment (backend/.venv, managed by uv) with the bundling group.
// uv installs the project editable, so the environment keeps pointing at the source tree.
run('uv', ['sync', '--group', 'bundle'], backend);

// 3. Freeze the server
run(
  'uv',
  ['run', '--group', 'bundle', 'pyinstaller', '--noconfirm', '--distpath', 'dist', '--workpath', 'build', join('packaging', 'gitcraft-server.spec')],
  backend,
);
console.log(`\nServer bundle ready: ${join(backend, 'dist', 'gitcraft-server')}`);
