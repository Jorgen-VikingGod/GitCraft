import { CdkDrag, CdkDragDrop, CdkDragHandle, CdkDropList, moveItemInArray } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { LogCommit, OpResult, RebaseAction } from '../core/models';
import { RepoState } from '../core/repo-state';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { Icon } from '../shared/icon';

interface Step {
  commit: LogCommit;
  action: RebaseAction;
  message: string;
  /** Message body (after the summary line), loaded when the commit is reworded. */
  body: string | null;
}

const ACTIONS: { value: RebaseAction; label: string; key: string; hint: string }[] = [
  { value: 'pick', label: 'Pick', key: 'p', hint: 'Use the commit' },
  { value: 'reword', label: 'Reword', key: 'r', hint: 'Use the commit, edit its message' },
  { value: 'edit', label: 'Edit', key: 'e', hint: 'Stop to amend the commit' },
  { value: 'squash', label: 'Squash', key: 's', hint: 'Meld into the commit above, combine messages' },
  { value: 'fixup', label: 'Fixup', key: 'f', hint: 'Meld into the commit above, discard this message' },
  { value: 'drop', label: 'Drop', key: 'd', hint: 'Remove the commit' },
];

@Component({
  selector: 'app-rebase-dialog',
  imports: [CdkDropList, CdkDrag, CdkDragHandle, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './rebase-dialog.html',
  styleUrl: './rebase-dialog.scss',
})
export class RebaseDialog implements OnInit {
  private readonly ws = inject(Workspace);
  protected readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();
  readonly base = input.required<string | null>();

  protected readonly actions = ACTIONS;
  protected readonly steps = signal<Step[]>([]);
  protected readonly loading = signal(true);
  protected readonly running = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly autostash = signal(true);
  protected readonly focused = signal(0);
  protected readonly short = shortSha;

  protected readonly baseLabel = computed(() => {
    const base = this.base();
    if (!base) return 'the root commit';
    const commit = this.repo().commits().find((c) => c.sha === base);
    return commit ? `${shortSha(base)} “${commit.subject}”` : shortSha(base);
  });

  protected readonly problem = computed<string | null>(() => {
    const steps = this.steps();
    const first = steps.find((s) => s.action !== 'drop');
    if (!steps.length) return 'There are no commits to rebase.';
    if (!first) return 'All commits are dropped.';
    if (first.action === 'squash' || first.action === 'fixup') {
      return 'The first remaining commit cannot be squashed or fixed up (there is nothing above it).';
    }
    if (steps.some((s) => s.action === 'reword' && !s.message.trim())) return 'Reworded commits need a message.';
    if (steps.some((s) => s.action === 'reword' && s.body === null)) return 'Loading commit message…';
    return null;
  });

  protected readonly unchanged = computed(() =>
    this.steps().every((s, i) => s.action === 'pick' && s.commit.sha === this.original[i]?.sha),
  );

  private original: LogCommit[] = [];

  async ngOnInit(): Promise<void> {
    try {
      const commits = await this.repo().get<LogCommit[]>('/rebase/commits', { base: this.base() });
      this.original = commits;
      this.steps.set(commits.map((commit) => ({ commit, action: 'pick', message: commit.subject, body: null })));
    } catch (e) {
      this.error.set(errorMessage(e));
    } finally {
      this.loading.set(false);
    }
  }

  protected drop(event: CdkDragDrop<Step[]>): void {
    const steps = [...this.steps()];
    moveItemInArray(steps, event.previousIndex, event.currentIndex);
    this.steps.set(steps);
    this.focused.set(event.currentIndex);
  }

  protected setAction(index: number, action: RebaseAction): void {
    this.steps.update((steps) => steps.map((s, i) => (i === index ? { ...s, action } : s)));
    const step = this.steps()[index];
    if (action === 'reword' && step.body === null) void this.loadBody(step.commit.sha);
  }

  private async loadBody(sha: string): Promise<void> {
    try {
      const detail = await this.repo().commitDetail(sha);
      const body = detail.message.split('\n').slice(1).join('\n').replace(/^\n+/, '');
      this.steps.update((steps) => steps.map((s) => (s.commit.sha === sha ? { ...s, body } : s)));
    } catch (e) {
      this.ui.toast('Could not load the commit message', 'error', errorMessage(e));
    }
  }

  protected setMessage(index: number, message: string): void {
    this.steps.update((steps) => steps.map((s, i) => (i === index ? { ...s, message } : s)));
  }

  protected move(index: number, delta: number): void {
    const target = index + delta;
    const steps = [...this.steps()];
    if (target < 0 || target >= steps.length) return;
    moveItemInArray(steps, index, target);
    this.steps.set(steps);
    this.focused.set(target);
  }

  protected key(event: KeyboardEvent, index: number): void {
    if ((event.target as HTMLElement).tagName === 'INPUT' || (event.target as HTMLElement).tagName === 'SELECT') return;
    const action = ACTIONS.find((a) => a.key === event.key.toLowerCase());
    if (action) {
      event.preventDefault();
      this.setAction(index, action.value);
    } else if (event.key === 'ArrowUp' && event.altKey) {
      event.preventDefault();
      this.move(index, -1);
    } else if (event.key === 'ArrowDown' && event.altKey) {
      event.preventDefault();
      this.move(index, 1);
    }
  }

  protected reset(): void {
    this.steps.set(this.original.map((commit) => ({ commit, action: 'pick', message: commit.subject, body: null })));
  }

  protected async start(): Promise<void> {
    if (this.problem() || this.running()) return;
    const steps = this.steps().map((s) => ({
      sha: s.commit.sha,
      action: s.action,
      message: s.action === 'reword' && s.message.trim() !== s.commit.subject ? fullMessage(s) : null,
    }));
    this.running.set(true);
    const result = await this.ws.run(
      'Rebasing',
      () => this.repo().post<OpResult>('/rebase/interactive', { base: this.base(), steps, autostash: this.autostash() }),
      this.repo(),
      { success: 'Interactive rebase complete' },
    );
    this.running.set(false);
    if (result) this.ui.closeOverlay();
  }
}

/** The edited summary replaces the first line; the original body is kept. */
function fullMessage(step: Step): string {
  return step.body ? `${step.message.trim()}\n\n${step.body}` : step.message.trim();
}
