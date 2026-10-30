import { ChangeDetectionStrategy, Component, ElementRef, computed, inject, input, signal, viewChild } from '@angular/core';
import { GitActions } from '../core/actions';
import { errorMessage } from '../core/api';
import { RepoState, SearchMode } from '../core/repo-state';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { Icon } from '../shared/icon';
import { Select } from '../shared/select';

@Component({
  selector: 'app-toolbar',
  imports: [Icon, Select],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let r = repo();
    <div class="group">
      <button class="tool" (click)="actions.fetch(r)" [disabled]="!!ws.busy()" title="Fetch all remotes">
        <app-icon name="fetch" [size]="18" /><span>Fetch</span>
      </button>
      <button class="tool" (click)="actions.pull(r)" [disabled]="!!ws.busy()" title="Pull">
        <app-icon name="pull" [size]="18" /><span>Pull</span>
        @if (behind()) { <em class="pill">{{ behind() }}</em> }
      </button>
      <button class="tool" (click)="actions.push(r)" [disabled]="!!ws.busy()" title="Push">
        <app-icon name="push" [size]="18" /><span>Push</span>
        @if (ahead()) { <em class="pill">{{ ahead() }}</em> }
      </button>
    </div>
    <div class="group">
      <button class="tool" (click)="actions.stash(r)" [disabled]="!!ws.busy()" title="Stash local changes">
        <app-icon name="stash" [size]="18" /><span>Stash</span>
      </button>
      <button class="tool" (click)="actions.newBranch(r)" [disabled]="!!ws.busy()" title="Create a branch">
        <app-icon name="branch" [size]="18" /><span>Branch</span>
      </button>
      <button class="tool" (click)="actions.repositoryMenu($event, r)" title="More repository commands">
        <app-icon name="more" [size]="18" /><span>More</span>
      </button>
    </div>
    <div class="repo-info">
      <div class="repo-name">{{ r.name }}</div>
      <div class="branch-name">
        @if (r.status()?.branch?.detached) {
          detached at {{ r.status()?.branch?.oid?.slice(0, 7) }}
        } @else {
          {{ r.currentBranch() ?? '…' }}
          @if (r.status()?.branch?.upstream; as upstream) { <span class="upstream">→ {{ upstream }}</span> }
        }
      </div>
    </div>
    <div class="search">
      <app-icon name="search" [size]="14" />
      <input #searchInput type="search" placeholder="Search commits (Ctrl+F)" [value]="query()"
        (input)="query.set($any($event.target).value)" (keydown.enter)="search()" (keydown.escape)="clear()" />
      <app-select plain title="Search in" [options]="searchModes" [value]="mode()"
        (valueChange)="mode.set($any($event)); query() && search()" />
    </div>
  `,
  styles: `
    :host { display: flex; align-items: center; gap: 14px; height: 52px; padding: 0 12px; background: var(--toolbar);
      border-bottom: 1px solid var(--border); }
    .group { display: flex; gap: 2px; }
    .tool { position: relative; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px;
      min-width: 52px; height: 44px; padding: 0 6px; border: 0; border-radius: 6px; background: none; color: var(--text);
      font: inherit; font-size: 11px; cursor: pointer; }
    .tool:hover:not(:disabled) { background: var(--hover); }
    .tool:disabled { opacity: .45; cursor: default; }
    .pill { position: absolute; top: 2px; right: 6px; font-style: normal; font-size: 10px; min-width: 16px; height: 16px;
      line-height: 16px; border-radius: 8px; background: var(--accent); color: #fff; padding: 0 4px; }
    .repo-info { flex: 1; min-width: 0; text-align: center; }
    .repo-name { font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .branch-name { font-size: 12px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .upstream { color: var(--text-faint); }
    .search { display: flex; align-items: center; gap: 6px; height: 30px; padding: 0 4px 0 10px; border: 1px solid var(--border);
      border-radius: 8px; background: var(--input); color: var(--text-muted); width: 340px; }
    .search:focus-within { border-color: var(--accent); }
    .search input { flex: 1; min-width: 0; border: 0; background: none; padding: 0; height: 26px; box-shadow: none; }
    @media (max-width: 1100px) { .search { width: 220px; } .tool span { display: none; } .tool { min-width: 36px; } }
  `,
})
export class Toolbar {
  protected readonly actions = inject(GitActions);
  protected readonly ws = inject(Workspace);
  private readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();
  protected readonly searchModes = [
    { value: 'message', label: 'Message' },
    { value: 'author', label: 'Author' },
    { value: 'sha', label: 'SHA' },
    { value: 'content', label: 'Content' },
  ];

  protected readonly query = signal('');
  protected readonly mode = signal<SearchMode>('message');
  private readonly searchInput = viewChild<ElementRef<HTMLInputElement>>('searchInput');

  protected readonly ahead = computed(() => this.repo().status()?.branch.ahead ?? 0);
  protected readonly behind = computed(() => this.repo().status()?.branch.behind ?? 0);

  focusSearch(): void {
    this.searchInput()?.nativeElement.focus();
    this.searchInput()?.nativeElement.select();
  }

  protected async search(): Promise<void> {
    const repo = this.repo();
    repo.view.set('history');
    try {
      await repo.runSearch(this.query(), this.mode());
      const first = repo.search()?.results[0];
      if (first) repo.selectedSha.set(first.sha);
    } catch (e) {
      this.ui.toast('Search failed', 'error', errorMessage(e));
    }
  }

  protected clear(): void {
    this.query.set('');
    this.repo().search.set(null);
  }
}
