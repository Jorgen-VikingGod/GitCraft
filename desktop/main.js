// GitCraft desktop shell: starts the bundled GitCraft server (a frozen Python
// executable, or the backend virtualenv during development) and shows its UI
// in a native window.
'use strict';

const { app, BrowserWindow, Menu, dialog, ipcMain, nativeTheme, shell } = require('electron');
const { spawn, execFileSync } = require('node:child_process');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const isMac = process.platform === 'darwin';
const isWin = process.platform === 'win32';
const STARTUP_TIMEOUT_MS = 30000;

/** @type {import('node:child_process').ChildProcess | null} */
let server = null;
/** Base URL of the running server, e.g. http://127.0.0.1:51234 */
let serverOrigin = null;
let serverToken = null;
/** @type {BrowserWindow | null} */
let mainWindow = null;
let quitting = false;

// ------------------------------------------------------------------ helpers

/** Repository path passed on the command line, if any. */
function repoFromArgs(argv) {
  const args = argv.slice(app.isPackaged ? 1 : 2).filter((a) => !a.startsWith('-'));
  const candidate = args[args.length - 1];
  if (!candidate) return null;
  const resolved = path.resolve(candidate);
  return fs.existsSync(resolved) && fs.statSync(resolved).isDirectory() ? resolved : null;
}

/**
 * GUI apps on macOS/Linux do not inherit the login shell PATH, so git from
 * Homebrew etc. (and its credential helpers) would not be found. Ask the login
 * shell for its PATH once and merge in common locations as a fallback.
 */
function fixPath() {
  if (isWin) return;
  const extra = ['/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'];
  let shellPath = '';
  try {
    const sh = process.env.SHELL || '/bin/sh';
    shellPath = execFileSync(sh, ['-ilc', 'printf "__PATH__%s" "$PATH"'], { encoding: 'utf8', timeout: 3000 })
      .split('__PATH__')
      .pop()
      .trim();
  } catch {
    // Fall back to the defaults below.
  }
  const parts = [...shellPath.split(':'), ...(process.env.PATH || '').split(':'), ...extra].filter(Boolean);
  process.env.PATH = [...new Set(parts)].join(':');
}

function serverCommand() {
  if (process.env.GITCRAFT_SERVER) return { command: process.env.GITCRAFT_SERVER, args: [] };
  if (app.isPackaged) {
    const exe = path.join(process.resourcesPath, 'server', isWin ? 'gitcraft-server.exe' : 'gitcraft-server');
    return { command: exe, args: [] };
  }
  // Development: run the backend from its virtualenv.
  const venv = path.join(__dirname, '..', 'backend', '.venv');
  const python = isWin ? path.join(venv, 'Scripts', 'python.exe') : path.join(venv, 'bin', 'python');
  return { command: python, args: ['-m', 'gitcraft.app'] };
}

// ------------------------------------------------------------------- server

function startServer(repo) {
  return new Promise((resolve, reject) => {
    const { command, args } = serverCommand();
    serverToken = crypto.randomBytes(24).toString('base64url');
    const fullArgs = [...args, '--no-browser', '--port', '0', '--exit-on-stdin-close'];
    if (repo) fullArgs.push(repo);

    const child = spawn(command, fullArgs, {
      env: { ...process.env, GITCRAFT_TOKEN: serverToken, PYTHONUNBUFFERED: '1' },
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });
    server = child;

    let stderr = '';
    let settled = false;
    const fail = (message) => {
      if (settled) return;
      settled = true;
      reject(new Error(message));
    };
    const timer = setTimeout(() => fail('The GitCraft server did not start in time.\n\n' + stderr), STARTUP_TIMEOUT_MS);

    let buffer = '';
    child.stdout.on('data', (chunk) => {
      buffer += chunk.toString();
      const match = buffer.match(/running at (http:\/\/[^\s]+)/);
      if (match && !settled) {
        settled = true;
        clearTimeout(timer);
        resolve(new URL(match[1]));
      }
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk.toString();
      if (stderr.length > 20000) stderr = stderr.slice(-20000);
    });
    child.on('error', (error) => {
      clearTimeout(timer);
      fail(`Could not start the GitCraft server (${command}):\n${error.message}`);
    });
    child.on('exit', (code) => {
      clearTimeout(timer);
      server = null;
      if (!settled) {
        fail(`The GitCraft server exited (code ${code}).\n\n${stderr.trim()}`);
      } else if (!quitting) {
        dialog.showErrorBox('GitCraft', `The GitCraft server stopped unexpectedly (code ${code}).\n\n${stderr.trim()}`);
        app.quit();
      }
    });
  });
}

function stopServer() {
  if (!server) return;
  // Closing stdin makes the server exit by itself (--exit-on-stdin-close).
  server.stdin.end();
  const child = server;
  setTimeout(() => child.exitCode === null && child.kill(), 2000).unref();
}

// ------------------------------------------------------------------- window

const stateFile = () => path.join(app.getPath('userData'), 'window-state.json');

function loadWindowState() {
  try {
    return JSON.parse(fs.readFileSync(stateFile(), 'utf8'));
  } catch {
    return { width: 1400, height: 900 };
  }
}

function saveWindowState(win) {
  try {
    const bounds = win.getNormalBounds();
    fs.writeFileSync(stateFile(), JSON.stringify({ ...bounds, maximized: win.isMaximized() }));
  } catch {
    // Not critical.
  }
}

function createWindow(url) {
  const state = loadWindowState();
  const win = new BrowserWindow({
    x: state.x,
    y: state.y,
    width: state.width,
    height: state.height,
    minWidth: 900,
    minHeight: 560,
    title: 'GitCraft',
    show: false,
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#1b1e24' : '#ffffff',
    icon: path.join(__dirname, 'build', 'icon.png'),
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      spellcheck: false,
    },
  });
  if (state.maximized) win.maximize();
  win.once('ready-to-show', () => win.show());
  win.on('close', () => saveWindowState(win));
  win.on('closed', () => {
    if (mainWindow === win) mainWindow = null;
  });

  // Only the GitCraft server may be shown inside the window; everything else
  // opens in the user's browser.
  win.webContents.setWindowOpenHandler(({ url: target }) => {
    if (/^https?:/.test(target)) void shell.openExternal(target);
    return { action: 'deny' };
  });
  win.webContents.on('will-navigate', (event, target) => {
    if (new URL(target).origin !== serverOrigin) {
      event.preventDefault();
      if (/^https?:/.test(target)) void shell.openExternal(target);
    }
  });

  void win.loadURL(url);
  mainWindow = win;
  return win;
}

function appUrl(repo) {
  const url = new URL(serverOrigin + '/');
  url.searchParams.set('token', serverToken);
  if (repo) url.searchParams.set('open', repo);
  return url.toString();
}

function sendToWindow(channel, ...args) {
  if (!mainWindow) return;
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.focus();
  mainWindow.webContents.send(channel, ...args);
}

// --------------------------------------------------------------------- menu

function buildMenu() {
  const openRepository = async () => {
    const result = await dialog.showOpenDialog(mainWindow, {
      title: 'Open repository',
      properties: ['openDirectory'],
    });
    if (!result.canceled && result.filePaths[0]) sendToWindow('gitcraft:open-repo', result.filePaths[0]);
  };

  /** @type {Electron.MenuItemConstructorOptions[]} */
  const template = [
    ...(isMac ? [{ role: 'appMenu' }] : []),
    {
      label: 'File',
      submenu: [
        { label: 'Open Repository…', accelerator: 'CmdOrCtrl+O', click: openRepository },
        { label: 'Clone / Recent…', click: () => sendToWindow('gitcraft:command', 'open-dialog') },
        { type: 'separator' },
        { label: 'Settings…', accelerator: 'CmdOrCtrl+,', click: () => sendToWindow('gitcraft:command', 'settings') },
        { type: 'separator' },
        isMac ? { role: 'close' } : { role: 'quit' },
      ],
    },
    { role: 'editMenu' },
    {
      label: 'View',
      // No reload accelerators: Ctrl/Cmd+R and F5 refresh the repository inside the app.
      submenu: [
        { label: 'Reload Window', click: () => mainWindow?.webContents.reload() },
        { role: 'toggleDevTools' },
        { type: 'separator' },
        { role: 'resetZoom' },
        { role: 'zoomIn' },
        { role: 'zoomOut' },
        { type: 'separator' },
        { role: 'togglefullscreen' },
      ],
    },
    { role: 'windowMenu' },
    {
      role: 'help',
      submenu: [
        {
          label: 'Show Server Data Folder',
          click: () => void shell.openPath(app.getPath('userData')),
        },
      ],
    },
  ];
  Menu.setApplicationMenu(Menu.buildFromTemplate(template));
}

// ---------------------------------------------------------------------- IPC

ipcMain.handle('gitcraft:pick-folder', async (event, options = {}) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  const result = await dialog.showOpenDialog(win, {
    title: typeof options.title === 'string' ? options.title : 'Choose folder',
    defaultPath: typeof options.defaultPath === 'string' ? options.defaultPath : undefined,
    properties: ['openDirectory', 'createDirectory'],
  });
  return result.canceled ? null : result.filePaths[0] ?? null;
});

ipcMain.handle('gitcraft:reveal', async (_event, target) => {
  if (typeof target === 'string' && fs.existsSync(target)) shell.showItemInFolder(target);
});

ipcMain.handle('gitcraft:open-path', async (_event, target) => {
  if (typeof target !== 'string' || !fs.existsSync(target)) return;
  const error = await shell.openPath(target);
  if (error) throw new Error(error);
});

// ---------------------------------------------------------------- lifecycle

// Separate profile (and single-instance lock) for development builds and tests,
// so they never hand over to an installed GitCraft that is already running.
if (process.env.GITCRAFT_USER_DATA) {
  app.setPath('userData', path.resolve(process.env.GITCRAFT_USER_DATA));
} else if (!app.isPackaged) {
  app.setPath('userData', path.join(app.getPath('appData'), 'GitCraft-dev'));
}

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', (_event, argv) => {
    const repo = repoFromArgs(argv);
    if (repo) sendToWindow('gitcraft:open-repo', repo);
    else sendToWindow('gitcraft:command', 'focus');
  });

  app.whenReady().then(async () => {
    fixPath();
    buildMenu();
    const repo = repoFromArgs(process.argv);
    try {
      const url = await startServer(repo);
      serverOrigin = url.origin;
      createWindow(appUrl(repo));
    } catch (error) {
      dialog.showErrorBox('GitCraft could not start', String(error.message || error));
      quitting = true;
      stopServer();
      app.quit();
    }
  });

  app.on('activate', () => {
    if (!mainWindow && serverOrigin) createWindow(appUrl(null));
  });

  app.on('window-all-closed', () => {
    if (!isMac) app.quit();
  });

  app.on('before-quit', () => {
    quitting = true;
    stopServer();
  });
}
