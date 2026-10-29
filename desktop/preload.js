// Exposes a minimal, typed bridge to the GitCraft web UI (window.gitcraftDesktop).
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('gitcraftDesktop', {
  platform: process.platform,
  /** Native folder picker; resolves to the chosen path or null. */
  pickFolder: (options) => ipcRenderer.invoke('gitcraft:pick-folder', options ?? {}),
  /** Show a file or folder in the system file manager. */
  reveal: (target) => ipcRenderer.invoke('gitcraft:reveal', target),
  /** Open a file with its default application. */
  openPath: (target) => ipcRenderer.invoke('gitcraft:open-path', target),
  /** Repositories opened from the menu or a second app instance. */
  onOpenRepo: (callback) => {
    const listener = (_event, repoPath) => callback(repoPath);
    ipcRenderer.on('gitcraft:open-repo', listener);
    return () => ipcRenderer.removeListener('gitcraft:open-repo', listener);
  },
  /** Menu commands: 'open-dialog' | 'settings' | 'focus'. */
  onCommand: (callback) => {
    const listener = (_event, command) => callback(command);
    ipcRenderer.on('gitcraft:command', listener);
    return () => ipcRenderer.removeListener('gitcraft:command', listener);
  },
});
