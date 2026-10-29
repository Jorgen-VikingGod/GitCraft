import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { Api } from './core/api';
import { readStorageValue } from './core/settings';
import { Overlay, Ui } from './core/ui';
import { Workspace } from './core/workspace';
import { ChangesView } from './changes/changes-view';
import { ConflictResolver } from './conflicts/conflict-resolver';
import { OpenRepo } from './dialogs/open-repo';
import { SettingsDialog } from './dialogs/settings-dialog';
import { HistoryView } from './history/history-view';
import { StashView } from './history/stash-view';
import { BlameView } from './inspect/blame-view';
import { FileHistory } from './inspect/file-history';
import { ReflogView } from './inspect/reflog-view';
import { RebaseDialog } from './rebase/rebase-dialog';
import { Splitter } from './shared/splitter';
import { ContextMenu } from './shell/context-menu';
import { FormDialog } from './shell/form-dialog';
import { OperationBanner } from './shell/operation-banner';
import { StatusBar } from './shell/status-bar';
import { TabBar } from './shell/tab-bar';
import { Toasts } from './shell/toasts';
import { Toolbar } from './shell/toolbar';
import { Sidebar } from './sidebar/sidebar';

@Component({
  selector: 'app-root',
  imports: [
    TabBar,
    Toolbar,
    Sidebar,
    Splitter,
    HistoryView,
    ChangesView,
    StashView,
    OperationBanner,
    StatusBar,
    Toasts,
    FormDialog,
    ContextMenu,
    OpenRepo,
    SettingsDialog,
    RebaseDialog,
    ConflictResolver,
    FileHistory,
    BlameView,
    ReflogView,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown)': 'shortcut($event)' },
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  protected readonly ws = inject(Workspace);
  protected readonly ui = inject(Ui);
  protected readonly api = inject(Api);
  protected readonly sidebarWidth = signal(readStorageValue('gitcraft-sidebar-width', 250));
  protected readonly repo = computed(() => this.ws.active());
  private readonly toolbar = viewChild(Toolbar);

  /** The current overlay, if it is of the given kind (typed for the template). */
  protected overlay<K extends Overlay['kind']>(kind: K): Extract<Overlay, { kind: K }> | null {
    const overlay = this.ui.overlay();
    return overlay?.kind === kind ? (overlay as Extract<Overlay, { kind: K }>) : null;
  }

  ngOnInit(): void {
    void this.ws.start();
  }

  protected shortcut(event: KeyboardEvent): void {
    const mod = event.ctrlKey || event.metaKey;
    const repo = this.repo();
    if (event.key === 'F5' || (mod && event.key.toLowerCase() === 'r' && !event.shiftKey)) {
      event.preventDefault();
      void repo?.refreshAll();
      return;
    }
    if (!mod || this.ui.form() || (this.ui.overlay() && this.ui.overlay()?.kind !== 'open')) return;
    const key = event.key.toLowerCase();
    if (key === 't') {
      event.preventDefault();
      this.ui.open({ kind: 'open' });
    } else if (repo && key === '1') {
      event.preventDefault();
      repo.view.set('history');
    } else if (repo && key === '2') {
      event.preventDefault();
      repo.view.set('changes');
    } else if (repo && key === 'f' && !event.shiftKey) {
      event.preventDefault();
      this.toolbar()?.focusSearch();
    }
  }
}
