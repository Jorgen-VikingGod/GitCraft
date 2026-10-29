import { NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { Api, errorMessage } from '../core/api';
import { desktop } from '../core/desktop';
import { DirListing, RecentRepo } from '../core/models';
import { Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { shortDate } from '../shared/format';
import { Icon } from '../shared/icon';

type Mode = 'recent' | 'browse' | 'clone' | 'init';

@Component({
  selector: 'app-open-repo',
  imports: [Icon, NgTemplateOutlet],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './open-repo.html',
  styleUrl: './open-repo.scss',
})
export class OpenRepo implements OnInit {
  protected readonly ws = inject(Workspace);
  private readonly api = inject(Api);
  protected readonly ui = inject(Ui);

  protected readonly mode = signal<Mode>('recent');
  protected readonly listing = signal<DirListing | null>(null);
  protected readonly pathInput = signal('');
  protected readonly showHidden = signal(false);
  protected readonly working = signal(false);
  protected readonly cloneUrl = signal('');
  protected readonly cloneParent = signal('');
  protected readonly cloneName = signal('');
  protected readonly initPath = signal('');
  protected readonly filter = signal('');
  protected readonly date = shortDate;
  protected readonly desktop = desktop;

  protected readonly recent = computed(() => {
    const f = this.filter().toLowerCase();
    return this.ws.recent().filter((r) => !f || r.path.toLowerCase().includes(f));
  });

  protected readonly suggestedName = computed(() => {
    const url = this.cloneUrl().trim().replace(/\/+$/, '');
    return url.split(/[/:]/).pop()?.replace(/\.git$/, '') ?? '';
  });

  ngOnInit(): void {
    void this.ws.loadRecent().then(() => {
      if (!this.ws.recent().length) this.setMode('browse');
    });
  }

  protected setMode(mode: Mode): void {
    this.mode.set(mode);
    if ((mode === 'browse' || mode === 'clone' || mode === 'init') && !this.listing()) void this.browse(null);
  }

  protected async browse(path: string | null): Promise<void> {
    try {
      const listing = await this.api.get<DirListing>('/fs', { path, hidden: this.showHidden() });
      this.listing.set(listing);
      this.pathInput.set(listing.path);
      if (!this.cloneParent()) this.cloneParent.set(listing.path);
      if (!this.initPath()) this.initPath.set(listing.path);
    } catch (e) {
      this.ui.toast(errorMessage(e), 'error');
    }
  }

  protected toggleHidden(): void {
    this.showHidden.update((v) => !v);
    void this.browse(this.listing()?.path ?? null);
  }

  protected async open(path: string): Promise<void> {
    this.working.set(true);
    const state = await this.ws.open(path);
    this.working.set(false);
    if (state) this.ui.closeOverlay();
  }

  protected async forget(event: MouseEvent, repo: RecentRepo): Promise<void> {
    event.stopPropagation();
    await this.api.post('/repos/forget', { path: repo.path });
    await this.ws.loadRecent();
  }

  protected useForClone(): void {
    const path = this.listing()?.path;
    if (path) this.cloneParent.set(path);
  }

  protected async clone(): Promise<void> {
    const url = this.cloneUrl().trim();
    if (!url || !this.cloneParent().trim()) return;
    this.working.set(true);
    const entry = await this.ws.run(
      'Cloning',
      (opId) =>
        this.api.post<RecentRepo>('/repos/clone', {
          url,
          parent: this.cloneParent().trim(),
          name: this.cloneName().trim() || null,
          opId,
        }),
      null,
      { success: 'Clone complete' },
    );
    this.working.set(false);
    if (entry) {
      this.ws.addTab(entry.path);
      this.ui.closeOverlay();
    }
  }

  protected async init(): Promise<void> {
    const path = this.initPath().trim();
    if (!path) return;
    this.working.set(true);
    const entry = await this.ws.run('Creating repository', () => this.api.post<RecentRepo>('/repos/init', { path }), null);
    this.working.set(false);
    if (entry) {
      this.ws.addTab(entry.path);
      this.ui.closeOverlay();
    }
  }

  /** Native folder dialog (desktop app only). */
  protected async choose(target: 'open' | 'clone' | 'init'): Promise<void> {
    const defaultPath = { open: this.pathInput(), clone: this.cloneParent(), init: this.initPath() }[target];
    const title = { open: 'Open repository', clone: 'Clone into folder', init: 'New repository folder' }[target];
    const path = await desktop?.pickFolder({ title, defaultPath: defaultPath || undefined });
    if (!path) return;
    if (target === 'open') await this.open(path);
    else if (target === 'clone') this.cloneParent.set(path);
    else this.initPath.set(path);
  }

  protected canClose(): boolean {
    return this.ws.tabs().length > 0;
  }
}
