import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal, untracked } from '@angular/core';
import { errorMessage } from '../core/api';
import { OpResult } from '../core/models';
import { RepoState } from '../core/repo-state';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';

@Component({
  selector: 'app-commit-box',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = repo().draft();
    <div class="summary-row">
      <input class="summary" placeholder="Commit summary" [value]="d.summary" maxlength="500"
        (input)="patch({ summary: $any($event.target).value })" (keydown)="key($event)" />
      <span class="count" [class.warn]="d.summary.length > 72">{{ d.summary.length }}</span>
    </div>
    <textarea class="body" placeholder="Description" [value]="d.body" rows="4"
      (input)="patch({ body: $any($event.target).value })" (keydown)="key($event)"></textarea>
    <div class="actions">
      <label class="check">
        <input type="checkbox" [checked]="d.amend" (change)="toggleAmend($any($event.target).checked)"
          [disabled]="!hasHead()" />
        Amend last commit
      </label>
      <span class="spacer"></span>
      <button class="btn primary" [disabled]="!canCommit() || committing()" (click)="commit()"
        title="Commit (Ctrl+Enter)">
        {{ committing() ? 'Committing…' : label() }}
      </button>
    </div>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 6px; padding: 10px; border-top: 1px solid var(--border);
      background: var(--panel); }
    .summary-row { position: relative; }
    .summary { width: 100%; padding-right: 36px; font-weight: 600; }
    .count { position: absolute; right: 8px; top: 50%; transform: translateY(-50%); font-size: 11px;
      color: var(--text-faint); }
    .count.warn { color: var(--warning); }
    .body { width: 100%; resize: vertical; min-height: 60px; font-family: inherit; }
    .actions { display: flex; align-items: center; gap: 8px; }
    .check { display: flex; align-items: center; gap: 6px; font-size: 12px; color: var(--text-muted); }
  `,
})
export class CommitBox {
  private readonly ws = inject(Workspace);
  private readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();
  protected readonly committing = signal(false);
  private templateLoaded = new WeakSet<RepoState>();

  protected readonly hasHead = computed(() => !!this.repo().status()?.branch.oid);
  protected readonly stagedCount = computed(() => this.repo().status()?.staged.length ?? 0);
  protected readonly canCommit = computed(() => {
    const d = this.repo().draft();
    const status = this.repo().status();
    if (!d.summary.trim() || !status || status.conflicts.length) return false;
    return d.amend || this.stagedCount() > 0 || status.operation?.kind === 'merge';
  });
  protected readonly label = computed(() => {
    if (this.repo().draft().amend) return 'Amend commit';
    const n = this.stagedCount();
    return n ? `Commit ${n} file${n === 1 ? '' : 's'}` : 'Commit';
  });

  constructor() {
    effect(() => {
      const repo = this.repo();
      if (this.templateLoaded.has(repo)) return;
      this.templateLoaded.add(repo);
      untracked(() => void this.loadTemplate(repo));
    });
  }

  private async loadTemplate(repo: RepoState): Promise<void> {
    const draft = repo.draft();
    if (draft.summary || draft.body) return;
    try {
      const { template } = await repo.get<{ template: string }>('/commit/template');
      if (template) this.fill(repo, template, false);
    } catch {
      // No template configured.
    }
  }

  protected patch(change: Partial<{ summary: string; body: string }>): void {
    this.repo().draft.update((d) => ({ ...d, ...change }));
  }

  protected async toggleAmend(amend: boolean): Promise<void> {
    const repo = this.repo();
    repo.draft.update((d) => ({ ...d, amend }));
    const d = repo.draft();
    if (amend && !d.summary && !d.body) {
      const { message } = await repo.get<{ message: string }>('/commit/head-message');
      this.fill(repo, message, true);
    }
  }

  private fill(repo: RepoState, message: string, amend: boolean): void {
    const lines = message.replace(/\r\n/g, '\n').split('\n');
    const summary = lines[0] ?? '';
    const body = lines.slice(1).join('\n').replace(/^\n+/, '');
    repo.draft.set({ summary, body, amend });
  }

  protected key(event: KeyboardEvent): void {
    if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void this.commit();
    }
  }

  protected async commit(): Promise<void> {
    if (!this.canCommit() || this.committing()) return;
    const repo = this.repo();
    const d = repo.draft();
    const message = d.body.trim() ? `${d.summary.trim()}\n\n${d.body.trim()}\n` : `${d.summary.trim()}\n`;
    this.committing.set(true);
    try {
      const result = await this.ws.run(
        d.amend ? 'Amending commit' : 'Committing',
        () => repo.post<OpResult>('/commit', { message, amend: d.amend }),
        repo,
      );
      if (result?.ok) {
        repo.draft.set({ summary: '', body: '', amend: false });
        this.ui.toast(d.amend ? 'Commit amended' : 'Committed', 'success');
      }
    } catch (e) {
      this.ui.toast('Commit failed', 'error', errorMessage(e));
    } finally {
      this.committing.set(false);
    }
  }
}
