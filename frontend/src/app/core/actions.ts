import { Injectable, inject } from '@angular/core';
import { desktop } from './desktop';
import { errorMessage } from './api';
import { BranchInfo, LogCommit, OpResult, RebaseAction, Remote, RemoteBranch, Stash, TagInfo } from './models';
import { RepoState } from './repo-state';
import { readStorage, writeStorage } from './settings';
import { MenuItem, Ui, copyText } from './ui';
import { Workspace } from './workspace';

export const shortSha = (sha: string) => sha.slice(0, 7);

/** Last choices in the pull dialog, reused as defaults next time. */
const PULL_KEY = 'gitcraft-pull';
const PULL_DEFAULTS = { mode: 'merge', autostash: false };
/** Last choice in the rebase dialog. */
const REBASE_KEY = 'gitcraft-rebase';
const REBASE_DEFAULTS = { autostash: false };

/** User-facing git commands with their dialogs and context menus. */
@Injectable({ providedIn: 'root' })
export class GitActions {
  private readonly ws = inject(Workspace);
  private readonly ui = inject(Ui);

  // ------------------------------------------------------------- remote ops

  fetch(repo: RepoState, remote: string | null = null): Promise<unknown> {
    return this.ws.run(
      remote ? `Fetching ${remote}` : 'Fetching',
      (opId) => repo.post('/fetch', { remote, prune: true, opId }),
      repo,
      { success: 'Fetch complete' },
    );
  }

  async pull(repo: RepoState): Promise<void> {
    const branch = repo.currentBranchInfo();
    const remotes = repo.refs()?.remotes ?? [];
    if (!remotes.length) {
      this.ui.toast('This repository has no remotes', 'warning');
      return;
    }
    const [upRemote, upBranch] = splitUpstream(branch?.upstream ?? null, remotes);
    const last = readStorage(PULL_KEY, PULL_DEFAULTS);
    const values = await this.ui.ask({
      title: 'Pull',
      message: branch ? `Pull into ${branch.name}` : undefined,
      fields: [
        remoteField(remotes, upRemote),
        { key: 'branch', label: 'Remote branch', type: 'text', value: upBranch ?? branch?.name ?? '' },
        {
          key: 'mode',
          label: 'Strategy',
          type: 'select',
          value: last.mode,
          options: [
            { value: 'merge', label: 'Merge (create a merge commit if needed)' },
            { value: 'rebase', label: 'Rebase (replay local commits on top)' },
            { value: 'ff-only', label: 'Fast-forward only' },
          ],
        },
        { key: 'autostash', label: 'Stash and reapply local changes', type: 'checkbox', value: last.autostash },
      ],
      ok: 'Pull',
    });
    if (!values) return;
    const mode = String(values['mode']);
    const autostash = !!values['autostash'];
    writeStorage(PULL_KEY, { mode, autostash });
    await this.ws.run(
      mode === 'rebase' ? 'Pulling (rebase)' : 'Pulling',
      (opId) =>
        repo.post<OpResult>('/pull', { remote: values['remote'], branch: values['branch'] || null, mode, autostash, opId }),
      repo,
      { success: 'Pull complete' },
    );
  }

  async push(repo: RepoState, branchName?: string): Promise<void> {
    const refs = repo.refs();
    const branch = refs?.local.find((b) => b.name === (branchName ?? repo.currentBranch())) ?? null;
    const remotes = refs?.remotes ?? [];
    if (!remotes.length) {
      this.ui.toast('This repository has no remotes. Add one from the sidebar first.', 'warning');
      return;
    }
    if (!branch) {
      this.ui.toast('Check out a branch before pushing', 'warning');
      return;
    }
    const [upRemote] = splitUpstream(branch.upstream, remotes);
    const values = await this.ui.ask({
      title: 'Push',
      message: `Push ${branch.name}`,
      fields: [
        remoteField(remotes, upRemote),
        { key: 'setUpstream', label: 'Track remote branch (set upstream)', type: 'checkbox', value: !branch.upstream },
        { key: 'tags', label: 'Push all tags', type: 'checkbox', value: false },
        { key: 'force', label: 'Force push (with lease)', type: 'checkbox', value: false },
      ],
      ok: 'Push',
    });
    if (!values) return;
    await this.ws.run(
      'Pushing',
      (opId) =>
        repo.post('/push', {
          remote: values['remote'],
          branch: branch.name,
          setUpstream: values['setUpstream'],
          force: values['force'],
          tags: values['tags'],
          opId,
        }),
      repo,
      { success: 'Push complete' },
    );
  }

  // --------------------------------------------------------------- stash

  async stash(repo: RepoState): Promise<void> {
    if (!repo.changeCount()) {
      this.ui.toast('There are no local changes to stash', 'info');
      return;
    }
    const values = await this.ui.ask({
      title: 'Stash changes',
      fields: [
        { key: 'message', label: 'Message', type: 'text', placeholder: 'Optional description' },
        { key: 'includeUntracked', label: 'Include untracked files', type: 'checkbox', value: true },
        { key: 'stagedOnly', label: 'Stash staged changes only', type: 'checkbox', value: false },
        { key: 'keepIndex', label: 'Keep staged changes in the working tree', type: 'checkbox', value: false },
      ],
      ok: 'Stash',
    });
    if (!values) return;
    await this.ws.run('Stashing', () => repo.post('/stash/', values), repo, { success: 'Changes stashed' });
  }

  applyStash(repo: RepoState, stash: Stash, pop: boolean): Promise<unknown> {
    return this.ws.run(pop ? 'Popping stash' : 'Applying stash', () => repo.post('/stash/apply', { name: stash.name, pop }), repo, {
      success: pop ? 'Stash popped' : 'Stash applied',
    });
  }

  async dropStash(repo: RepoState, stash: Stash): Promise<void> {
    if (!(await this.ui.confirm('Drop stash', `Delete "${stash.message}"? This cannot be undone.`, 'Drop', true))) return;
    if (repo.selectedStash() === stash.name) {
      repo.selectedStash.set(null);
      repo.view.set('history');
    }
    await this.ws.run('Dropping stash', () => repo.post('/stash/drop', { name: stash.name }), repo);
  }

  // ------------------------------------------------------------ branches

  async newBranch(repo: RepoState, start: string | null = null, label?: string): Promise<void> {
    const values = await this.ui.ask({
      title: 'New branch',
      message: `Based on ${label ?? (start ? shortSha(start) : repo.currentBranch() ?? 'HEAD')}`,
      fields: [
        { key: 'name', label: 'Branch name', type: 'text', required: true, placeholder: 'feature/my-change' },
        { key: 'checkout', label: 'Check out after creating', type: 'checkbox', value: true },
      ],
      ok: 'Create branch',
    });
    if (!values) return;
    const name = String(values['name']).trim().replace(/\s+/g, '-');
    await this.ws.run('Creating branch', () => repo.post('/branch', { name, start, checkout: values['checkout'] }), repo, {
      success: `Created ${name}`,
    });
  }

  checkout(repo: RepoState, ref: string, kind: 'local' | 'remote' | 'commit' = 'local'): Promise<unknown> {
    return this.ws.run(`Checking out ${kind === 'commit' ? shortSha(ref) : ref}`, () => repo.post('/checkout', { ref, kind }), repo);
  }

  async checkoutCommit(repo: RepoState, sha: string): Promise<void> {
    const ok = await this.ui.confirm(
      'Checkout commit',
      `Check out ${shortSha(sha)}? You will be in "detached HEAD" state; create a branch to keep new commits.`,
      'Checkout',
    );
    if (ok) await this.checkout(repo, sha, 'commit');
  }

  async renameBranch(repo: RepoState, branch: BranchInfo): Promise<void> {
    const values = await this.ui.ask({
      title: 'Rename branch',
      fields: [{ key: 'name', label: 'New name', type: 'text', value: branch.name, required: true }],
      ok: 'Rename',
    });
    if (values) {
      await this.ws.run('Renaming branch', () => repo.post('/branch/rename', { old: branch.name, new: values['name'] }), repo);
    }
  }

  async deleteBranch(repo: RepoState, branch: BranchInfo): Promise<void> {
    const remote = branch.upstream ? branch.upstream : null;
    const values = await this.ui.ask({
      title: 'Delete branch',
      message: `Delete local branch ${branch.name}?`,
      fields: [
        { key: 'force', label: 'Force delete, even if it is not merged', type: 'checkbox', value: false },
        ...(remote
          ? [{ key: 'remote', label: `Also delete ${remote} on the remote`, type: 'checkbox' as const, value: false }]
          : []),
      ],
      ok: 'Delete',
      danger: true,
    });
    if (!values) return;
    await this.ws.run('Deleting branch', async (opId) => {
      await repo.post('/branch/delete', { name: branch.name, force: values['force'] });
      if (values['remote'] && remote) {
        const [r, b] = splitUpstream(remote, repo.refs()?.remotes ?? []);
        await repo.post('/branch/delete-remote', { remote: r, branch: b, opId });
      }
    }, repo);
  }

  async deleteRemoteBranch(repo: RepoState, branch: RemoteBranch): Promise<void> {
    if (!(await this.ui.confirm('Delete remote branch', `Delete ${branch.name} from ${branch.remote}?`, 'Delete', true))) return;
    await this.ws.run(
      'Deleting remote branch',
      (opId) => repo.post('/branch/delete-remote', { remote: branch.remote, branch: branch.branch, opId }),
      repo,
    );
  }

  async setUpstream(repo: RepoState, branch: BranchInfo): Promise<void> {
    const remotes = repo.refs()?.remote ?? [];
    const values = await this.ui.ask({
      title: 'Tracking branch',
      message: `Choose the upstream of ${branch.name}`,
      fields: [
        {
          key: 'upstream',
          label: 'Upstream',
          type: 'select',
          value: branch.upstream ?? '',
          options: [{ value: '', label: '(none)' }, ...remotes.map((r) => ({ value: r.name, label: r.name }))],
        },
      ],
      ok: 'Save',
    });
    if (values) {
      await this.ws.run('Setting upstream', () =>
        repo.post('/branch/upstream', { branch: branch.name, upstream: values['upstream'] || null }), repo);
    }
  }

  // ------------------------------------------------------ merge & rebase

  async merge(repo: RepoState, ref: string): Promise<void> {
    const current = repo.currentBranch() ?? 'HEAD';
    const values = await this.ui.ask({
      title: 'Merge',
      message: `Merge ${ref} into ${current}`,
      fields: [
        {
          key: 'mode',
          label: 'Options',
          type: 'select',
          value: 'default',
          options: [
            { value: 'default', label: 'Default (fast-forward if possible)' },
            { value: 'no-ff', label: 'Always create a merge commit (--no-ff)' },
            { value: 'ff-only', label: 'Fast-forward only' },
            { value: 'squash', label: 'Squash (stage changes, do not commit)' },
          ],
        },
      ],
      ok: 'Merge',
    });
    if (!values) return;
    await this.ws.run('Merging', () => repo.post<OpResult>('/merge', { ref, mode: values['mode'] }), repo, {
      success: `Merged ${ref}`,
    });
    if (values['mode'] === 'squash') repo.view.set('changes');
  }

  async rebase(repo: RepoState, onto: string, label = onto): Promise<void> {
    const current = repo.currentBranch() ?? 'HEAD';
    const last = readStorage(REBASE_KEY, REBASE_DEFAULTS);
    const values = await this.ui.ask({
      title: 'Rebase',
      message: `Rebase ${current} onto ${label}?`,
      fields: [{ key: 'autostash', label: 'Stash and reapply local changes', type: 'checkbox', value: last.autostash }],
      ok: 'Rebase',
    });
    if (!values) return;
    const autostash = !!values['autostash'];
    writeStorage(REBASE_KEY, { autostash });
    await this.ws.run('Rebasing', () => repo.post<OpResult>('/rebase', { onto, autostash }), repo, {
      success: 'Rebase complete',
    });
  }

  interactiveRebase(repo: RepoState, base: string | null): void {
    this.ui.open({ kind: 'rebase', repo, base });
  }

  cherryPick(repo: RepoState, commit: LogCommit): Promise<unknown> {
    return this.ws.run('Cherry-picking', () => repo.post<OpResult>('/cherry-pick', { shas: [commit.sha] }), repo, {
      success: `Cherry-picked ${shortSha(commit.sha)}`,
    });
  }

  async revert(repo: RepoState, commit: LogCommit): Promise<void> {
    let mainline: number | null = null;
    if (commit.parents.length > 1) {
      const values = await this.ui.ask({
        title: 'Revert merge commit',
        message: 'Choose the parent that should be kept (the mainline).',
        fields: [
          {
            key: 'mainline',
            label: 'Mainline parent',
            type: 'select',
            value: '1',
            options: commit.parents.map((p, i) => ({ value: String(i + 1), label: `${i + 1}: ${shortSha(p)}` })),
          },
        ],
        ok: 'Revert',
      });
      if (!values) return;
      mainline = Number(values['mainline']);
    } else if (!(await this.ui.confirm('Revert commit', `Create a commit that reverts "${commit.subject}"?`, 'Revert'))) {
      return;
    }
    await this.ws.run('Reverting', () => repo.post<OpResult>('/revert', { sha: commit.sha, mainline }), repo, {
      success: 'Commit reverted',
    });
  }

  continueOperation(repo: RepoState): Promise<unknown> {
    return this.ws.run('Continuing', () => repo.post<OpResult>('/operation/continue'), repo);
  }

  async abortOperation(repo: RepoState): Promise<void> {
    const kind = repo.status()?.operation?.kind ?? 'operation';
    if (await this.ui.confirm(`Abort ${kind}`, `Abort the ${kind} and restore the previous state?`, 'Abort', true)) {
      await this.ws.run('Aborting', () => repo.post('/operation/abort'), repo);
    }
  }

  skipOperation(repo: RepoState): Promise<unknown> {
    return this.ws.run('Skipping', () => repo.post<OpResult>('/operation/skip'), repo);
  }

  // ------------------------------------------------------- tags & remotes

  async createTag(repo: RepoState, target: string): Promise<void> {
    const values = await this.ui.ask({
      title: 'New tag',
      message: `Tag ${shortSha(target)}`,
      fields: [
        { key: 'name', label: 'Tag name', type: 'text', required: true, placeholder: 'v1.0.0' },
        { key: 'message', label: 'Message (creates an annotated tag)', type: 'textarea' },
        ...(repo.refs()?.remotes.length
          ? [{ key: 'push', label: 'Push to all remotes', type: 'checkbox' as const, value: false }]
          : []),
      ],
      ok: 'Create tag',
    });
    if (!values) return;
    const name = String(values['name']).trim();
    await this.ws.run('Creating tag', async (opId) => {
      await repo.post('/tag', { name, target, message: values['message'] || null });
      if (values['push']) {
        for (const remote of repo.refs()?.remotes ?? []) {
          await repo.post('/tag/push', { remote: remote.name, name, opId });
        }
      }
    }, repo, { success: `Created tag ${name}` });
  }

  async deleteTag(repo: RepoState, tag: TagInfo): Promise<void> {
    const remotes = repo.refs()?.remotes ?? [];
    const values = await this.ui.ask({
      title: 'Delete tag',
      message: `Delete tag ${tag.name}?`,
      fields: remotes.length
        ? [{ key: 'remote', label: 'Also delete from all remotes', type: 'checkbox', value: false }]
        : [],
      ok: 'Delete',
      danger: true,
    });
    if (!values) return;
    await this.ws.run('Deleting tag', async (opId) => {
      await repo.post('/tag/delete', { name: tag.name });
      if (values['remote']) {
        for (const remote of remotes) await repo.post('/tag/push', { remote: remote.name, name: tag.name, delete: true, opId });
      }
    }, repo);
  }

  async pushTag(repo: RepoState, tag: TagInfo): Promise<void> {
    const remotes = repo.refs()?.remotes ?? [];
    const values = await this.ui.ask({ title: 'Push tag', message: tag.name, fields: [remoteField(remotes, null)], ok: 'Push' });
    if (values) {
      await this.ws.run('Pushing tag', (opId) => repo.post('/tag/push', { remote: values['remote'], name: tag.name, opId }), repo, {
        success: `Pushed ${tag.name}`,
      });
    }
  }

  async addRemote(repo: RepoState): Promise<void> {
    const values = await this.ui.ask({
      title: 'Add remote',
      fields: [
        { key: 'name', label: 'Name', type: 'text', value: repo.refs()?.remotes.length ? '' : 'origin', required: true },
        { key: 'url', label: 'URL', type: 'text', required: true, placeholder: 'git@github.com:user/repo.git' },
        { key: 'fetch', label: 'Fetch after adding', type: 'checkbox', value: true },
      ],
      ok: 'Add remote',
    });
    if (!values) return;
    await this.ws.run('Adding remote', async (opId) => {
      await repo.post('/remote', { name: values['name'], url: values['url'] });
      if (values['fetch']) await repo.post('/fetch', { remote: values['name'], opId });
    }, repo);
  }

  async editRemote(repo: RepoState, remote: Remote): Promise<void> {
    const values = await this.ui.ask({
      title: 'Edit remote',
      fields: [
        { key: 'newName', label: 'Name', type: 'text', value: remote.name, required: true },
        { key: 'url', label: 'URL', type: 'text', value: remote.fetchUrl, required: true },
      ],
      ok: 'Save',
    });
    if (values) {
      await this.ws.run('Saving remote', () => repo.post('/remote/edit', { name: remote.name, ...values }), repo);
    }
  }

  async removeRemote(repo: RepoState, remote: Remote): Promise<void> {
    if (await this.ui.confirm('Remove remote', `Remove remote ${remote.name} and its remote-tracking branches?`, 'Remove', true)) {
      await this.ws.run('Removing remote', () => repo.post('/remote/remove', { name: remote.name }), repo);
    }
  }

  // ------------------------------------------------------- commit editing

  /** Reset the current branch to `commit` with the given mode (hard asks first). */
  async resetTo(repo: RepoState, commit: LogCommit, mode: ResetMode): Promise<void> {
    const branch = repo.currentBranch() ?? 'HEAD';
    if (mode === 'hard') {
      const sure = await this.ui.confirm(
        'Hard reset',
        `Reset ${branch} to ${shortSha(commit.sha)} "${commit.subject}" and discard ALL uncommitted changes? This cannot be undone.`,
        'Reset (hard)',
        true,
      );
      if (!sure) return;
    }
    await this.ws.run(`Resetting (${mode})`, () => repo.post('/reset', { target: commit.sha, mode }), repo, {
      success: `${branch} reset to ${shortSha(commit.sha)}`,
    });
  }

  async undoLastCommit(repo: RepoState): Promise<void> {
    const ok = await this.ui.confirm(
      'Undo last commit',
      'Remove the last commit from the branch and keep its changes staged?',
      'Undo commit',
    );
    if (ok) {
      await this.ws.run('Undoing commit', () => repo.post('/reset', { target: 'HEAD~1', mode: 'soft' }), repo, {
        success: 'Last commit undone; its changes are staged',
      });
      repo.view.set('changes');
    }
  }

  async editMessage(repo: RepoState, commit: LogCommit): Promise<void> {
    const detail = await repo.commitDetail(commit.sha);
    const values = await this.ui.ask({
      title: 'Edit commit message',
      message: `${shortSha(commit.sha)}: commits after it are rewritten.`,
      fields: [{ key: 'message', label: 'Message', type: 'textarea', value: detail.message, required: true }],
      ok: 'Save message',
    });
    if (!values) return;
    await this.quickRebase(repo, commit, 'reword', String(values['message']), 0);
  }

  async squashIntoParent(repo: RepoState, commit: LogCommit, fixup: boolean): Promise<void> {
    const verb = fixup ? 'Fixup' : 'Squash';
    const ok = await this.ui.confirm(
      `${verb} into parent`,
      `${verb} "${commit.subject}" into its parent commit${fixup ? ', discarding its message' : ', combining both messages'}?`,
      verb,
    );
    if (ok) await this.quickRebase(repo, commit, fixup ? 'fixup' : 'squash', null, 1);
  }

  async dropCommit(repo: RepoState, commit: LogCommit): Promise<void> {
    const ok = await this.ui.confirm('Drop commit', `Remove "${commit.subject}" from the branch history?`, 'Drop', true);
    if (ok) await this.quickRebase(repo, commit, 'drop', null, 0);
  }

  /**
   * Rewrites history with a one-step interactive rebase. `depth` is how many
   * parents the base lies below the commit (1 = include the parent, for squash).
   */
  private async quickRebase(
    repo: RepoState,
    commit: LogCommit,
    action: RebaseAction,
    message: string | null,
    depth: number,
  ): Promise<void> {
    if (commit.parents.length > 1) {
      this.ui.toast('Merge commits cannot be edited this way', 'warning');
      return;
    }
    try {
      let base: string | null = commit.parents[0] ?? null;
      if (depth === 1) {
        if (!base) {
          this.ui.toast('The first commit has no parent to squash into', 'warning');
          return;
        }
        base = (await repo.resolveCommit(base)).parents[0] ?? null;
      }
      const candidates = await repo.get<LogCommit[]>('/rebase/commits', { base });
      if (!candidates.some((c) => c.sha === commit.sha)) {
        this.ui.toast('Only commits on the current branch can be edited', 'warning');
        return;
      }
      const steps = candidates.map((c) => ({
        sha: c.sha,
        action: c.sha === commit.sha ? action : 'pick',
        message: c.sha === commit.sha ? message : null,
      }));
      await this.ws.run(
        'Rewriting history',
        () => repo.post<OpResult>('/rebase/interactive', { base, steps, autostash: true }),
        repo,
        { success: 'History rewritten' },
      );
    } catch (e) {
      this.ui.toast('Could not edit the commit', 'error', errorMessage(e));
    }
  }

  // --------------------------------------------------------------- tools

  async savePatch(repo: RepoState, commit: LogCommit): Promise<void> {
    try {
      const text = await repo.getText('/patch', { sha: commit.sha });
      const slug = commit.subject.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 50);
      download(`${shortSha(commit.sha)}-${slug || 'commit'}.patch`, text);
    } catch (e) {
      this.ui.toast('Could not create patch', 'error', errorMessage(e));
    }
  }

  async applyPatch(repo: RepoState): Promise<void> {
    const file = await pickFile('.patch,.diff,.eml,.txt');
    if (!file) return;
    const patch = await file.text();
    const isMailbox = /^From [0-9a-f]{40} /m.test(patch);
    const values = await this.ui.ask({
      title: 'Apply patch',
      message: file.name,
      fields: [
        {
          key: 'mode',
          label: 'How',
          type: 'select',
          value: isMailbox ? 'am' : 'apply',
          options: [
            { value: 'am', label: 'Create commits (git am, for format-patch files)' },
            { value: 'apply', label: 'Apply changes only, staged (git apply)' },
          ],
        },
      ],
      ok: 'Apply',
    });
    if (!values) return;
    await this.ws.run('Applying patch', () => repo.post<OpResult>('/patch/apply', { patch, mode: values['mode'] }), repo, {
      success: 'Patch applied',
    });
  }

  fastForward(repo: RepoState, branch: string, target: string | null = null): Promise<unknown> {
    return this.ws.run('Fast-forwarding', () => repo.post('/branch/fast-forward', { branch, target }), repo, {
      success: `${branch} fast-forwarded`,
    });
  }

  ignore(repo: RepoState, pattern: string): Promise<unknown> {
    return this.ws.run('Updating .gitignore', () => repo.post('/ignore', { pattern }), repo, {
      success: `Added "${pattern}" to .gitignore`,
    });
  }

  /** Menu entries for ignoring a file, its extension or its folder. */
  ignoreItems(repo: RepoState, path: string): MenuItem[] {
    const name = path.split('/').pop() ?? path;
    const ext = name.includes('.') && !name.startsWith('.') ? name.slice(name.lastIndexOf('.')) : null;
    const dir = path.includes('/') ? path.slice(0, path.lastIndexOf('/') + 1) : null;
    return [
      { label: `Ignore "${path}"`, action: () => void this.ignore(repo, '/' + path) },
      ...(ext ? [{ label: `Ignore all "*${ext}" files`, action: () => void this.ignore(repo, '*' + ext) }] : []),
      ...(dir ? [{ label: `Ignore folder "${dir}"`, action: () => void this.ignore(repo, '/' + dir) }] : []),
    ];
  }

  async stashFiles(repo: RepoState, paths: string[]): Promise<void> {
    const values = await this.ui.ask({
      title: 'Stash selected files',
      message: paths.length === 1 ? paths[0] : `${paths.length} files`,
      fields: [{ key: 'message', label: 'Message', type: 'text', placeholder: 'Optional description' }],
      ok: 'Stash',
    });
    if (!values) return;
    await this.ws.run(
      'Stashing',
      () => repo.post('/stash/', { message: values['message'] || null, includeUntracked: true, paths }),
      repo,
      { success: 'Files stashed' },
    );
  }

  async clean(repo: RepoState): Promise<void> {
    const values = await this.ui.ask({
      title: 'Clean working tree',
      message: 'Delete untracked files from the working tree.',
      fields: [
        { key: 'directories', label: 'Include untracked folders', type: 'checkbox', value: true },
        { key: 'ignored', label: 'Also delete ignored files (build output, dependencies…)', type: 'checkbox', value: false },
      ],
      ok: 'Preview…',
    });
    if (!values) return;
    const options = { directories: values['directories'], ignored: values['ignored'] };
    try {
      const preview = await repo.post<{ files: string[] }>('/clean', { ...options, dryRun: true });
      if (!preview.files.length) {
        this.ui.toast('Nothing to clean', 'info');
        return;
      }
      const list = preview.files.slice(0, 25).join('\n') + (preview.files.length > 25 ? `\n… and ${preview.files.length - 25} more` : '');
      const ok = await this.ui.confirm(`Delete ${preview.files.length} item(s)?`, list, 'Delete', true);
      if (ok) await this.ws.run('Cleaning', () => repo.post('/clean', { ...options, dryRun: false }), repo, { success: 'Cleaned' });
    } catch (e) {
      this.ui.toast('Clean failed', 'error', errorMessage(e));
    }
  }

  async checkoutFile(repo: RepoState, sha: string, path: string): Promise<void> {
    const ok = await this.ui.confirm(
      'Checkout file',
      `Replace ${path} in the working tree and index with its version from ${shortSha(sha)}? Local changes to it are lost.`,
      'Checkout file',
      true,
    );
    if (ok) {
      await this.ws.run('Checking out file', () => repo.post('/checkout-file', { sha, path }), repo, {
        success: `${path} restored from ${shortSha(sha)}`,
      });
    }
  }

  pruneRemote(repo: RepoState, name: string): Promise<unknown> {
    return this.ws.run(`Pruning ${name}`, (opId) => repo.post('/remote/prune', { name, opId }), repo, {
      success: `Pruned stale branches of ${name}`,
    });
  }

  updateSubmodules(repo: RepoState): Promise<unknown> {
    return this.ws.run('Updating submodules', (opId) => repo.post('/submodules/update', { opId }), repo, {
      success: 'Submodules updated',
    });
  }

  compare(repo: RepoState, base: string, target: string): void {
    repo.search.set(null);
    repo.view.set('history');
    repo.compare.set({ base, target });
  }

  // ---------------------------------------------------------------- menus

  /**
   * Context menu for a commit. When `focus` is given (a branch/tag that was
   * right-clicked) its actions come first; other refs on the commit get submenus.
   */
  async commitMenu(event: MouseEvent, repo: RepoState, commitOrSha: LogCommit | string, focus?: RefTarget): Promise<void> {
    event.preventDefault();
    event.stopPropagation();
    let commit: LogCommit;
    try {
      commit = typeof commitOrSha === 'string' ? await repo.resolveCommit(commitOrSha) : commitOrSha;
    } catch (e) {
      this.ui.toast('Commit not found', 'error', errorMessage(e));
      return;
    }
    const items: MenuItem[] = [];
    if (focus) {
      items.push({ header: true, label: focus.title }, ...this.refItems(repo, focus), { separator: true });
    }
    const others = this.refsOf(repo, commit).filter((r) => !focus || r.key !== focus.key);
    for (const ref of others) items.push({ label: ref.title, children: this.refItems(repo, ref) });
    if (others.length) items.push({ separator: true });
    items.push({ header: true, label: `Commit ${shortSha(commit.sha)}` }, ...this.commitItems(repo, commit));
    this.ui.openMenu(event, items);
  }

  branchMenu(event: MouseEvent, repo: RepoState, branch: BranchInfo): void {
    void this.commitMenu(event, repo, branch.sha, { kind: 'local', key: 'local:' + branch.name, title: `Branch ${branch.name}`, branch });
  }

  remoteBranchMenu(event: MouseEvent, repo: RepoState, branch: RemoteBranch): void {
    void this.commitMenu(event, repo, branch.sha, { kind: 'remote', key: 'remote:' + branch.name, title: branch.name, remote: branch });
  }

  tagMenu(event: MouseEvent, repo: RepoState, tag: TagInfo): void {
    void this.commitMenu(event, repo, tag.sha, { kind: 'tag', key: 'tag:' + tag.name, title: `Tag ${tag.name}`, tag });
  }

  private refsOf(repo: RepoState, commit: LogCommit): RefTarget[] {
    const refs = repo.refs();
    const result: RefTarget[] = [];
    for (const label of commit.refs) {
      if (label.kind === 'local') {
        const branch = refs?.local.find((b) => b.name === label.name);
        if (branch) result.push({ kind: 'local', key: 'local:' + branch.name, title: `Branch ${branch.name}`, branch });
      } else if (label.kind === 'remote') {
        const remote = refs?.remote.find((b) => b.name === label.name);
        if (remote) result.push({ kind: 'remote', key: 'remote:' + remote.name, title: remote.name, remote });
      } else if (label.kind === 'tag') {
        const tag = refs?.tags.find((t) => t.name === label.name);
        if (tag) result.push({ kind: 'tag', key: 'tag:' + tag.name, title: `Tag ${tag.name}`, tag });
      }
    }
    return result;
  }

  private refItems(repo: RepoState, ref: RefTarget): MenuItem[] {
    const current = repo.currentBranch();
    if (ref.kind === 'local') {
      const branch = ref.branch;
      return [
        { label: `Checkout ${branch.name}`, action: () => void this.checkout(repo, branch.name), disabled: branch.current },
        { label: `Merge ${branch.name} into ${current}…`, action: () => void this.merge(repo, branch.name), disabled: branch.current || !current },
        { label: `Rebase ${current} onto ${branch.name}…`, action: () => void this.rebase(repo, branch.name), disabled: branch.current || !current },
        ...(branch.upstream
          ? [{
              label: `Fast-forward to ${branch.upstream}`,
              action: () => void this.fastForward(repo, branch.name),
              disabled: !branch.behind || branch.ahead > 0,
            }]
          : []),
        { separator: true },
        { label: 'Push…', action: () => void this.push(repo, branch.name) },
        ...(branch.current ? [{ label: 'Pull…', action: () => void this.pull(repo) }] : []),
        { label: 'Set upstream…', action: () => void this.setUpstream(repo, branch) },
        { separator: true },
        { label: 'Rename…', action: () => void this.renameBranch(repo, branch) },
        { label: 'Delete…', action: () => void this.deleteBranch(repo, branch), danger: true, disabled: branch.current },
        { label: 'Copy branch name', action: () => void copyText(branch.name, this.ui) },
      ];
    }
    if (ref.kind === 'remote') {
      const branch = ref.remote;
      const local = repo.refs()?.local.find((b) => b.upstream === branch.name);
      return [
        { label: `Checkout ${branch.branch} (track)`, action: () => void this.checkout(repo, branch.name, 'remote') },
        ...(local
          ? [{
              label: `Fast-forward ${local.name} to here`,
              action: () => void this.fastForward(repo, local.name, branch.name),
              disabled: !local.behind || local.ahead > 0,
            }]
          : []),
        { label: `Merge ${branch.name} into ${current}…`, action: () => void this.merge(repo, branch.name), disabled: !current },
        { label: `Rebase ${current} onto ${branch.name}…`, action: () => void this.rebase(repo, branch.name), disabled: !current },
        { separator: true },
        { label: `Delete from ${branch.remote}…`, action: () => void this.deleteRemoteBranch(repo, branch), danger: true },
        { label: 'Copy branch name', action: () => void copyText(branch.name, this.ui) },
      ];
    }
    const tag = ref.tag;
    return [
      { label: 'Checkout tag…', action: () => void this.checkoutCommit(repo, tag.sha) },
      { label: `Merge ${tag.name} into ${current}…`, action: () => void this.merge(repo, tag.name), disabled: !current },
      { label: 'Push to remote…', action: () => void this.pushTag(repo, tag), disabled: !repo.refs()?.remotes.length },
      { label: 'Delete…', action: () => void this.deleteTag(repo, tag), danger: true },
      { label: 'Copy tag name', action: () => void copyText(tag.name, this.ui) },
    ];
  }

  private commitItems(repo: RepoState, commit: LogCommit): MenuItem[] {
    const current = repo.currentBranch() ?? 'HEAD';
    const head = repo.refs()?.head;
    const isHead = commit.sha === head;
    const selected = repo.selectedSha();
    const resetModes: { mode: ResetMode; label: string; danger?: boolean }[] = [
      { mode: 'soft', label: 'Soft: keep all changes staged' },
      { mode: 'mixed', label: 'Mixed: keep changes, unstaged' },
      { mode: 'keep', label: 'Keep: discard commits, keep local changes' },
      { mode: 'hard', label: 'Hard: discard all changes', danger: true },
    ];
    return [
      { label: 'Checkout commit…', action: () => void this.checkoutCommit(repo, commit.sha), disabled: isHead },
      { label: 'New branch here…', action: () => void this.newBranch(repo, commit.sha) },
      { label: 'New tag here…', action: () => void this.createTag(repo, commit.sha) },
      { separator: true },
      { label: `Merge into ${current}…`, action: () => void this.merge(repo, commit.sha), disabled: isHead },
      { label: `Rebase ${current} onto here…`, action: () => void this.rebase(repo, commit.sha, shortSha(commit.sha)), disabled: isHead },
      { label: 'Interactive rebase from here…', action: () => this.interactiveRebase(repo, commit.sha) },
      { label: `Cherry-pick into ${current}`, action: () => void this.cherryPick(repo, commit), disabled: isHead },
      { label: 'Revert commit…', action: () => void this.revert(repo, commit) },
      {
        label: `Reset ${current} to here`,
        children: resetModes.map((m) => ({ label: m.label, danger: m.danger, action: () => void this.resetTo(repo, commit, m.mode) })),
      },
      ...(isHead && commit.parents.length
        ? [{ label: 'Undo last commit (keep changes)', action: () => void this.undoLastCommit(repo) }]
        : []),
      {
        label: 'Edit commit',
        children: [
          { label: 'Edit message…', action: () => void this.editMessage(repo, commit) },
          { label: 'Squash into parent…', action: () => void this.squashIntoParent(repo, commit, false), disabled: !commit.parents.length },
          { label: 'Fixup into parent (discard message)…', action: () => void this.squashIntoParent(repo, commit, true), disabled: !commit.parents.length },
          { label: 'Drop commit…', action: () => void this.dropCommit(repo, commit), danger: true },
        ],
      },
      { separator: true },
      ...(head && !isHead ? [{ label: 'Compare with HEAD', action: () => this.compare(repo, commit.sha, head) }] : []),
      ...(selected && selected !== commit.sha
        ? [{ label: `Compare with ${shortSha(selected)}`, action: () => this.compare(repo, commit.sha, selected) }]
        : []),
      { label: 'Save as patch…', action: () => void this.savePatch(repo, commit), disabled: commit.parents.length > 1 },
      {
        label: 'Copy',
        children: [
          { label: 'SHA', action: () => void copyText(commit.sha, this.ui) },
          { label: 'Short SHA', action: () => void copyText(shortSha(commit.sha), this.ui) },
          { label: 'Subject', action: () => void copyText(commit.subject, this.ui) },
          { label: 'Author', action: () => void copyText(`${commit.author} <${commit.email}>`, this.ui) },
        ],
      },
    ];
  }

  stashMenu(event: MouseEvent, repo: RepoState, stash: Stash): void {
    this.ui.openMenu(event, [
      { label: 'Apply', action: () => void this.applyStash(repo, stash, false) },
      { label: 'Pop (apply and drop)', action: () => void this.applyStash(repo, stash, true) },
      { label: 'New branch from stash…', action: () => void this.newBranch(repo, `${stash.name}^`, stash.name) },
      { label: 'Drop…', action: () => void this.dropStash(repo, stash), danger: true },
    ]);
  }

  remoteMenu(event: MouseEvent, repo: RepoState, remote: Remote): void {
    this.ui.openMenu(event, [
      { label: `Fetch ${remote.name}`, action: () => void this.fetch(repo, remote.name) },
      { label: 'Prune stale branches', action: () => void this.pruneRemote(repo, remote.name) },
      { label: 'Edit…', action: () => void this.editRemote(repo, remote) },
      { label: 'Remove…', action: () => void this.removeRemote(repo, remote), danger: true },
      { separator: true },
      { label: 'Copy URL', action: () => void copyText(remote.fetchUrl, this.ui) },
    ]);
  }

  /** Repository-wide commands (toolbar "More" button). */
  repositoryMenu(event: MouseEvent, repo: RepoState): void {
    const remotes = repo.refs()?.remotes ?? [];
    this.ui.openMenu(event, [
      { label: 'Apply patch…', action: () => void this.applyPatch(repo) },
      { label: 'Clean untracked files…', action: () => void this.clean(repo), danger: true },
      { label: 'Reflog (recover lost commits)…', action: () => this.ui.open({ kind: 'reflog', repo }) },
      { separator: true },
      { label: 'Fetch all remotes (incl. tags)', action: () => void this.ws.run('Fetching', (opId) => repo.post('/fetch', { prune: true, tags: true, opId }), repo, { success: 'Fetch complete' }), disabled: !remotes.length },
      { label: 'Add remote…', action: () => void this.addRemote(repo) },
      ...(repo.refs()?.submodules.length ? [{ label: 'Update submodules', action: () => void this.updateSubmodules(repo) }] : []),
      { separator: true },
      { label: 'Refresh', action: () => void repo.refreshAll() },
      { label: 'Copy repository path', action: () => void copyText(repo.path, this.ui) },
      ...(desktop ? [{ label: revealLabel(), action: () => void desktop!.reveal(repo.path) }] : []),
    ]);
  }

  /** Menu entries shared by every file list (history, blame, copy). */
  fileMenuItems(repo: RepoState, path: string, rev: string | null = null): MenuItem[] {
    return [
      { label: 'File history', action: () => this.ui.open({ kind: 'file-history', repo, path }) },
      { label: 'Blame', action: () => this.ui.open({ kind: 'blame', repo, path, rev }) },
      ...(rev ? [{ label: 'Checkout this version of the file…', action: () => void this.checkoutFile(repo, rev, path) }] : []),
      { separator: true },
      ...(desktop
        ? [
            { label: 'Open file', action: () => void desktop!.openPath(joinPath(repo.path, path)) },
            { label: revealLabel(), action: () => void desktop!.reveal(joinPath(repo.path, path)) },
          ]
        : []),
      { label: 'Copy path', action: () => void copyText(path, this.ui) },
      { label: 'Copy full path', action: () => void copyText(joinPath(repo.path, path), this.ui) },
    ];
  }
}

export type ResetMode = 'soft' | 'mixed' | 'keep' | 'hard';

export type RefTarget =
  | { kind: 'local'; key: string; title: string; branch: BranchInfo }
  | { kind: 'remote'; key: string; title: string; remote: RemoteBranch }
  | { kind: 'tag'; key: string; title: string; tag: TagInfo };

function download(filename: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/x-patch' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = accept;
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.oncancel = () => resolve(null);
    input.click();
  });
}

export function revealLabel(): string {
  const platform = desktop?.platform;
  return platform === 'darwin' ? 'Reveal in Finder' : platform === 'win32' ? 'Show in Explorer' : 'Show in file manager';
}

export function joinPath(root: string, path: string): string {
  const separator = root.includes('\\') && !root.includes('/') ? '\\' : '/';
  return root.replace(/[\\/]+$/, '') + separator + path;
}

function remoteField(remotes: Remote[], preferred: string | null) {
  return {
    key: 'remote',
    label: 'Remote',
    type: 'select' as const,
    value: preferred ?? remotes.find((r) => r.name === 'origin')?.name ?? remotes[0]?.name ?? '',
    options: remotes.map((r) => ({ value: r.name, label: `${r.name}  (${r.fetchUrl})` })),
  };
}

function splitUpstream(upstream: string | null, remotes: Remote[]): [string | null, string | null] {
  if (!upstream) return [null, null];
  const remote = [...remotes].sort((a, b) => b.name.length - a.name.length).find((r) => upstream.startsWith(r.name + '/'));
  if (!remote) return [null, upstream];
  return [remote.name, upstream.slice(remote.name.length + 1)];
}
