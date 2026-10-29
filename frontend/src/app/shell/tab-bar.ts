import { CdkDrag, CdkDragDrop, CdkDropList } from '@angular/cdk/drag-drop';
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { revealLabel } from '../core/actions';
import { desktop } from '../core/desktop';
import { copyText, Ui } from '../core/ui';
import { Workspace } from '../core/workspace';
import { Icon } from '../shared/icon';

@Component({
  selector: 'app-tab-bar',
  imports: [Icon, CdkDropList, CdkDrag],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="brand" title="GitCraft"><app-icon name="merge" [size]="16" /> GitCraft</div>
    <div class="tabs" cdkDropList cdkDropListOrientation="horizontal" (cdkDropListDropped)="drop($event)">
      @for (tab of ws.tabs(); track tab.path; let i = $index) {
        <div class="tab" cdkDrag cdkDragLockAxis="x" [class.active]="i === ws.activeIndex()" [title]="tab.path"
          (mousedown)="ws.activate(i)" (auxclick)="$event.button === 1 && ws.close(i)" (contextmenu)="menu($event, i)">
          <app-icon name="repo" [size]="13" />
          <span class="name">{{ tab.name }}</span>
          @if (tab.currentBranch(); as branch) {
            <span class="branch">{{ branch }}</span>
          }
          <button class="close" title="Close tab" (mousedown)="$event.stopPropagation()" (click)="ws.close(i)">
            <app-icon name="x" [size]="12" />
          </button>
        </div>
      }
    </div>
    <button class="icon-btn add" title="Open repository (Ctrl+T)" (click)="ui.open({ kind: 'open' })">
      <app-icon name="plus" [size]="16" />
    </button>
    <span class="spacer"></span>
    <button class="icon-btn" title="Settings" (click)="ui.open({ kind: 'settings' })">
      <app-icon name="settings" [size]="16" />
    </button>
  `,
  styles: `
    :host { display: flex; align-items: stretch; height: 36px; background: var(--titlebar); border-bottom: 1px solid var(--border);
      padding-right: 6px; gap: 2px; user-select: none; }
    .brand { display: flex; align-items: center; gap: 6px; padding: 0 12px; font-weight: 700; color: var(--accent); }
    .tabs { display: flex; min-width: 0; overflow-x: auto; scrollbar-width: none; }
    .tab { display: flex; align-items: center; gap: 6px; padding: 0 6px 0 12px; max-width: 260px; min-width: 120px;
      border-right: 1px solid var(--border); color: var(--text-muted); cursor: default; background: var(--titlebar); }
    .tab:hover { background: var(--hover); }
    .tab.active { background: var(--bg); color: var(--text); box-shadow: inset 0 2px 0 var(--accent); }
    .name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .branch { font-size: 11px; color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
    .close { display: grid; place-items: center; width: 20px; height: 20px; border: 0; border-radius: 4px; background: none;
      color: inherit; cursor: pointer; visibility: hidden; margin-left: auto; flex: none; }
    .tab:hover .close, .tab.active .close { visibility: visible; }
    .close:hover { background: var(--hover-strong); }
    .add, :host > .icon-btn { align-self: center; }
    .cdk-drag-preview { box-shadow: var(--shadow); }
    .cdk-drag-placeholder { opacity: .4; }
  `,
})
export class TabBar {
  protected readonly ws = inject(Workspace);
  protected readonly ui = inject(Ui);

  protected drop(event: CdkDragDrop<unknown>): void {
    if (event.previousIndex !== event.currentIndex) this.ws.move(event.previousIndex, event.currentIndex);
  }

  protected menu(event: MouseEvent, index: number): void {
    const tab = this.ws.tabs()[index];
    this.ui.openMenu(event, [
      { label: 'Close', action: () => this.ws.close(index) },
      {
        label: 'Close others',
        action: () => {
          for (let i = this.ws.tabs().length - 1; i >= 0; i--) if (this.ws.tabs()[i] !== tab) this.ws.close(i);
        },
        disabled: this.ws.tabs().length < 2,
      },
      { separator: true },
      { label: 'Refresh', action: () => void tab.refreshAll() },
      { label: 'Copy path', action: () => void copyText(tab.path, this.ui) },
      ...(desktop ? [{ label: revealLabel(), action: () => void desktop!.reveal(tab.path) }] : []),
    ]);
  }
}
