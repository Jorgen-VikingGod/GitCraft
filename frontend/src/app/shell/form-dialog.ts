import { ChangeDetectionStrategy, Component, ElementRef, afterNextRender, effect, inject, signal, viewChild } from '@angular/core';
import { FormField, FormValues, Ui } from '../core/ui';
import { Select } from '../shared/select';

/** Generic modal used for prompts, confirmations and small option forms. */
@Component({
  selector: 'app-form-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Select],
  template: `
    @if (ui.form(); as current) {
      <div class="backdrop" (mousedown)="$event.target === $event.currentTarget && cancel()">
        <form #form class="dialog" (submit)="$event.preventDefault(); submit()" (keydown.escape)="cancel()">
          <h2>{{ current.spec.title }}</h2>
          @if (current.spec.message) {
            <p class="message">{{ current.spec.message }}</p>
          }
          @for (field of current.spec.fields ?? []; track field.key) {
            @switch (field.type) {
              @case ('checkbox') {
                <label class="check">
                  <input type="checkbox" [checked]="!!values()[field.key]" (change)="set(field, $any($event.target).checked)" />
                  {{ field.label }}
                </label>
              }
              @case ('select') {
                <label class="field">
                  <span>{{ field.label }}</span>
                  <app-select [options]="field.options ?? []" [value]="$any(values()[field.key])"
                    (valueChange)="set(field, $event)" />
                </label>
              }
              @case ('textarea') {
                <label class="field">
                  <span>{{ field.label }}</span>
                  <textarea rows="4" [placeholder]="field.placeholder ?? ''" [value]="values()[field.key] ?? ''"
                    (input)="set(field, $any($event.target).value)"></textarea>
                </label>
              }
              @default {
                <label class="field">
                  <span>{{ field.label }}</span>
                  <input [type]="field.type === 'number' ? 'number' : 'text'" [placeholder]="field.placeholder ?? ''"
                    [value]="values()[field.key] ?? ''" (input)="set(field, $any($event.target).value)"
                    spellcheck="false" autocomplete="off" />
                </label>
              }
            }
            @if (field.hint) {
              <div class="hint">{{ field.hint }}</div>
            }
          }
          <div class="buttons">
            <button type="button" class="btn" (click)="cancel()">{{ current.spec.cancel ?? 'Cancel' }}</button>
            <button type="submit" class="btn" [class.primary]="!current.spec.danger" [class.danger]="current.spec.danger"
              [disabled]="!valid()">{{ current.spec.ok ?? 'OK' }}</button>
          </div>
        </form>
      </div>
    }
  `,
  styles: `
    .backdrop { position: fixed; inset: 0; z-index: 900; background: var(--backdrop); display: grid; place-items: start center;
      padding-top: 12vh; }
    .dialog { width: min(480px, calc(100vw - 32px)); background: var(--panel); border: 1px solid var(--border);
      border-radius: 10px; box-shadow: var(--shadow); padding: 18px 20px; display: flex; flex-direction: column; gap: 12px; }
    h2 { margin: 0; font-size: 16px; }
    .message { margin: 0; color: var(--text-muted); word-break: break-word; white-space: pre-wrap; }
    .field { display: flex; flex-direction: column; gap: 4px; }
    .field span { font-size: 12px; font-weight: 600; color: var(--text-muted); }
    .check { display: flex; align-items: center; gap: 8px; }
    .hint { font-size: 12px; color: var(--text-muted); margin-top: -6px; }
    .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }
  `,
})
export class FormDialog {
  protected readonly ui = inject(Ui);
  protected readonly values = signal<FormValues>({});
  private readonly form = viewChild<ElementRef<HTMLFormElement>>('form');

  constructor() {
    effect(() => {
      const current = this.ui.form();
      if (!current) return;
      const initial: FormValues = {};
      for (const field of current.spec.fields ?? []) {
        initial[field.key] = field.value ?? (field.type === 'checkbox' ? false : '');
      }
      this.values.set(initial);
      setTimeout(() => this.focusFirst());
    });
    afterNextRender(() => this.focusFirst());
  }

  private focusFirst(): void {
    const form = this.form()?.nativeElement;
    const target = form?.querySelector<HTMLElement>('input:not([type=checkbox]), textarea, app-select button') ??
      form?.querySelector<HTMLElement>('button[type=submit]');
    target?.focus();
  }

  protected set(field: FormField, value: string | boolean): void {
    this.values.update((v) => ({ ...v, [field.key]: field.type === 'number' ? Number(value) : value }));
  }

  protected valid(): boolean {
    const spec = this.ui.form()?.spec;
    return (spec?.fields ?? []).every((f) => !f.required || String(this.values()[f.key] ?? '').trim() !== '');
  }

  protected submit(): void {
    if (this.valid()) this.ui.closeForm(this.values());
  }

  protected cancel(): void {
    this.ui.closeForm(null);
  }
}
