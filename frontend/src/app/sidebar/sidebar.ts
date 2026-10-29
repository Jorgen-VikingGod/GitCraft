import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { GitActions, joinPath } from '../core/actions';
import { BranchInfo, RemoteBranch, Stash, Submodule, TagInfo } from '../core/models';
import { RepoState } from '../core/repo-state';
import { readStorageValue, writeStorage } from '../core/settings';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { Icon } from '../shared/icon';

export interface TreeRow<T> {
  key: string;
  label: string;
  depth: number;
  folder: boolean;
  item?: T;
}

/** Build a flattened folder tree from slash-separated names. */
export function buildTree<T>(
  items: { name: string; item: T }[],
  prefix: string,
  collapsed: ReadonlySet<string>,
  depth = 0,
): TreeRow<T>[] {
  const folders = new Map<string, { name: string; item: T }[]>();
  const leaves: TreeRow<T>[] = [];
  for (const entry of items) {
    const slash = entry.name.indexOf('/');
    if (slash > 0) {
      const folder = entry.name.slice(0, slash);
      const list = folders.get(folder) ?? [];
      list.push({ name: entry.name.slice(slash + 1), item: entry.item });
      folders.set(folder, list);
    } else {
      leaves.push({ key: `${prefix}/${entry.name}`, label: entry.name, depth, folder: false, item: entry.item });
    }
  }
  const rows: TreeRow<T>[] = [];
  for (const [folder, children] of [...folders].sort(([a], [b]) => a.localeCompare(b))) {
    const key = `${prefix}/${folder}`;
    rows.push({ key, label: folder, depth, folder: true });
    if (!collapsed.has(key)) rows.push(...buildTree(children, key, collapsed, depth + 1));
  }
  return [...rows, ...leaves];
}

const COLLAPSED_KEY = 'gitcraft-sidebar-collapsed';

@Component({
  selector: 'app-sidebar',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './sidebar.html',
  styleUrl: './sidebar.scss',
})
export class Sidebar {
  protected readonly actions = inject(GitActions);
  private readonly ws = inject(Workspace);
  private readonly ui = inject(Ui);
  readonly repo = input.required<RepoState>();

  protected readonly filter = signal('');
  protected readonly collapsed = signal<ReadonlySet<string>>(
    new Set(readStorageValue<string[]>(COLLAPSED_KEY, ['section:submodules'])),
  );

  private readonly refs = computed(() => this.repo().refs());
  private matches(name: string): boolean {
    const f = this.filter().trim().toLowerCase();
    return !f || name.toLowerCase().includes(f);
  }

  protected readonly branchRows = computed(() =>
    buildTree(
      (this.refs()?.local ?? []).filter((b) => this.matches(b.name)).map((b) => ({ name: b.name, item: b })),
      'local',
      this.filter() ? new Set() : this.collapsed(),
    ),
  );

  protected readonly remotes = computed(() =>
    (this.refs()?.remotes ?? []).map((remote) => ({
      remote,
      rows: buildTree(
        (this.refs()?.remote ?? [])
          .filter((b) => b.remote === remote.name && this.matches(b.name))
          .map((b) => ({ name: b.branch, item: b })),
        `remote:${remote.name}`,
        this.filter() ? new Set() : this.collapsed(),
      ),
    })),
  );

  protected readonly tags = computed(() =>
    [...(this.refs()?.tags ?? [])].filter((t) => this.matches(t.name)).sort((a, b) => b.time - a.time || a.name.localeCompare(b.name)),
  );
  protected readonly stashes = computed(() => this.refs()?.stashes ?? []);
  protected readonly submodules = computed(() => this.refs()?.submodules ?? []);

  protected isCollapsed(key: string): boolean {
    return this.collapsed().has(key) && !this.filter();
  }

  protected toggle(key: string): void {
    const next = new Set(this.collapsed());
    if (next.has(key)) next.delete(key);
    else next.add(key);
    this.collapsed.set(next);
    writeStorage(COLLAPSED_KEY, [...next]);
  }

  protected showCommit(sha: string): void {
    const repo = this.repo();
    repo.search.set(null);
    repo.view.set('history');
    repo.selectedSha.set(sha);
  }

  protected openStash(stash: Stash): void {
    this.repo().selectedStash.set(stash.name);
    this.repo().view.set('stash');
  }

  protected checkoutBranch(branch: BranchInfo): void {
    if (!branch.current) void this.actions.checkout(this.repo(), branch.name);
  }

  protected checkoutRemote(branch: RemoteBranch): void {
    void this.actions.checkout(this.repo(), branch.name, 'remote');
  }

  protected tagTitle(tag: TagInfo): string {
    return tag.annotated && tag.message ? `${tag.name}: ${tag.message}` : tag.name;
  }

  protected openSubmodule(module: Submodule): void {
    void this.ws.open(joinPath(this.repo().path, module.path));
  }

  protected submoduleMenu(event: MouseEvent, module: Submodule): void {
    this.ui.openMenu(event, [
      { label: 'Open in new tab', action: () => this.openSubmodule(module) },
      { label: 'Update all submodules', action: () => void this.actions.updateSubmodules(this.repo()) },
    ]);
  }

  protected indent(depth: number): number {
    return 22 + depth * 14;
  }
}
