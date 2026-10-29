import { computed, signal } from '@angular/core';
import { Api, Params } from './api';
import { ChangeEvent } from './events';
import { CommitDetail, LogCommit, LogResult, Refs, Status } from './models';
import { SettingsStore } from './settings';

export type RepoView = 'history' | 'changes' | 'stash';
export type SearchMode = 'message' | 'author' | 'sha' | 'content';

export interface SearchState {
  query: string;
  mode: SearchMode;
  results: LogCommit[];
  loading: boolean;
}

/** Everything the UI knows about one open repository (one tab). */
export class RepoState {
  readonly name: string;
  readonly view = signal<RepoView>('history');
  readonly status = signal<Status | null>(null);
  readonly refs = signal<Refs | null>(null);
  readonly commits = signal<LogCommit[]>([]);
  readonly hasMore = signal(false);
  readonly logLoading = signal(false);
  readonly logLimit = signal(0);
  readonly selectedSha = signal<string | null>(null);
  readonly selectedStash = signal<string | null>(null);
  readonly search = signal<SearchState | null>(null);
  readonly error = signal<string | null>(null);
  /** Bumped whenever working tree files change, so open diffs reload. */
  readonly worktreeVersion = signal(0);
  /** Two commits being compared in the history view (base -> target). */
  readonly compare = signal<{ base: string; target: string } | null>(null);
  /** Show all branches, or only the history of HEAD. */
  readonly branchFilter = signal<'all' | 'current'>('all');
  /** Commit message being written in Local Changes. */
  readonly draft = signal({ summary: '', body: '', amend: false });

  readonly changeCount = computed(() => {
    const s = this.status();
    if (!s) return 0;
    return new Set([...s.staged, ...s.unstaged, ...s.conflicts].map((f) => f.path)).size;
  });
  readonly currentBranch = computed(() => this.status()?.branch.head ?? null);
  readonly currentBranchInfo = computed(() => this.refs()?.local.find((b) => b.current) ?? null);
  readonly visibleCommits = computed(() => this.search()?.results ?? this.commits());

  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private pending = { status: false, refs: false };

  constructor(
    private readonly api: Api,
    private readonly settings: SettingsStore,
    readonly path: string,
  ) {
    this.name = path.split(/[\\/]/).filter(Boolean).pop() ?? path;
  }

  get<T>(path: string, params: Params = {}): Promise<T> {
    return this.api.get<T>(path, { repo: this.path, ...params });
  }

  getText(path: string, params: Params = {}): Promise<string> {
    return this.api.getText(path, { repo: this.path, ...params });
  }

  post<T>(path: string, body: unknown = {}, params: Params = {}): Promise<T> {
    return this.api.post<T>(path, body, { repo: this.path, ...params });
  }

  async refreshAll(): Promise<void> {
    await Promise.all([this.refreshStatus(), this.refreshRefs(), this.refreshLog()]);
  }

  async refreshStatus(): Promise<void> {
    try {
      this.status.set(await this.get<Status>('/status'));
      this.error.set(null);
    } catch (e) {
      this.error.set(e instanceof Error ? e.message : String(e));
    }
  }

  async refreshRefs(): Promise<void> {
    try {
      this.refs.set(await this.get<Refs>('/refs'));
    } catch {
      // Status refresh reports repository-level errors.
    }
  }

  async refreshLog(limit?: number): Promise<void> {
    const wanted = limit ?? Math.max(this.logLimit(), this.settings.value().logLimit);
    this.logLoading.set(true);
    try {
      const result = await this.get<LogResult>('/log', { limit: wanted, all: this.branchFilter() === 'all' });
      this.logLimit.set(wanted);
      this.commits.set(result.commits);
      this.hasMore.set(result.hasMore);
      const selected = this.selectedSha();
      if (selected && !result.commits.some((c) => c.sha === selected) && !this.search()) {
        this.selectedSha.set(null);
      }
    } finally {
      this.logLoading.set(false);
    }
  }

  loadMore(): Promise<void> {
    return this.refreshLog(this.logLimit() + this.settings.value().logLimit);
  }

  commitDetail(sha: string): Promise<CommitDetail> {
    return this.get<CommitDetail>('/commit', { sha });
  }

  /** A commit from the loaded log, or fetched from the server. */
  async resolveCommit(sha: string): Promise<LogCommit> {
    return this.commits().find((c) => c.sha === sha || c.sha.startsWith(sha)) ?? (await this.commitDetail(sha));
  }

  async runSearch(query: string, mode: SearchMode): Promise<void> {
    if (!query.trim()) {
      this.search.set(null);
      return;
    }
    this.search.set({ query, mode, results: this.search()?.results ?? [], loading: true });
    try {
      const results = await this.get<LogCommit[]>('/search', { q: query, mode });
      if (this.search()?.query === query) this.search.set({ query, mode, results, loading: false });
    } catch (e) {
      this.search.set({ query, mode, results: [], loading: false });
      throw e;
    }
  }

  /** Debounced reaction to file-system events from the backend watcher. */
  onChange(event: ChangeEvent): void {
    this.pending.status = true;
    if (event.git) this.pending.refs = true;
    if (event.worktree) this.worktreeVersion.update((v) => v + 1);
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      const { refs } = this.pending;
      this.pending = { status: false, refs: false };
      void this.refreshStatus();
      if (refs) {
        void this.refreshRefs();
        void this.refreshLog();
      }
    }, 250);
  }
}
