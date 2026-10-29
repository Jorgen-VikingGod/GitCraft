import { ChangeDetectionStrategy, Component, input, model } from '@angular/core';
import { writeStorage } from '../core/settings';

/**
 * Drag handle that resizes the pane before it (or after it, with `invert`).
 * `axis="x"` resizes width, `axis="y"` height.
 */
@Component({
  selector: 'app-splitter',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '',
  host: {
    '[class.x]': "axis() === 'x'",
    '[class.y]': "axis() === 'y'",
    '(mousedown)': 'start($event)',
    '(dblclick)': 'size.set(defaultSize())',
  },
  styles: `
    :host { flex: none; position: relative; z-index: 3; background: var(--border); }
    :host.x { width: 1px; cursor: col-resize; }
    :host.y { height: 1px; cursor: row-resize; }
    :host::after { content: ''; position: absolute; inset: -3px; }
    :host:hover, :host.dragging { background: var(--accent); }
  `,
})
export class Splitter {
  readonly axis = input<'x' | 'y'>('x');
  readonly size = model.required<number>();
  readonly min = input(120);
  readonly max = input(2000);
  readonly invert = input(false);
  readonly defaultSize = input(300);
  /** localStorage key to remember the size under. */
  readonly storageKey = input<string | null>(null);

  protected start(event: MouseEvent): void {
    if (event.button !== 0) return;
    event.preventDefault();
    const handle = event.currentTarget as HTMLElement;
    handle.classList.add('dragging');
    const origin = this.axis() === 'x' ? event.clientX : event.clientY;
    const initial = this.size();
    const sign = this.invert() ? -1 : 1;
    const move = (e: MouseEvent) => {
      const delta = (this.axis() === 'x' ? e.clientX : e.clientY) - origin;
      this.size.set(Math.max(this.min(), Math.min(this.max(), initial + sign * delta)));
    };
    const up = () => {
      handle.classList.remove('dragging');
      document.removeEventListener('mousemove', move);
      document.removeEventListener('mouseup', up);
      document.body.style.cursor = '';
      const key = this.storageKey();
      if (key) writeStorage(key, this.size());
    };
    document.body.style.cursor = this.axis() === 'x' ? 'col-resize' : 'row-resize';
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
  }
}
