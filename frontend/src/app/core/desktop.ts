/** Bridge exposed by the Electron shell (desktop/preload.js); null in a normal browser. */
export interface DesktopBridge {
  platform: string;
  pickFolder(options?: { title?: string; defaultPath?: string }): Promise<string | null>;
  reveal(path: string): Promise<void>;
  /** Open a file with its default application. */
  openPath(path: string): Promise<void>;
  onOpenRepo(callback: (path: string) => void): () => void;
  onCommand(callback: (command: 'open-dialog' | 'settings' | 'focus') => void): () => void;
}

export const desktop: DesktopBridge | null =
  (globalThis as { gitcraftDesktop?: DesktopBridge }).gitcraftDesktop ?? null;
