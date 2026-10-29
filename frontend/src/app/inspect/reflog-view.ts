import { ChangeDetectionStrategy, Component, OnInit, inject, input, signal } from '@angular/core';
import { GitActions, shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { RepoState } from '../core/repo-state';
import { Ui } from '../core/ui';
import { fullDate, shortDate } from '../shared/format';
import { Icon } from '../shared/icon';

interface ReflogEntry {
  sha: string;
  selector: string;
  action: string;
  time: number;
  subject: string;
  author: string;
}

/** HEAD reflog: find and recover commits lost by resets, rebases or deleted branches. */
@Component({
  selector: 'app-reflog-view',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'ui.closeOverlay()' },
  template: `
    <div class="dialog">
      <header>
        <app-icon name="clock" [size]="18" />
        <div class="title">
          <h2>Reflog</h2>
          <div class="path">Every position HEAD has been at. Right-click an entry for all commit actions.</div>
        </div>
        <span class="spacer"></span>
        <button class="icon-btn" (click)="ui.closeOverlay()" title="Close"><app-icon name="x" [size]="18" /></button>
      </header>
      <div class="entries">
        @if (loading()) {
          <div class="empty">Loading…</div>
        } @else if (error()) {
          <div class="empty error">{{ error() }}</div>
        }
        @for (e of entries(); track e.selector) {
          <div class="entry" (contextmenu)="menu($event, e)" (dblclick)="show(e)">
            <code class="selector">{{ e.selector }}</code>
            <code class="sha">{{ short(e.sha) }}</code>
            <span class="action" [title]="e.action">{{ e.action }}</span>
            <span class="subject" [title]="e.subject">{{ e.subject }}</span>
            <span class="date" [title]="long(e.time)">{{ date(e.time) }}</span>
            <span class="buttons">
              <button class="btn small" (click)="show(e)">Show</button>
              <button class="btn small" (click)="actions.newBranch(repo(), e.sha)">Branch…</button>
            </span>
          </div>
        } @empty {
          @if (!loading() && !error()) { <div class="empty">The reflog is empty</div> }
        }
      </div>
    </div>
  `,
  styles: `
    :host { position: fixed; inset: 0; z-index: 800; display: grid; place-items: center; background: var(--backdrop); padding: 16px; }
    .dialog { width: min(1100px, 100%); height: min(760px, 100%); display: flex; flex-direction: column; background: var(--panel);
      border: 1px solid var(--border); border-radius: 12px; box-shadow: var(--shadow); overflow: hidden; }
    header { display: flex; align-items: center; gap: 10px; padding: 14px 18px; border-bottom: 1px solid var(--border); }
    h2 { margin: 0; font-size: 16px; }
    .path { font-size: 12px; color: var(--text-muted); }
    .entries { flex: 1; overflow: auto; }
    .entry { display: flex; align-items: center; gap: 12px; height: 32px; padding: 0 18px; border-bottom: 1px solid var(--border);
      white-space: nowrap; }
    .entry:hover { background: var(--hover); }
    code { font-family: var(--mono); font-size: 12px; }
    .selector { width: 90px; color: var(--text-muted); }
    .sha { width: 64px; color: var(--accent); }
    .action { width: 260px; overflow: hidden; text-overflow: ellipsis; color: var(--text-muted); }
    .subject { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .date { width: 110px; color: var(--text-muted); font-size: 12px; }
    .buttons { display: flex; gap: 4px; visibility: hidden; }
    .entry:hover .buttons { visibility: visible; }
    .empty { padding: 24px; text-align: center; color: var(--text-muted); }
    .empty.error { color: var(--danger); }
  `,
})
export class ReflogView implements OnInit {
  protected readonly ui = inject(Ui);
  protected readonly actions = inject(GitActions);
  readonly repo = input.required<RepoState>();

  protected readonly entries = signal<ReflogEntry[]>([]);
  protected readonly loading = signal(true);
  protected readonly error = signal<string | null>(null);
  protected readonly short = shortSha;
  protected readonly date = shortDate;
  protected readonly long = fullDate;

  async ngOnInit(): Promise<void> {
    try {
      this.entries.set(await this.repo().get<ReflogEntry[]>('/reflog', { limit: 500 }));
    } catch (e) {
      this.error.set(errorMessage(e));
    } finally {
      this.loading.set(false);
    }
  }

  protected show(entry: ReflogEntry): void {
    const repo = this.repo();
    repo.search.set(null);
    repo.compare.set(null);
    repo.view.set('history');
    repo.selectedSha.set(entry.sha);
    this.ui.closeOverlay();
  }

  protected menu(event: MouseEvent, entry: ReflogEntry): void {
    void this.actions.commitMenu(event, this.repo(), entry.sha);
  }
}
