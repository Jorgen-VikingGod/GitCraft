import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { GitActions, shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { CommitDetail, FileChange, FileDiff } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore, readStorageValue } from '../core/settings';
import { Ui, copyText } from '../core/ui';
import { DiffView } from '../diff/diff-view';
import { FileList, FilePick } from '../shared/file-list';
import { fullDate, initials, nameHue } from '../shared/format';
import { Splitter } from '../shared/splitter';
import { RefBadges } from './ref-badges';

@Component({
  selector: 'app-commit-details',
  imports: [FileList, DiffView, Splitter, RefBadges],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (error()) {
      <div class="placeholder error">{{ error() }}</div>
    } @else if (detail(); as d) {
      <div class="left" [style.width.px]="leftWidth()">
        <div class="meta">
          <div class="author">
            <span class="avatar" [style.background]="'hsl(' + hue(d.author) + ' 55% 45%)'">{{ initials(d.author) }}</span>
            <div>
              <div class="name">{{ d.author }} <span class="email">&lt;{{ d.email }}&gt;</span></div>
              <div class="date">{{ date(d.time) }}</div>
            </div>
          </div>
          @if (d.refs.length) {
            <app-ref-badges [refs]="d.refs" />
          }
          <pre class="message">{{ d.message }}</pre>
          <table class="facts">
            <tr><th>SHA</th><td><a (click)="copy(d.sha)" title="Copy">{{ d.sha }}</a></td></tr>
            <tr>
              <th>{{ d.parents.length > 1 ? 'Parents' : 'Parent' }}</th>
              <td>
                @for (p of d.parents; track p) {
                  <a (click)="repo().selectedSha.set(p)" title="Go to parent">{{ short(p) }}</a>&nbsp;
                } @empty { (root commit) }
              </td>
            </tr>
            @if (d.committer !== d.author || d.commitTime !== d.time) {
              <tr><th>Committer</th><td>{{ d.committer }}, {{ date(d.commitTime) }}</td></tr>
            }
          </table>
        </div>
        <div class="files-title">{{ d.files.length }} changed file{{ d.files.length === 1 ? '' : 's' }}
          @if (d.parents.length > 1) { <span class="muted">(compared to first parent)</span> }
        </div>
        <app-file-list class="files" [files]="d.files" [selected]="selectedFile() ? [selectedFile()!.path] : []"
          (pick)="select($event.file)" (menu)="fileMenu($event)" />
      </div>
      <app-splitter axis="x" [(size)]="leftWidth" [min]="220" [max]="900" [defaultSize]="360"
        storageKey="gitcraft-details-width" />
      <app-diff-view class="diff" [diff]="diff()" [loading]="diffLoading()"
        [emptyText]="d.files.length ? 'Select a file to see its changes' : 'This commit has no file changes'" />
    } @else {
      <div class="placeholder">Loading…</div>
    }
  `,
  styleUrl: './commit-details.scss',
})
export class CommitDetails {
  private readonly ui = inject(Ui);
  private readonly actions = inject(GitActions);
  private readonly settings = inject(SettingsStore);

  readonly repo = input.required<RepoState>();
  readonly sha = input.required<string>();

  protected readonly detail = signal<CommitDetail | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly selectedFile = signal<FileChange | null>(null);
  protected readonly diff = signal<FileDiff | null>(null);
  protected readonly diffLoading = signal(false);
  protected readonly leftWidth = signal(readStorageValue('gitcraft-details-width', 360));
  private readonly diffOptions = computed(() => ({
    context: this.settings.value().diffContext,
    ignore_ws: this.settings.value().ignoreWhitespace,
  }));

  protected readonly initials = initials;
  protected readonly hue = nameHue;
  protected readonly date = fullDate;
  protected readonly short = shortSha;

  constructor() {
    effect(() => {
      const repo = this.repo();
      const sha = this.sha();
      untracked(() => void this.load(repo, sha));
    });
    effect(() => {
      const file = this.selectedFile();
      const detail = this.detail();
      const options = this.diffOptions();
      if (file && detail) untracked(() => void this.loadDiff(detail.sha, file, options));
      else this.diff.set(null);
    });
  }

  private async load(repo: RepoState, sha: string): Promise<void> {
    this.error.set(null);
    try {
      const detail = await repo.commitDetail(sha);
      if (this.sha() !== sha) return;
      const previous = this.selectedFile()?.path;
      this.detail.set(detail);
      this.selectedFile.set(detail.files.find((f) => f.path === previous) ?? detail.files[0] ?? null);
    } catch (e) {
      this.detail.set(null);
      this.error.set(errorMessage(e));
    }
  }

  private async loadDiff(sha: string, file: FileChange, options: { context: number; ignore_ws: boolean }): Promise<void> {
    this.diffLoading.set(true);
    try {
      const diff = await this.repo().get<FileDiff | null>('/diff/commit', {
        sha,
        path: file.path,
        orig_path: file.origPath,
        ...options,
      });
      if (this.selectedFile() === file) this.diff.set(diff);
    } catch (e) {
      this.diff.set(null);
      this.ui.toast('Could not load diff', 'error', errorMessage(e));
    } finally {
      this.diffLoading.set(false);
    }
  }

  protected select(file: FileChange): void {
    this.selectedFile.set(file);
  }

  protected fileMenu({ file, event }: FilePick): void {
    this.ui.openMenu(event, this.actions.fileMenuItems(this.repo(), file.path, this.sha()));
  }

  protected copy(text: string): void {
    void copyText(text, this.ui);
  }
}
