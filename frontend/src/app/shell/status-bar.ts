import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Events } from '../core/events';
import { Workspace } from '../core/workspace';

@Component({
  selector: 'app-status-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (ws.busy(); as busy) {
      <span class="spinner"></span>
      <span class="busy">{{ busy }}…</span>
      <span class="progress">{{ ws.progressLine() }}</span>
    } @else {
      <span class="muted">{{ ws.active()?.path ?? 'No repository open' }}</span>
    }
    <span class="spacer"></span>
    @if (ws.active(); as repo) {
      <span class="muted">{{ repo.commits().length }}{{ repo.hasMore() ? '+' : '' }} commits</span>
    }
    <span class="conn" [class.on]="events.connected()" [title]="events.connected() ? 'Live updates connected' : 'Reconnecting…'"></span>
  `,
  styles: `
    :host { display: flex; align-items: center; gap: 10px; height: 24px; padding: 0 12px; font-size: 12px;
      background: var(--titlebar); border-top: 1px solid var(--border); white-space: nowrap; overflow: hidden; }
    .muted { color: var(--text-muted); overflow: hidden; text-overflow: ellipsis; }
    .busy { font-weight: 600; }
    .progress { color: var(--text-muted); font-family: var(--mono); overflow: hidden; text-overflow: ellipsis; }
    .conn { width: 8px; height: 8px; border-radius: 50%; background: var(--warning); flex: none; }
    .conn.on { background: var(--success); }
    .spinner { width: 12px; height: 12px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%;
      animation: spin .8s linear infinite; flex: none; }
    @keyframes spin { to { transform: rotate(360deg); } }
  `,
})
export class StatusBar {
  protected readonly ws = inject(Workspace);
  protected readonly events = inject(Events);
}
