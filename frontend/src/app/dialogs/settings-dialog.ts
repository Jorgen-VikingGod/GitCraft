import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Settings, SettingsStore } from '../core/settings';
import { Ui } from '../core/ui';
import { Icon } from '../shared/icon';
import { Select } from '../shared/select';

@Component({
  selector: 'app-settings-dialog',
  imports: [Icon, Select],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'ui.closeOverlay()' },
  template: `
    @let s = settings.value();
    <div class="dialog">
      <header>
        <h2>Settings</h2>
        <span class="spacer"></span>
        <button class="icon-btn" (click)="ui.closeOverlay()" title="Close"><app-icon name="x" [size]="16" /></button>
      </header>
      <section>
        <h3>Appearance</h3>
        <label class="row"><span>Theme</span>
          <app-select [options]="themes" [value]="s.theme" (valueChange)="set('theme', $any($event))" />
        </label>
      </section>
      <section>
        <h3>Diffs</h3>
        <label class="row"><span>Layout</span>
          <app-select [options]="diffModes" [value]="s.diffMode" (valueChange)="set('diffMode', $any($event))" />
        </label>
        <label class="row"><span>Context lines</span>
          <input type="number" min="0" max="100" [value]="s.diffContext"
            (change)="set('diffContext', clamp($any($event.target).value, 0, 100))" />
        </label>
        <label class="row check"><input type="checkbox" [checked]="s.ignoreWhitespace"
          (change)="set('ignoreWhitespace', $any($event.target).checked)" /> Ignore whitespace changes</label>
        <label class="row check"><input type="checkbox" [checked]="s.syntaxHighlight"
          (change)="set('syntaxHighlight', $any($event.target).checked)" /> Syntax highlighting</label>
      </section>
      <section>
        <h3>Repository</h3>
        <label class="row"><span>Commits loaded at a time</span>
          <input type="number" min="100" max="100000" step="100" [value]="s.logLimit"
            (change)="set('logLimit', clamp($any($event.target).value, 100, 100000))" />
        </label>
        <label class="row check"><input type="checkbox" [checked]="s.fetchOnOpen"
          (change)="set('fetchOnOpen', $any($event.target).checked)" /> Fetch when opening a repository</label>
      </section>
      <section class="keys">
        <h3>Keyboard shortcuts</h3>
        <dl>
          <dt>Ctrl/⌘ + 1 / 2</dt><dd>All commits / Local changes</dd>
          <dt>Ctrl/⌘ + F</dt><dd>Search commits</dd>
          <dt>Ctrl/⌘ + T</dt><dd>Open repository</dd>
          <dt>Ctrl/⌘ + Enter</dt><dd>Commit</dd>
          <dt>F5 / Ctrl/⌘ + R</dt><dd>Refresh repository</dd>
          <dt>↑ ↓ PgUp PgDn</dt><dd>Move through commits</dd>
        </dl>
      </section>
    </div>
  `,
  styles: `
    :host { position: fixed; inset: 0; z-index: 800; background: var(--backdrop); display: grid; place-items: center; padding: 16px; }
    .dialog { width: min(520px, 100%); max-height: 100%; overflow: auto; background: var(--panel); border: 1px solid var(--border);
      border-radius: 12px; box-shadow: var(--shadow); padding: 16px 20px 20px; }
    header { display: flex; align-items: center; }
    h2 { margin: 0; font-size: 18px; }
    h3 { margin: 18px 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: .05em; color: var(--text-muted); }
    .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 4px 0; }
    .row.check { justify-content: flex-start; }
    .row input[type=number] { width: 100px; }
    .row app-select { width: 160px; }
    dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 16px; margin: 0; font-size: 12px; }
    dt { font-family: var(--mono); color: var(--text-muted); }
    dd { margin: 0; }
  `,
})
export class SettingsDialog {
  protected readonly settings = inject(SettingsStore);
  protected readonly ui = inject(Ui);
  protected readonly themes = [
    { value: 'system', label: 'Follow system' },
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];
  protected readonly diffModes = [
    { value: 'unified', label: 'Unified' },
    { value: 'split', label: 'Side by side' },
  ];

  protected set<K extends keyof Settings>(key: K, value: Settings[K]): void {
    this.settings.update({ [key]: value } as Partial<Settings>);
  }

  protected clamp(value: string, min: number, max: number): number {
    const n = Math.round(Number(value));
    return Number.isFinite(n) ? Math.max(min, Math.min(max, n)) : min;
  }
}
