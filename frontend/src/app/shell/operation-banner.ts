import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { GitActions } from '../core/actions';
import { RepoState } from '../core/repo-state';
import { Icon } from '../shared/icon';

const TITLES: Record<string, string> = {
  merge: 'Merge in progress',
  rebase: 'Rebase in progress',
  'cherry-pick': 'Cherry-pick in progress',
  revert: 'Revert in progress',
  am: 'Applying patches (git am)',
};

/** Shown while a merge/rebase/cherry-pick/revert is stopped half-way. */
@Component({
  selector: 'app-operation-banner',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (operation(); as op) {
      <div class="banner" [class.conflicted]="conflicts()">
        <app-icon [name]="conflicts() ? 'alert' : 'merge'" [size]="16" />
        <div class="text">
          <strong>{{ title() }}</strong>
          @if (op.kind === 'rebase' && op.step && op.total) {
            <span> · step {{ op.step }} of {{ op.total }}</span>
          }
          @if (op.branch) { <span> · {{ op.branch }}</span> }
          <div class="hint">
            @if (conflicts()) {
              {{ conflicts() }} conflicted file{{ conflicts() === 1 ? '' : 's' }}. Resolve them in Local Changes, then continue.
            } @else if (op.kind === 'rebase' && op.stoppedSha) {
              Stopped at {{ op.stoppedSha.slice(0, 7) }}. Amend or make changes, then continue.
            } @else {
              All conflicts resolved. Continue to finish.
            }
          </div>
        </div>
        @if (conflicts()) {
          <button class="btn small" (click)="repo().view.set('changes')">Show conflicts</button>
        }
        @if (op.kind !== 'merge') {
          <button class="btn small" (click)="actions.skipOperation(repo())">Skip</button>
        }
        <button class="btn small danger-text" (click)="actions.abortOperation(repo())">Abort</button>
        <button class="btn small primary" [disabled]="conflicts() > 0" (click)="actions.continueOperation(repo())">
          Continue
        </button>
      </div>
    }
  `,
  styles: `
    .banner { display: flex; align-items: center; gap: 10px; padding: 8px 14px; background: var(--info-bg);
      border-bottom: 1px solid var(--border); }
    .banner.conflicted { background: var(--warning-bg); }
    .text { flex: 1; min-width: 0; }
    .hint { font-size: 12px; color: var(--text-muted); }
  `,
})
export class OperationBanner {
  protected readonly actions = inject(GitActions);
  readonly repo = input.required<RepoState>();
  protected readonly operation = computed(() => this.repo().status()?.operation ?? null);
  protected readonly conflicts = computed(() => this.repo().status()?.conflicts.length ?? 0);
  protected readonly title = computed(() => TITLES[this.operation()?.kind ?? ''] ?? 'Operation in progress');
}
