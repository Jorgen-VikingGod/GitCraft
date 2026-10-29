import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { GitActions, shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { FileChange, FileDiff } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore } from '../core/settings';
import { Ui } from '../core/ui';
import { DiffView } from '../diff/diff-view';
import { FileList, FilePick } from '../shared/file-list';
import { Icon } from '../shared/icon';
import { Splitter } from '../shared/splitter';

interface CompareResult {
  files: FileChange[];
  baseOnly: number;
  targetOnly: number;
}

/** Changes between two commits (base -> target). */
@Component({
  selector: 'app-compare-view',
  imports: [FileList, DiffView, Splitter, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="left" [style.width.px]="leftWidth()">
      <div class="meta">
        <div class="title"><app-icon name="merge" [size]="14" /> Comparing commits</div>
        <div class="range">
          <code>{{ short(base()) }}</code> {{ subject(base()) }}<br />
          → <code>{{ short(target()) }}</code> {{ subject(target()) }}
        </div>
        @if (result(); as r) {
          <div class="counts">
            {{ r.targetOnly }} commit{{ r.targetOnly === 1 ? '' : 's' }} only in target,
            {{ r.baseOnly }} only in base
          </div>
        }
        <div class="buttons">
          <button class="btn small" (click)="swap()">Swap</button>
          <button class="btn small" (click)="repo().compare.set(null)">Close</button>
        </div>
      </div>
      @if (error()) {
        <div class="error">{{ error() }}</div>
      }
      <app-file-list class="files" [files]="result()?.files ?? []" [selected]="selected() ? [selected()!.path] : []"
        (pick)="selected.set($event.file)" (menu)="fileMenu($event)" emptyText="No differences" />
    </div>
    <app-splitter axis="x" [(size)]="leftWidth" [min]="220" [max]="900" [defaultSize]="360" />
    <app-diff-view class="diff" [diff]="diff()" [loading]="diffLoading()" />
  `,
  styles: `
    :host { display: flex; height: 100%; min-height: 0; }
    .left { display: flex; flex-direction: column; flex: none; background: var(--panel); min-width: 0; }
    .meta { padding: 12px 14px; border-bottom: 1px solid var(--border); }
    .title { display: flex; align-items: center; gap: 6px; font-weight: 600; margin-bottom: 6px; }
    .range { font-size: 12px; line-height: 1.7; overflow: hidden; }
    .counts { font-size: 12px; color: var(--text-muted); margin-top: 4px; }
    code { font-family: var(--mono); color: var(--accent); }
    .buttons { display: flex; gap: 6px; margin-top: 10px; }
    .files { flex: 1; overflow: auto; padding: 6px 0; }
    .diff { flex: 1; min-width: 0; }
    .error { padding: 10px 14px; color: var(--danger); }
  `,
})
export class CompareView {
  private readonly ui = inject(Ui);
  private readonly actions = inject(GitActions);
  private readonly settings = inject(SettingsStore);
  readonly repo = input.required<RepoState>();
  readonly base = input.required<string>();
  readonly target = input.required<string>();

  protected readonly leftWidth = signal(360);
  protected readonly result = signal<CompareResult | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly selected = signal<FileChange | null>(null);
  protected readonly diff = signal<FileDiff | null>(null);
  protected readonly diffLoading = signal(false);
  protected readonly short = shortSha;
  private readonly subjects = computed(() => new Map(this.repo().commits().map((c) => [c.sha, c.subject])));

  constructor() {
    effect(() => {
      const [repo, base, target] = [this.repo(), this.base(), this.target()];
      untracked(() => void this.load(repo, base, target));
    });
    effect(() => {
      const file = this.selected();
      const context = this.settings.value().diffContext;
      const [base, target] = [this.base(), this.target()];
      if (file) untracked(() => void this.loadDiff(base, target, file.path, context));
      else this.diff.set(null);
    });
  }

  protected subject(sha: string): string {
    return this.subjects().get(sha) ?? '';
  }

  private async load(repo: RepoState, base: string, target: string): Promise<void> {
    this.error.set(null);
    try {
      const result = await repo.get<CompareResult>('/compare', { base, target });
      this.result.set(result);
      this.selected.set(result.files[0] ?? null);
    } catch (e) {
      this.result.set(null);
      this.error.set(errorMessage(e));
    }
  }

  private async loadDiff(base: string, target: string, path: string, context: number): Promise<void> {
    this.diffLoading.set(true);
    try {
      this.diff.set(await this.repo().get<FileDiff | null>('/diff/compare', { base, target, path, context }));
    } catch (e) {
      this.diff.set(null);
      this.ui.toast('Could not load diff', 'error', errorMessage(e));
    } finally {
      this.diffLoading.set(false);
    }
  }

  protected swap(): void {
    this.repo().compare.set({ base: this.target(), target: this.base() });
  }

  protected fileMenu({ file, event }: FilePick): void {
    this.ui.openMenu(event, this.actions.fileMenuItems(this.repo(), file.path, this.target()));
  }
}
