import { ChangeDetectionStrategy, Component, ElementRef, computed, effect, inject, signal, viewChild } from '@angular/core';
import { MenuItem, Ui } from '../core/ui';

@Component({
  selector: 'app-context-menu',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    '(document:mousedown)': 'outside($event)',
    '(document:keydown.escape)': 'close()',
    '(window:blur)': 'close()',
    '(window:resize)': 'close()',
  },
  template: `
    @if (ui.menu(); as menu) {
      <div #menu class="menu" role="menu" [class.flip]="flipSubmenus()" [style.left.px]="position().x"
        [style.top.px]="position().y" (contextmenu)="$event.preventDefault()">
        @for (item of menu.items; track $index; let i = $index) {
          @if (item.separator) {
            <div class="sep"></div>
          } @else if (item.header) {
            <div class="header">{{ item.label }}</div>
          } @else {
            <div class="entry" (mouseenter)="openSub.set(item.children ? i : null)">
              <button role="menuitem" [disabled]="item.disabled" [class.danger]="item.danger"
                [class.open]="openSub() === i" (click)="run(item, i)">
                <span class="label">{{ item.label }}</span>
                @if (item.children) { <span class="arrow">›</span> }
              </button>
              @if (item.children && openSub() === i) {
                <div class="menu sub" role="menu">
                  @for (child of item.children; track $index) {
                    @if (child.separator) {
                      <div class="sep"></div>
                    } @else if (child.header) {
                      <div class="header">{{ child.label }}</div>
                    } @else {
                      <button role="menuitem" [disabled]="child.disabled" [class.danger]="child.danger"
                        (click)="run(child, -1)"><span class="label">{{ child.label }}</span></button>
                    }
                  }
                </div>
              }
            </div>
          }
        }
      </div>
    }
  `,
  styles: `
    .menu { position: fixed; z-index: 950; min-width: 210px; max-width: 380px; padding: 4px; background: var(--panel);
      border: 1px solid var(--border); border-radius: 8px; box-shadow: var(--shadow); }
    .menu.sub { position: absolute; left: calc(100% - 2px); top: -5px; }
    .menu.flip .menu.sub { left: auto; right: calc(100% - 2px); }
    .entry { position: relative; }
    button { display: flex; align-items: center; gap: 12px; width: 100%; text-align: left; padding: 5px 12px; border: 0;
      border-radius: 5px; background: none; color: var(--text); font: inherit; cursor: default; white-space: nowrap; }
    .label { flex: 1; overflow: hidden; text-overflow: ellipsis; }
    .arrow { color: var(--text-muted); font-size: 15px; line-height: 1; }
    button:hover:not(:disabled), button.open { background: var(--accent); color: #fff; }
    button:hover:not(:disabled) .arrow, button.open .arrow { color: #fff; }
    button:disabled { color: var(--text-faint); }
    button.danger { color: var(--danger); }
    button.danger:hover:not(:disabled) { background: var(--danger); color: #fff; }
    .sep { height: 1px; margin: 4px 6px; background: var(--border); }
    .header { padding: 4px 12px 2px; font-size: 11px; font-weight: 700; color: var(--text-muted); text-transform: uppercase;
      letter-spacing: .04em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  `,
})
export class ContextMenu {
  protected readonly ui = inject(Ui);
  private readonly menuEl = viewChild<ElementRef<HTMLElement>>('menu');
  private readonly size = signal({ w: 0, h: 0 });
  protected readonly openSub = signal<number | null>(null);

  protected readonly position = computed(() => {
    const menu = this.ui.menu();
    const { w, h } = this.size();
    if (!menu) return { x: 0, y: 0 };
    return {
      x: Math.max(4, Math.min(menu.x, innerWidth - w - 4)),
      y: Math.max(4, menu.y + h > innerHeight - 4 ? Math.max(4, innerHeight - h - 4) : menu.y),
    };
  });
  /** Open submenus to the left when there is no room on the right. */
  protected readonly flipSubmenus = computed(() => this.position().x + this.size().w * 2 + 8 > innerWidth);

  constructor() {
    effect(() => {
      const el = this.menuEl()?.nativeElement;
      this.openSub.set(null);
      if (el) queueMicrotask(() => this.size.set({ w: el.offsetWidth, h: el.offsetHeight }));
      else this.size.set({ w: 0, h: 0 });
    });
  }

  protected outside(event: MouseEvent): void {
    if (!this.menuEl()?.nativeElement.contains(event.target as Node)) this.close();
  }

  protected close(): void {
    this.ui.menu.set(null);
  }

  protected run(item: MenuItem, index: number): void {
    if (item.children) {
      this.openSub.set(index);
      return;
    }
    this.close();
    item.action?.();
  }
}
