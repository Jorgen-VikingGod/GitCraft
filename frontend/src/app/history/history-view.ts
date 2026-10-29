import { CdkVirtualScrollViewport, ScrollingModule } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, viewChild } from '@angular/core';
import { GitActions, shortSha } from '../core/actions';
import { LogCommit, RefLabel } from '../core/models';
import { RepoState } from '../core/repo-state';
import { readStorageValue } from '../core/settings';
import { Ui } from '../core/ui';
import { fullDate, shortDate } from '../shared/format';
import { Icon } from '../shared/icon';
import { Splitter } from '../shared/splitter';
import { CommitDetails } from './commit-details';
import { CompareView } from './compare-view';
import { GRAPH_PAD, GraphCell, LANE_WIDTH } from './graph-cell';
import { RefBadges } from './ref-badges';

const ROW_HEIGHT = 26;

@Component({
  selector: 'app-history-view',
  imports: [ScrollingModule, GraphCell, RefBadges, CommitDetails, CompareView, Splitter, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="top" [style.height.px]="listHeight()">
      @if (repo().search(); as s) {
        <div class="search-bar">
          <app-icon name="search" [size]="13" />
          @if (s.loading) {
            Searching…
          } @else {
            {{ s.results.length }} commit{{ s.results.length === 1 ? '' : 's' }} matching “{{ s.query }}” ({{ s.mode }})
          }
          <span class="spacer"></span>
          <button class="btn small" (click)="repo().search.set(null)">Clear search</button>
        </div>
      } @else if (repo().changeCount()) {
        <div class="row uncommitted" (click)="repo().view.set('changes')" [style.padding-left.px]="graphWidth()">
          <span class="dot"></span>
          <span class="subject">Uncommitted changes</span>
          <span class="count">{{ repo().changeCount() }} file{{ repo().changeCount() === 1 ? '' : 's' }}</span>
        </div>
      }
      <div class="header row">
        <span class="col-subject" [style.padding-left.px]="search() ? 10 : graphWidth()">
          Description
          @if (!search()) {
            <span class="segmented filter" title="Which branches to show">
              <button [class.active]="repo().branchFilter() === 'all'" (click)="setFilter('all')">All branches</button>
              <button [class.active]="repo().branchFilter() === 'current'" (click)="setFilter('current')">Current branch</button>
            </span>
          }
        </span>
        <span class="col-author">Author</span>
        <span class="col-date">Date</span>
        <span class="col-sha">Commit</span>
      </div>
      <cdk-virtual-scroll-viewport [itemSize]="rowHeight" class="list" tabindex="0" (keydown)="onKey($event)">
        <div *cdkVirtualFor="let c of commits(); trackBy: trackSha; let i = index" class="row commit"
 [class.selected]="c.sha === repo().selectedSha()" [class.head]="c.sha === head()"
          [class.compared]="c.sha === repo().compare()?.base || c.sha === repo().compare()?.target"
          (mousedown)="select(c, $event)" (contextmenu)="menu($event, c)" (dblclick)="activate(c)">
          @if (!search()) {
            <app-graph-cell [row]="c.graph" [width]="graphWidth()" [height]="rowHeight" [merge]="c.parents.length > 1"
              [head]="c.sha === head()" />
          }
          <span class="col-subject" [style.padding-left.px]="search() ? 10 : 0">
            @if (c.refs.length) {
              <app-ref-badges [refs]="c.refs" (menu)="refMenu($event.event, $event.ref, c)" (activate)="refActivate($event)" />
            }
            <span class="subject" [title]="c.subject">{{ c.subject }}</span>
          </span>
          <span class="col-author" [title]="c.author + ' <' + c.email + '>'">{{ c.author }}</span>
          <span class="col-date" [title]="long(c.time)">{{ date(c.time) }}</span>
          <span class="col-sha">{{ short(c.sha) }}</span>
        </div>
        @if (repo().hasMore() && !search()) {
          <div class="more">
            <button class="btn small" (click)="repo().loadMore()" [disabled]="repo().logLoading()">
              {{ repo().logLoading() ? 'Loading…' : 'Load more commits' }}
            </button>
          </div>
        }
      </cdk-virtual-scroll-viewport>
      @if (!commits().length && !repo().logLoading()) {
        <div class="empty">{{ search() ? 'No matching commits' : 'No commits yet' }}</div>
      }
    </div>
    <app-splitter axis="y" [(size)]="listHeight" [min]="120" [max]="2000" [defaultSize]="340"
      storageKey="gitcraft-history-height" />
    <div class="bottom">
      @if (repo().compare(); as cmp) {
        <app-compare-view [repo]="repo()" [base]="cmp.base" [target]="cmp.target" />
      } @else if (repo().selectedSha(); as sha) {
        <app-commit-details [repo]="repo()" [sha]="sha" />
      } @else {
        <div class="empty">Select a commit to see its details</div>
      }
    </div>
  `,
  styleUrl: './history-view.scss',
})
export class HistoryView {
  private readonly actions = inject(GitActions);
  private readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();
  private readonly viewport = viewChild(CdkVirtualScrollViewport);

  protected readonly rowHeight = ROW_HEIGHT;
  protected readonly listHeight = signal(readStorageValue('gitcraft-history-height', 340));
  protected readonly search = computed(() => this.repo().search());
  protected readonly commits = computed(() => this.repo().visibleCommits());
  protected readonly head = computed(() => this.repo().refs()?.head ?? null);
  protected readonly graphWidth = computed(() => {
    const lanes = this.repo().commits().reduce((max, c) => Math.max(max, c.graph?.width ?? 1), 1);
    return GRAPH_PAD * 2 + (Math.min(lanes, 24) - 1) * LANE_WIDTH + 4;
  });

  protected readonly date = shortDate;
  protected readonly long = fullDate;
  protected readonly short = shortSha;
  protected trackSha = (_: number, c: LogCommit) => c.sha;

  constructor() {
    // Select HEAD by default once the log is loaded.
    effect(() => {
      const repo = this.repo();
      const commits = repo.commits();
      if (!repo.selectedSha() && commits.length) {
        const head = repo.refs()?.head;
        repo.selectedSha.set(commits.find((c) => c.sha === head)?.sha ?? commits[0].sha);
      }
    });
    // Keep the list scrolled to the selection when it changes from outside.
    effect(() => {
      const sha = this.repo().selectedSha();
      const index = this.commits().findIndex((c) => c.sha === sha);
      if (index >= 0) queueMicrotask(() => this.reveal(index));
    });
  }

  /** Click selects; Ctrl/Cmd+click compares the selected commit with the clicked one. */
  protected select(commit: LogCommit, event?: MouseEvent): void {
    const repo = this.repo();
    const selected = repo.selectedSha();
    if (event && event.button === 0 && (event.ctrlKey || event.metaKey) && selected && selected !== commit.sha) {
      this.actions.compare(repo, selected, commit.sha);
      return;
    }
    if (event?.button === 0) repo.compare.set(null);
    repo.selectedSha.set(commit.sha);
  }

  protected setFilter(filter: 'all' | 'current'): void {
    this.repo().branchFilter.set(filter);
    void this.repo().refreshLog();
  }

  protected activate(commit: LogCommit): void {
    const branch = commit.refs.find((r) => r.kind === 'local' && !r.current);
    if (branch) void this.actions.checkout(this.repo(), branch.name);
  }

  protected menu(event: MouseEvent, commit: LogCommit): void {
    void this.actions.commitMenu(event, this.repo(), commit);
  }

  /** Right-click on a branch/tag badge: that ref's actions first, then the commit actions. */
  protected refMenu(event: MouseEvent, ref: RefLabel, commit: LogCommit): void {
    const refs = this.repo().refs();
    if (ref.kind === 'local') {
      const branch = refs?.local.find((b) => b.name === ref.name);
      if (branch) return this.actions.branchMenu(event, this.repo(), branch);
    } else if (ref.kind === 'remote') {
      const branch = refs?.remote.find((b) => b.name === ref.name);
      if (branch) return this.actions.remoteBranchMenu(event, this.repo(), branch);
    } else if (ref.kind === 'tag') {
      const tag = refs?.tags.find((t) => t.name === ref.name);
      if (tag) return this.actions.tagMenu(event, this.repo(), tag);
    }
    this.menu(event, commit);
  }

  protected refActivate(ref: RefLabel): void {
    if (ref.kind === 'local' && !ref.current) void this.actions.checkout(this.repo(), ref.name);
    else if (ref.kind === 'remote') void this.actions.checkout(this.repo(), ref.name, 'remote');
  }

  protected onKey(event: KeyboardEvent): void {
    const commits = this.commits();
    if (!commits.length) return;
    const index = commits.findIndex((c) => c.sha === this.repo().selectedSha());
    const page = Math.max(1, Math.floor((this.viewport()?.getViewportSize() ?? 260) / ROW_HEIGHT) - 1);
    const moves: Record<string, number> = { ArrowDown: 1, ArrowUp: -1, PageDown: page, PageUp: -page };
    let next: number | null = null;
    if (event.key in moves) next = Math.max(0, Math.min(commits.length - 1, index + moves[event.key]));
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = commits.length - 1;
    if (next === null) return;
    event.preventDefault();
    this.repo().selectedSha.set(commits[next].sha);
  }

  private reveal(index: number): void {
    const viewport = this.viewport();
    if (!viewport) return;
    const top = viewport.measureScrollOffset('top');
    const height = viewport.getViewportSize();
    const rowTop = index * ROW_HEIGHT;
    if (rowTop < top) viewport.scrollToOffset(rowTop);
    else if (rowTop + ROW_HEIGHT > top + height) viewport.scrollToOffset(rowTop + ROW_HEIGHT - height);
  }
}
