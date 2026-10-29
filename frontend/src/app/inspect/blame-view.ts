import { ChangeDetectionStrategy, Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { shortSha } from '../core/actions';
import { errorMessage } from '../core/api';
import { BlameLine } from '../core/models';
import { RepoState } from '../core/repo-state';
import { SettingsStore } from '../core/settings';
import { Ui } from '../core/ui';
import { highlightLine, languageFor } from '../diff/highlight';
import { fullDate, shortDate } from '../shared/format';
import { Icon } from '../shared/icon';

interface Row extends BlameLine {
  first: boolean;
  html: string;
  age: number;
}

@Component({
  selector: 'app-blame-view',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(document:keydown.escape)': 'ui.closeOverlay()' },
  template: `
    <div class="dialog">
      <header>
        <app-icon name="user" [size]="18" />
        <div class="title">
          <h2>Blame</h2>
          <div class="path">{{ path() }}{{ rev() ? ' @ ' + short(rev()!) : '' }}</div>
        </div>
        <span class="spacer"></span>
        <button class="btn small" (click)="ui.open({ kind: 'file-history', repo: repo(), path: path() })">File history</button>
        <button class="icon-btn" (click)="ui.closeOverlay()" title="Close"><app-icon name="x" [size]="18" /></button>
      </header>
      <div class="blame">
        @if (loading()) {
          <div class="empty">Loading…</div>
        } @else if (error()) {
          <div class="empty error">{{ error() }}</div>
        } @else {
          <table>
            @for (row of rows(); track row.line) {
              <tr [class.first]="row.first" [class.hover]="hovered() === row.sha"
                (mouseenter)="hovered.set(row.sha)">
                <td class="gutter" [style.border-left-color]="'hsl(210 70% ' + (85 - row.age * 45) + '%)'"
                  (click)="show(row)" [title]="row.summary + '\\n' + row.author + ', ' + long(row.time)">
                  @if (row.first) {
                    <div class="g">
                      <span class="g-sha">{{ short(row.sha) }}</span>
                      <span class="g-author">{{ row.author }}</span>
                      <span class="g-date">{{ date(row.time) }}</span>
                    </div>
                  }
                </td>
                <td class="no">{{ row.line }}</td>
                <td class="code" [innerHTML]="row.html"></td>
              </tr>
            }
          </table>
        }
      </div>
    </div>
  `,
  styleUrl: './inspect.scss',
})
export class BlameView implements OnInit {
  protected readonly ui = inject(Ui);
  private readonly settings = inject(SettingsStore);
  readonly repo = input.required<RepoState>();
  readonly path = input.required<string>();
  readonly rev = input<string | null>(null);

  protected readonly lines = signal<BlameLine[]>([]);
  protected readonly loading = signal(true);
  protected readonly error = signal<string | null>(null);
  protected readonly hovered = signal<string | null>(null);
  protected readonly date = shortDate;
  protected readonly long = fullDate;
  protected readonly short = shortSha;

  protected readonly rows = computed<Row[]>(() => {
    const lines = this.lines();
    const language = this.settings.value().syntaxHighlight && lines.length < 8000 ? languageFor(this.path()) : null;
    const times = lines.map((l) => l.time);
    const min = Math.min(...times);
    const span = Math.max(1, Math.max(...times) - min);
    return lines.map((line, i) => ({
      ...line,
      first: i === 0 || lines[i - 1].sha !== line.sha,
      html: highlightLine(line.text, language) || ' ',
      age: (line.time - min) / span,
    }));
  });

  async ngOnInit(): Promise<void> {
    try {
      this.lines.set(await this.repo().get<BlameLine[]>('/blame', { path: this.path(), rev: this.rev() }));
    } catch (e) {
      this.error.set(errorMessage(e));
    } finally {
      this.loading.set(false);
    }
  }

  protected show(row: BlameLine): void {
    if (/^0+$/.test(row.sha)) return;
    const repo = this.repo();
    repo.search.set(null);
    repo.view.set('history');
    repo.selectedSha.set(row.sha);
    this.ui.closeOverlay();
  }
}
