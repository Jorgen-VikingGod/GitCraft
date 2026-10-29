import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { GitActions } from '../core/actions';
import { errorMessage } from '../core/api';
import { FileChange, FileDiff } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore, readStorageValue } from '../core/settings';
import { MenuItem, Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { DiffView, HunkRequest } from '../diff/diff-view';
import { FileList, FilePick } from '../shared/file-list';
import { Icon } from '../shared/icon';
import { Splitter } from '../shared/splitter';
import { CommitBox } from './commit-box';

type Area = 'unstaged' | 'staged' | 'conflicts';

interface Focus {
  area: Area;
  path: string;
}

@Component({
  selector: 'app-changes-view',
  imports: [FileList, DiffView, Splitter, CommitBox, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './changes-view.html',
  styleUrl: './changes-view.scss',
})
export class ChangesView {
  private readonly ws = inject(Workspace);
  private readonly ui = inject(Ui);
  private readonly actions = inject(GitActions);
  private readonly settings = inject(SettingsStore);

  readonly repo = input.required<RepoState>();

  protected readonly leftWidth = signal(readStorageValue('gitcraft-changes-width', 380));
  protected readonly focus = signal<Focus | null>(null);
  protected readonly selection = signal<{ area: Area; paths: string[] }>({ area: 'unstaged', paths: [] });
  protected readonly diff = signal<FileDiff | null>(null);
  protected readonly diffLoading = signal(false);
  private anchor: string | null = null;

  protected readonly status = computed(() => this.repo().status());
  protected readonly unstaged = computed(() => this.status()?.unstaged ?? []);
  protected readonly staged = computed(() => this.status()?.staged ?? []);
  protected readonly conflicts = computed<FileChange[]>(() =>
    (this.status()?.conflicts ?? []).map((c) => ({ path: c.path, status: 'U', origPath: null })),
  );
  protected readonly focusedFile = computed(
    () => {
      const f = this.focus();
      return f ? (this.list(f.area).find((x) => x.path === f.path) ?? null) : null;
    },
    { equal: (a, b) => a?.path === b?.path && a?.status === b?.status && a?.origPath === b?.origPath },
  );
  protected readonly stageMode = computed(() => {
    const f = this.focus();
    return f?.area === 'unstaged' ? 'stage' : f?.area === 'staged' ? 'unstage' : null;
  });
  protected readonly partialNote = computed(() => {
    const file = this.focusedFile();
    if (file?.status === '?') return 'Untracked file: stage the whole file to include it.';
    if (this.settings.value().ignoreWhitespace) return 'Line staging is disabled while “Ignore whitespace” is on.';
    return 'Click lines to select them (Shift+click for a range), then stage or discard just those lines.';
  });
  protected readonly allowPartial = computed(
    () => this.focusedFile()?.status !== '?' && !this.settings.value().ignoreWhitespace,
  );

  constructor() {
    // Keep focus valid when status changes (e.g. after staging the focused file).
    effect(() => {
      const status = this.status();
      const focus = this.focus();
      if (!status) return;
      if (focus && this.list(focus.area).some((f) => f.path === focus.path)) return;
      const next =
        (focus && this.otherArea(focus)) ??
        (this.conflicts()[0] && { area: 'conflicts' as Area, path: this.conflicts()[0].path }) ??
        (this.unstaged()[0] && { area: 'unstaged' as Area, path: this.unstaged()[0].path }) ??
        (this.staged()[0] && { area: 'staged' as Area, path: this.staged()[0].path }) ??
        null;
      this.focus.set(next);
      this.selection.set({ area: next?.area ?? 'unstaged', paths: next ? [next.path] : [] });
    });
    // Reload the diff when the focused file, its status or settings change.
    effect(() => {
      const file = this.focusedFile();
      const focus = this.focus();
      const repo = this.repo();
      repo.worktreeVersion();
      const { diffContext, ignoreWhitespace } = this.settings.value();
      if (!file || !focus || focus.area === 'conflicts') {
        this.diff.set(null);
        return;
      }
      untracked(() => void this.loadDiff(repo, file, focus.area === 'staged', diffContext, ignoreWhitespace));
    });
  }

  private list(area: Area): FileChange[] {
    return area === 'unstaged' ? this.unstaged() : area === 'staged' ? this.staged() : this.conflicts();
  }

  /** When a file moves between areas, follow it; otherwise pick a neighbour. */
  private otherArea(focus: Focus): Focus | null {
    for (const area of ['unstaged', 'staged', 'conflicts'] as Area[]) {
      if (area !== focus.area && this.list(area).some((f) => f.path === focus.path)) {
        const list = this.list(focus.area);
        if (list.length) return { area: focus.area, path: list[0].path };
        return { area, path: focus.path };
      }
    }
    const list = this.list(focus.area);
    return list.length ? { area: focus.area, path: list[0].path } : null;
  }

  private async loadDiff(repo: RepoState, file: FileChange, staged: boolean, context: number, ignoreWs: boolean) {
    this.diffLoading.set(this.diff()?.newPath !== file.path);
    try {
      const diff = await repo.get<FileDiff | null>('/diff/working', {
        path: file.path,
        staged,
        context,
        ignore_ws: ignoreWs,
        orig_path: staged ? file.origPath : null,
      });
      // Keep the same object (and the line selection) when nothing changed.
      if (this.focusedFile()?.path === file.path && JSON.stringify(diff) !== JSON.stringify(this.diff())) {
        this.diff.set(diff);
      }
    } catch (e) {
      this.diff.set(null);
      this.ui.toast('Could not load diff', 'error', errorMessage(e));
    } finally {
      this.diffLoading.set(false);
    }
  }

  protected isSelected(area: Area): string[] {
    const sel = this.selection();
    return sel.area === area ? sel.paths : [];
  }

  protected pick(area: Area, { file, event }: FilePick): void {
    const current = this.selection();
    const list = this.list(area).map((f) => f.path);
    let paths: string[];
    if ((event.ctrlKey || event.metaKey) && current.area === area) {
      paths = current.paths.includes(file.path)
        ? current.paths.filter((p) => p !== file.path)
        : [...current.paths, file.path];
    } else if (event.shiftKey && current.area === area && this.anchor && list.includes(this.anchor)) {
      const [a, b] = [list.indexOf(this.anchor), list.indexOf(file.path)].sort((x, y) => x - y);
      paths = list.slice(a, b + 1);
    } else {
      paths = [file.path];
      this.anchor = file.path;
    }
    this.selection.set({ area, paths });
    this.focus.set({ area, path: file.path });
  }

  private targets(area: Area, file: FileChange): string[] {
    const sel = this.selection();
    return sel.area === area && sel.paths.includes(file.path) ? sel.paths : [file.path];
  }

  protected stage(paths: string[]): Promise<unknown> {
    return this.ws.run('Staging', () => this.repo().post('/stage', { paths }), this.repo(), { refresh: false })
      .then(() => this.repo().refreshStatus());
  }

  protected unstage(paths: string[]): Promise<unknown> {
    return this.ws.run('Unstaging', () => this.repo().post('/unstage', { paths }), this.repo(), { refresh: false })
      .then(() => this.repo().refreshStatus());
  }

  protected async discard(paths: string[]): Promise<void> {
    const what = paths.length === 1 ? paths[0] : `${paths.length} files`;
    if (!(await this.ui.confirm('Discard changes', `Discard all changes to ${what}? This cannot be undone.`, 'Discard', true))) return;
    await this.ws.run('Discarding', () => this.repo().post('/discard', { paths }), this.repo(), { refresh: false });
    await this.repo().refreshStatus();
  }

  protected stageAll(): Promise<unknown> {
    return this.ws.run('Staging all', () => this.repo().post('/stage/all'), this.repo(), { refresh: false })
      .then(() => this.repo().refreshStatus());
  }

  protected unstageAll(): Promise<unknown> {
    return this.ws.run('Unstaging all', () => this.repo().post('/unstage/all'), this.repo(), { refresh: false })
      .then(() => this.repo().refreshStatus());
  }

  protected async discardAll(): Promise<void> {
    await this.discard(this.unstaged().map((f) => f.path));
  }

  protected async hunk(request: HunkRequest): Promise<void> {
    const file = this.focusedFile();
    if (!file) return;
    if (request.mode === 'discard') {
      const what = request.lines ? `${request.lines.length} selected line(s)` : 'this hunk';
      if (!(await this.ui.confirm('Discard changes', `Discard ${what}? This cannot be undone.`, 'Discard', true))) return;
    }
    const label = { stage: 'Staging', unstage: 'Unstaging', discard: 'Discarding' }[request.mode];
    await this.ws.run(
      label,
      () =>
        this.repo().post('/selection', {
          path: file.path,
          mode: request.mode,
          selection: { [request.hunk]: request.lines },
          context: this.settings.value().diffContext,
        }),
      this.repo(),
      { refresh: false },
    );
    this.repo().worktreeVersion.update((v) => v + 1);
    await this.repo().refreshStatus();
  }

  protected menu(area: Area, { file, event }: FilePick): void {
    if (!this.isSelected(area).includes(file.path)) this.pick(area, { file, event: new MouseEvent('click') });
    const paths = this.targets(area, file);
    const n = paths.length > 1 ? ` ${paths.length} files` : '';
    const repo = this.repo();
    let items: MenuItem[];
    if (area === 'unstaged') {
      const untracked = paths.every((p) => this.unstaged().find((f) => f.path === p)?.status === '?');
      items = [
        { label: `Stage${n}`, action: () => void this.stage(paths) },
        { label: `Discard changes${n}…`, action: () => void this.discard(paths), danger: true },
        { label: `Stash${n}…`, action: () => void this.actions.stashFiles(repo, paths) },
        ...(paths.length === 1 ? [{ label: 'Ignore', children: this.actions.ignoreItems(repo, file.path) }] : []),
        ...(untracked && paths.length > 1
          ? [{ label: `Ignore${n}`, action: () => paths.forEach((p) => void this.actions.ignore(repo, '/' + p)) }]
          : []),
      ];
    } else if (area === 'staged') {
      items = [
        { label: `Unstage${n}`, action: () => void this.unstage(paths) },
        { label: `Stash${n}…`, action: () => void this.actions.stashFiles(repo, paths) },
      ];
    } else {
      items = [
        { label: 'Resolve in merge tool…', action: () => this.ui.open({ kind: 'conflict', repo, path: file.path }) },
        { label: 'Resolve using ours', action: () => void this.resolve(file.path, 'ours') },
        { label: 'Resolve using theirs', action: () => void this.resolve(file.path, 'theirs') },
        { label: 'Mark as resolved', action: () => void this.resolve(file.path, null) },
      ];
    }
    this.ui.openMenu(event, [...items, { separator: true }, ...this.actions.fileMenuItems(repo, file.path)]);
  }

  protected openConflict(path: string): void {
    this.ui.open({ kind: 'conflict', repo: this.repo(), path });
  }

  protected async resolve(path: string, side: 'ours' | 'theirs' | null): Promise<void> {
    await this.ws.run('Resolving', () => this.repo().post('/conflict/resolve', { path, side }), this.repo(), {
      refresh: false,
    });
    await this.repo().refreshStatus();
  }

  protected activate(area: Area, file: FileChange): void {
    if (area === 'unstaged') void this.stage([file.path]);
    else if (area === 'staged') void this.unstage([file.path]);
    else this.openConflict(file.path);
  }
}
