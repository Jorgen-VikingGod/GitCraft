import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { GitActions } from '../core/actions';
import { errorMessage } from '../core/api';
import { FileChange, FileDiff } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore } from '../core/settings';
import { Ui } from '../core/ui';
import { DiffView } from '../diff/diff-view';
import { FileList } from '../shared/file-list';
import { fullDate } from '../shared/format';
import { Splitter } from '../shared/splitter';

@Component({
  selector: 'app-stash-view',
  imports: [FileList, DiffView, Splitter],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (stash(); as s) {
      <div class="left" [style.width.px]="leftWidth()">
        <div class="meta">
          <div class="title">{{ s.message }}</div>
          <div class="sub">{{ s.name }} · {{ date(s.time) }}</div>
          <div class="buttons">
            <button class="btn small primary" (click)="actions.applyStash(repo(), s, false)">Apply</button>
            <button class="btn small" (click)="actions.applyStash(repo(), s, true)">Pop</button>
            <button class="btn small ghost danger-text" (click)="actions.dropStash(repo(), s)">Drop…</button>
          </div>
        </div>
        <app-file-list class="files" [files]="files()" [selected]="selected() ? [selected()!.path] : []"
          (pick)="selected.set($event.file)" />
      </div>
      <app-splitter axis="x" [(size)]="leftWidth" [min]="220" [max]="900" [defaultSize]="360" />
      <app-diff-view class="diff" [diff]="diff()" [loading]="loading()" />
    } @else {
      <div class="empty">Stash not found</div>
    }
  `,
  styles: `
    :host { display: flex; height: 100%; min-height: 0; }
    .left { display: flex; flex-direction: column; flex: none; background: var(--panel); min-width: 0; }
    .meta { padding: 12px 14px; border-bottom: 1px solid var(--border); }
    .title { font-weight: 600; word-break: break-word; }
    .sub { color: var(--text-muted); font-size: 12px; margin: 4px 0 10px; }
    .buttons { display: flex; gap: 6px; }
    .files { flex: 1; overflow: auto; padding: 6px 0; }
    .diff { flex: 1; min-width: 0; }
    .empty { margin: auto; color: var(--text-muted); }
  `,
})
export class StashView {
  protected readonly actions = inject(GitActions);
  private readonly ui = inject(Ui);
  private readonly settings = inject(SettingsStore);
  readonly repo = input.required<RepoState>();
  readonly name = input.required<string>();

  protected readonly leftWidth = signal(360);
  protected readonly files = signal<FileChange[]>([]);
  protected readonly selected = signal<FileChange | null>(null);
  protected readonly diff = signal<FileDiff | null>(null);
  protected readonly loading = signal(false);
  protected readonly stash = computed(() => this.repo().refs()?.stashes.find((s) => s.name === this.name()) ?? null);
  private readonly stashName = computed(() => this.stash()?.name ?? null);
  protected readonly date = fullDate;

  constructor() {
    effect(() => {
      const repo = this.repo();
      const name = this.stashName();
      if (name) untracked(() => void this.loadFiles(repo, name));
    });
    effect(() => {
      const file = this.selected();
      const name = this.stashName();
      const context = this.settings.value().diffContext;
      if (file && name) untracked(() => void this.loadDiff(name, file.path, context));
      else this.diff.set(null);
    });
  }

  private async loadFiles(repo: RepoState, name: string): Promise<void> {
    try {
      const files = await repo.get<FileChange[]>('/stash/files', { name });
      this.files.set(files);
      this.selected.set(files[0] ?? null);
    } catch (e) {
      this.ui.toast('Could not load stash', 'error', errorMessage(e));
    }
  }

  private async loadDiff(name: string, path: string, context: number): Promise<void> {
    this.loading.set(true);
    try {
      this.diff.set(await this.repo().get<FileDiff | null>('/stash/diff', { name, path, context }));
    } catch {
      this.diff.set(null);
    } finally {
      this.loading.set(false);
    }
  }
}
