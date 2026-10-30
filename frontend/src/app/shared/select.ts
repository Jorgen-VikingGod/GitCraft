import { ChangeDetectionStrategy, Component, ElementRef, booleanAttribute, computed, inject, input, output, signal } from '@angular/core';
import { Icon } from './icon';

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * Drop-down list rendered inside the page. Native <select> popups are separate
 * windows on some platforms (e.g. Electron on Wayland), where they may not open.
 */
@Component({
  selector: 'app-select',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  host: {
    '[class.plain]': 'plain()',
    '(document:mousedown)': 'outside($event)',
    '(window:blur)': 'close()',
    '(window:resize)': 'close()',
  },
  template: `
    <button type="button" class="trigger" role="combobox" aria-haspopup="listbox" [attr.aria-expanded]="open()"
      [attr.title]="title() || null" [disabled]="disabled()" (click)="toggle()" (keydown)="key($event)">
      <span class="label">{{ current()?.label ?? '' }}</span>
      <app-icon name="chevron-down" [size]="14" />
    </button>
    @if (open()) {
      <div class="list" role="listbox" [style.left.px]="pos().x" [style.top.px]="pos().y" [style.min-width.px]="pos().w"
        (mousedown)="$event.preventDefault()">
        @for (option of options(); track option.value; let i = $index) {
          <div class="option" role="option" [attr.aria-selected]="option.value === value()"
            [class.active]="i === active()" [class.selected]="option.value === value()"
            (mouseenter)="active.set(i)" (click)="$event.preventDefault(); pick(option)">
            <span class="check">@if (option.value === value()) { <app-icon name="check" [size]="13" /> }</span>
            {{ option.label }}
          </div>
        }
      </div>
    }
  `,
  styles: `
    :host { display: inline-block; min-width: 0; }
    .trigger { display: flex; align-items: center; gap: 6px; width: 100%; min-height: 28px; padding: 3px 6px 3px 8px;
      font: inherit; line-height: 1.3; text-align: left; color: var(--text); background: var(--input);
      border: 1px solid var(--border); border-radius: 6px; outline: none; cursor: default; }
    .trigger:focus-visible, .trigger[aria-expanded='true'] { border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
    .trigger:disabled { opacity: .5; }
    .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    app-icon { flex: none; color: var(--text-muted); }
    :host(.plain) .trigger { min-height: 26px; border: 0; background: none; box-shadow: none; color: var(--text-muted);
      font-size: 12px; }
    .list { position: fixed; z-index: 1000; max-height: 280px; overflow-y: auto; padding: 4px; background: var(--panel);
      border: 1px solid var(--border); border-radius: 8px; box-shadow: var(--shadow); }
    .option { display: flex; align-items: center; gap: 4px; padding: 5px 10px 5px 4px; border-radius: 5px;
      white-space: nowrap; color: var(--text); cursor: default; }
    .option.active { background: var(--accent); color: #fff; }
    .check { display: inline-flex; width: 16px; justify-content: center; }
    .check app-icon { color: inherit; }
  `,
})
export class Select {
  readonly options = input.required<SelectOption[]>();
  readonly value = input<string | null | undefined>(null);
  readonly title = input('');
  readonly disabled = input(false, { transform: booleanAttribute });
  /** Borderless look, for toolbars. */
  readonly plain = input(false, { transform: booleanAttribute });
  readonly valueChange = output<string>();

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly open = signal(false);
  protected readonly active = signal(0);
  protected readonly pos = signal({ x: 0, y: 0, w: 0 });

  protected readonly current = computed(() => this.options().find((o) => o.value === this.value()) ?? null);
  private readonly index = computed(() => Math.max(0, this.options().findIndex((o) => o.value === this.value())));

  protected toggle(): void {
    if (this.open()) {
      this.close();
      return;
    }
    const rect = this.host.nativeElement.getBoundingClientRect();
    const height = Math.min(280, this.options().length * 28 + 10);
    const below = rect.bottom + 4 + height <= innerHeight;
    this.pos.set({ x: rect.left, y: below ? rect.bottom + 4 : Math.max(4, rect.top - 4 - height), w: rect.width });
    this.active.set(this.index());
    this.open.set(true);
  }

  close(): void {
    this.open.set(false);
  }

  protected pick(option: SelectOption): void {
    this.close();
    if (option.value !== this.value()) this.valueChange.emit(option.value);
  }

  protected key(event: KeyboardEvent): void {
    const count = this.options().length;
    if (!count) return;
    const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
    if (!this.open()) {
      if ((step && event.altKey) || event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        this.toggle();
      } else if (step) {
        // Like a native select: arrows change the value directly.
        event.preventDefault();
        this.pick(this.options()[Math.min(count - 1, Math.max(0, this.index() + step))]);
      }
      return;
    }
    if (step) {
      event.preventDefault();
      this.active.update((i) => (i + step + count) % count);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      this.pick(this.options()[this.active()]);
    } else if (event.key === 'Escape') {
      // Close only the list, not the dialog around it.
      event.preventDefault();
      event.stopPropagation();
      this.close();
    } else if (event.key === 'Tab') {
      this.close();
    }
  }

  protected outside(event: MouseEvent): void {
    if (this.open() && !this.host.nativeElement.contains(event.target as Node)) this.close();
  }
}
