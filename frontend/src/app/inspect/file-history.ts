import { ChangeDetectionStrategy, Component, OnInit, effect, inject, input, signal, untracked } from '@angular/core';
import { shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { FileDiff, LogCommit } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore } from '../core/settings';
import { Ui } from '../core/ui';
import { DiffView } from '../diff/diff-view';
import { fullDate, shortDate } from '../shared/format';
import { Icon } from '../shared/icon';

@Component({
  selector: 'app-file-history',
  imports: [DiffView, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'ui.closeOverlay()' },
  template: `
    <div class="dialog">
      <header>
        <app-icon name="history" [size]="18" />
        <div class="title">
          <h2>File history</h2>
          <div class="path">{{ path() }}</div>
        </div>
        <span class="spacer"></span>
        <button class="icon-btn" (click)="ui.closeOverlay()" title="Close"><app-icon name="x" [size]="18" /></button>
      </header>
      <div class="body">
        <div class="list" tabindex="0" (keydown)="key($event)">
          @if (loading()) {
            <div class="empty">Loading…</div>
          } @else if (error()) {
            <div class="empty error">{{ error() }}</div>
          }
          @for (c of commits(); track c.sha) {
            <div class="item" [class.selected]="selected()?.sha === c.sha" (click)="selected.set(c)">
              <div class="subject">{{ c.subject }}</div>
              <div class="meta">
                <span class="status" [attr.data-status]="c.file?.status">{{ c.file?.status }}</span>
                {{ c.author }} · <span [title]="long(c.time)">{{ date(c.time) }}</span> · <code>{{ short(c.sha) }}</code>
              </div>
              @if (c.file?.origPath; as orig) {
                <div class="rename">renamed from {{ orig }}</div>
              }
            </div>
          }
        </div>
        <div class="right">
          @if (selected(); as c) {
            <div class="actions">
              <button class="btn small" (click)="showInHistory(c)">Show in history</button>
              <button class="btn small" (click)="blame(c)">Blame at this commit</button>
            </div>
          }
          <app-diff-view class="diff" [diff]="diff()" [loading]="diffLoading()" emptyText="Select a commit" />
        </div>
      </div>
    </div>
  `,
  styleUrl: './inspect.scss',
})
export class FileHistory implements OnInit {
  protected readonly ui = inject(Ui);
  private readonly settings = inject(SettingsStore);
  readonly repo = input.required<RepoState>();
  readonly path = input.required<string>();

  protected readonly commits = signal<LogCommit[]>([]);
  protected readonly selected = signal<LogCommit | null>(null);
  protected readonly diff = signal<FileDiff | null>(null);
  protected readonly loading = signal(true);
  protected readonly diffLoading = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly date = shortDate;
  protected readonly long = fullDate;
  protected readonly short = shortSha;

  constructor() {
    effect(() => {
      const commit = this.selected();
      const { diffContext, ignoreWhitespace } = this.settings.value();
      if (commit) untracked(() => void this.loadDiff(commit, diffContext, ignoreWhitespace));
    });
  }

  async ngOnInit(): Promise<void> {
    try {
      const commits = await this.repo().get<LogCommit[]>('/file-history', { path: this.path() });
      this.commits.set(commits);
      this.selected.set(commits[0] ?? null);
    } catch (e) {
      this.error.set(errorMessage(e));
    } finally {
      this.loading.set(false);
    }
  }

  private async loadDiff(commit: LogCommit, context: number, ignoreWs: boolean): Promise<void> {
    this.diffLoading.set(true);
    try {
      const file = commit.file;
      const diff = await this.repo().get<FileDiff | null>('/diff/commit', {
        sha: commit.sha,
        path: file?.path ?? this.path(),
        orig_path: file?.origPath,
        context,
        ignore_ws: ignoreWs,
      });
      if (this.selected() === commit) this.diff.set(diff);
    } catch (e) {
      this.diff.set(null);
      this.ui.toast('Could not load diff', 'error', errorMessage(e));
    } finally {
      this.diffLoading.set(false);
    }
  }

  protected key(event: KeyboardEvent): void {
    const list = this.commits();
    const index = list.findIndex((c) => c.sha === this.selected()?.sha);
    const delta = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (!delta) return;
    event.preventDefault();
    const next = list[Math.max(0, Math.min(list.length - 1, index + delta))];
    if (next) this.selected.set(next);
  }

  protected showInHistory(commit: LogCommit): void {
    const repo = this.repo();
    repo.search.set(null);
    repo.view.set('history');
    repo.selectedSha.set(commit.sha);
    this.ui.closeOverlay();
  }

  protected blame(commit: LogCommit): void {
    if (commit.file?.status === 'D') {
      this.ui.toast('The file was deleted in this commit', 'info');
      return;
    }
    this.ui.open({ kind: 'blame', repo: this.repo(), path: commit.file?.path ?? this.path(), rev: commit.sha });
  }
}
