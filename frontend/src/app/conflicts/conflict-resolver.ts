import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { errorMessage } from '../core/api';
import { ConflictChunk, ConflictVersions } from '../core/models';
import { RepoState } from '../core/repo-state';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { Icon } from '../shared/icon';

export type Choice = 'ours' | 'theirs' | 'ours-theirs' | 'theirs-ours' | null;

type Conflict = Extract<ConflictChunk, { type: 'conflict' }>;

/** Rebuild the file from chunks; unresolved conflicts keep their markers. */
export function buildResult(chunks: ConflictChunk[], choices: Choice[]): string {
  const lines: string[] = [];
  let index = 0;
  for (const chunk of chunks) {
    if (chunk.type === 'common') {
      lines.push(...chunk.lines);
      continue;
    }
    const choice = choices[index++];
    if (choice === 'ours') lines.push(...chunk.ours);
    else if (choice === 'theirs') lines.push(...chunk.theirs);
    else if (choice === 'ours-theirs') lines.push(...chunk.ours, ...chunk.theirs);
    else if (choice === 'theirs-ours') lines.push(...chunk.theirs, ...chunk.ours);
    else lines.push('<<<<<<< ours', ...chunk.ours, '=======', ...chunk.theirs, '>>>>>>> theirs');
  }
  return lines.join('\n');
}

@Component({
  selector: 'app-conflict-resolver',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './conflict-resolver.html',
  styleUrl: './conflict-resolver.scss',
})
export class ConflictResolver implements OnInit {
  private readonly ws = inject(Workspace);
  protected readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();
  readonly path = input.required<string>();

  protected readonly data = signal<ConflictVersions | null>(null);
  protected readonly error = signal<string | null>(null);
  protected readonly choices = signal<Choice[]>([]);
  protected readonly result = signal('');
  protected readonly edited = signal(false);
  protected readonly saving = signal(false);

  protected readonly conflicts = computed(() =>
    (this.data()?.chunks ?? []).filter((c): c is Conflict => c.type === 'conflict'),
  );
  /** Up to three common lines before each conflict, for orientation. */
  protected readonly contexts = computed(() => {
    const chunks = this.data()?.chunks ?? [];
    const contexts: string[][] = [];
    chunks.forEach((chunk, i) => {
      if (chunk.type !== 'conflict') return;
      const previous = chunks[i - 1];
      contexts.push(previous?.type === 'common' ? previous.lines.slice(-3) : []);
    });
    return contexts;
  });
  protected readonly unresolved = computed(() => this.choices().filter((c) => c === null).length);
  protected readonly hasMarkers = computed(() => /^(<<<<<<<|>>>>>>>)( |$)/m.test(this.result()));

  async ngOnInit(): Promise<void> {
    try {
      const data = await this.repo().get<ConflictVersions>('/conflict/', { path: this.path() });
      this.data.set(data);
      const count = data.chunks.filter((c) => c.type === 'conflict').length;
      this.choices.set(Array(count).fill(null));
      this.result.set(data.working ?? '');
    } catch (e) {
      this.error.set(errorMessage(e));
    }
  }

  protected async choose(index: number, choice: Choice): Promise<void> {
    if (this.edited()) {
      const ok = await this.ui.confirm(
        'Replace edits?',
        'Choosing a side rebuilds the result and discards your manual edits.',
        'Rebuild',
      );
      if (!ok) return;
    }
    const choices = [...this.choices()];
    choices[index] = choices[index] === choice ? null : choice;
    this.choices.set(choices);
    this.result.set(buildResult(this.data()!.chunks, choices));
    this.edited.set(false);
  }

  protected async chooseAll(choice: Choice): Promise<void> {
    const choices = this.choices().map(() => choice);
    this.choices.set(choices);
    this.result.set(buildResult(this.data()!.chunks, choices));
    this.edited.set(false);
  }

  protected edit(text: string): void {
    this.result.set(text);
    this.edited.set(true);
  }

  protected async save(): Promise<void> {
    if (this.hasMarkers()) {
      const ok = await this.ui.confirm(
        'Conflict markers remain',
        'The result still contains conflict markers. Save and mark the file as resolved anyway?',
        'Save anyway',
        true,
      );
      if (!ok) return;
    }
    await this.resolve({ path: this.path(), content: this.result() });
  }

  protected takeFile(side: 'ours' | 'theirs'): Promise<void> {
    return this.resolve({ path: this.path(), side });
  }

  protected markResolved(): Promise<void> {
    return this.resolve({ path: this.path() });
  }

  private async resolve(body: Record<string, string>): Promise<void> {
    this.saving.set(true);
    const result = await this.ws.run('Resolving conflict', () => this.repo().post('/conflict/resolve', body), this.repo(), {
      success: `Resolved ${this.path()}`,
    });
    this.saving.set(false);
    if (result) this.ui.closeOverlay();
  }

  protected lines(lines: string[]): string {
    return lines.join('\n');
  }
}
