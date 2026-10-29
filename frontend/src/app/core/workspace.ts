import { Injectable, computed, inject, signal } from '@angular/core';
import { Api, errorMessage, launch } from './api';
import { desktop } from './desktop';
import { Events } from './events';
import { OpResult, RecentRepo } from './models';
import { RepoState } from './repo-state';
import { SettingsStore, readStorageValue, writeStorage } from './settings';
import { Ui } from './ui';

const TABS_KEY = 'gitcraft-tabs';

export interface RunOptions {
  /** Toast shown when the operation succeeds. */
  success?: string;
  /** Reload status/refs/log afterwards (default true). */
  refresh?: boolean;
}

/** Open repository tabs and the shared operation runner. */
@Injectable({ providedIn: 'root' })
export class Workspace {
  private readonly api = inject(Api);
  private readonly events = inject(Events);
  private readonly ui = inject(Ui);
  private readonly settings = inject(SettingsStore);

  readonly tabs = signal<RepoState[]>([]);
  readonly activeIndex = signal(-1);
  readonly active = computed<RepoState | null>(() => this.tabs()[this.activeIndex()] ?? null);
  readonly recent = signal<RecentRepo[]>([]);
  readonly busy = signal<string | null>(null);
  readonly progressLine = signal<string>('');
  private currentOp: string | null = null;

  constructor() {
    this.events.changes.subscribe((event) => {
      this.tabs().find((t) => t.path === event.repo)?.onChange(event);
    });
    this.events.progress.subscribe((event) => {
      if (event.opId === this.currentOp) this.progressLine.set(event.line);
    });
  }

  async start(): Promise<void> {
    this.events.connect();
    desktop?.onOpenRepo((path) => void this.open(path));
    desktop?.onCommand((command) => {
      if (command === 'open-dialog') this.ui.open({ kind: 'open' });
      else if (command === 'settings') this.ui.open({ kind: 'settings' });
    });
    await this.loadRecent();
    const saved = readStorageValue<{ paths: string[]; active: number }>(TABS_KEY, { paths: [], active: 0 });
    for (const path of saved.paths) {
      await this.open(path, { quiet: true, activate: false });
    }
    if (this.tabs().length) this.activeIndex.set(Math.min(saved.active, this.tabs().length - 1));
    if (launch.open) await this.open(launch.open);
    if (!this.tabs().length) this.ui.open({ kind: 'open' });
  }

  async loadRecent(): Promise<void> {
    try {
      this.recent.set(await this.api.get<RecentRepo[]>('/repos'));
    } catch {
      this.recent.set([]);
    }
  }

  async open(path: string, options: { quiet?: boolean; activate?: boolean } = {}): Promise<RepoState | null> {
    let entry: RecentRepo;
    try {
      entry = await this.api.post<RecentRepo>('/repos/open', { path });
    } catch (e) {
      if (!options.quiet) this.ui.toast(errorMessage(e), 'error');
      return null;
    }
    return this.addTab(entry.path, options.activate ?? true);
  }

  addTab(path: string, activate = true): RepoState {
    let index = this.tabs().findIndex((t) => t.path === path);
    if (index < 0) {
      const state = new RepoState(this.api, this.settings, path);
      this.tabs.update((tabs) => [...tabs, state]);
      index = this.tabs().length - 1;
      void state.refreshAll();
      if (this.settings.value().fetchOnOpen) {
        void this.run('Fetching', (opId) => state.post('/fetch', { opId }), state, { refresh: true });
      }
    }
    if (activate || this.activeIndex() < 0) this.activeIndex.set(index);
    this.persist();
    void this.loadRecent();
    return this.tabs()[index];
  }

  close(index: number): void {
    this.tabs.update((tabs) => tabs.filter((_, i) => i !== index));
    const active = this.activeIndex();
    if (active >= this.tabs().length) this.activeIndex.set(this.tabs().length - 1);
    else if (index < active) this.activeIndex.set(active - 1);
    this.persist();
    if (!this.tabs().length) this.ui.open({ kind: 'open' });
  }

  activate(index: number): void {
    this.activeIndex.set(index);
    this.persist();
  }

  move(from: number, to: number): void {
    const active = this.active();
    this.tabs.update((tabs) => {
      const copy = [...tabs];
      const [item] = copy.splice(from, 1);
      copy.splice(to, 0, item);
      return copy;
    });
    if (active) this.activeIndex.set(this.tabs().indexOf(active));
    this.persist();
  }

  private persist(): void {
    const paths = this.tabs().map((t) => t.path);
    writeStorage(TABS_KEY, { paths, active: this.activeIndex() });
    this.events.watch(paths);
  }

  /**
   * Runs a git operation with busy indicator, progress, error toasts and a
   * refresh afterwards. Operations returning an OpResult that stopped with
   * conflicts switch the repository to Local Changes.
   */
  async run<T>(
    label: string,
    fn: (opId: string) => Promise<T>,
    repo: RepoState | null = this.active(),
    options: RunOptions = {},
  ): Promise<T | undefined> {
    const opId = Math.random().toString(36).slice(2);
    this.currentOp = opId;
    this.busy.set(label);
    this.progressLine.set('');
    try {
      const result = await fn(opId);
      const op = result as unknown as OpResult | undefined;
      if (op && typeof op === 'object' && 'ok' in op && op.ok === false) {
        if (op.conflicts) {
          this.ui.toast('Stopped because of conflicts. Resolve them in Local Changes.', 'warning', op.message);
        } else {
          this.ui.toast('Operation paused', 'warning', op.message);
        }
        repo?.view.set('changes');
      } else if (options.success) {
        this.ui.toast(options.success, 'success');
      }
      return result;
    } catch (e) {
      this.ui.toast(`${label} failed`, 'error', errorMessage(e));
      return undefined;
    } finally {
      if (this.currentOp === opId) {
        this.currentOp = null;
        this.busy.set(null);
      }
      if (repo && options.refresh !== false) void repo.refreshAll();
    }
  }
}
