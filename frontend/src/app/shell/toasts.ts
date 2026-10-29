import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Ui } from '../core/ui';
import { Icon } from '../shared/icon';

@Component({
  selector: 'app-toasts',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (toast of ui.toasts(); track toast.id) {
      <div class="toast" [attr.data-kind]="toast.kind" role="status">
        <div class="body">
          <div class="message">{{ toast.message }}</div>
          @if (toast.detail) {
            <pre class="detail">{{ toast.detail }}</pre>
          }
        </div>
        <button class="icon-btn" (click)="ui.dismiss(toast.id)" title="Dismiss"><app-icon name="x" [size]="13" /></button>
      </div>
    }
  `,
  styles: `
    :host { position: fixed; left: 16px; bottom: 36px; z-index: 1000; display: flex; flex-direction: column; gap: 8px;
      width: min(420px, calc(100vw - 32px)); pointer-events: none; }
    .toast { pointer-events: auto; display: flex; gap: 8px; align-items: flex-start; padding: 10px 10px 10px 14px;
      border-radius: 8px; background: var(--panel); border: 1px solid var(--border); border-left: 4px solid var(--accent);
      box-shadow: var(--shadow); animation: slide .15s ease-out; }
    .toast[data-kind='success'] { border-left-color: var(--success); }
    .toast[data-kind='warning'] { border-left-color: var(--warning); }
    .toast[data-kind='error'] { border-left-color: var(--danger); }
    .body { flex: 1; min-width: 0; }
    .message { font-weight: 600; }
    .detail { margin: 6px 0 0; max-height: 160px; overflow: auto; font-family: var(--mono); font-size: 11px;
      white-space: pre-wrap; word-break: break-word; color: var(--text-muted); }
    @keyframes slide { from { transform: translateY(8px); opacity: 0; } }
  `,
})
export class Toasts {
  protected readonly ui = inject(Ui);
}
