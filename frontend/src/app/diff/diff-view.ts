import { ChangeDetectionStrategy, Component, computed, effect, inject, input, output, signal } from '@angular/core';
import { DiffLine, FileDiff, Hunk } from '../core/models';
import { SettingsStore } from '../core/settings';
import { Icon } from '../shared/icon';
import { escapeHtml, highlightLine, languageFor } from './highlight';

export type HunkMode = 'stage' | 'unstage' | 'discard';

export interface HunkRequest {
  hunk: number;
  /** Selected line indices within the hunk, or null for the whole hunk. */
  lines: number[] | null;
  mode: HunkMode;
}

interface Cell {
  line: DiffLine;
  index: number;
  html: string;
}

interface SplitRow {
  left: Cell | null;
  right: Cell | null;
}

interface RenderedHunk {
  hunk: Hunk;
  index: number;
  title: string;
  cells: Cell[];
  split: SplitRow[];
}

const HIGHLIGHT_LIMIT = 6000;

@Component({
  selector: 'app-diff-view',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (loading()) {
      <div class="placeholder">Loading diff…</div>
    } @else if (!diff()) {
      <div class="placeholder">{{ emptyText() }}</div>
    } @else {
      @let d = diff()!;
      <div class="file-header">
        <app-icon name="file" [size]="14" />
        <span class="title">
          @if (d.oldPath && d.newPath && d.oldPath !== d.newPath) { {{ d.oldPath }} → }{{ d.newPath ?? d.oldPath }}
        </span>
        <span class="stats"><span class="add">+{{ stats().added }}</span> <span class="del">−{{ stats().removed }}</span></span>
        <span class="spacer"></span>
        <ng-content />
        <div class="segmented">
          <button [class.active]="mode() === 'unified'" (click)="setMode('unified')">Unified</button>
          <button [class.active]="mode() === 'split'" (click)="setMode('split')">Split</button>
        </div>
      </div>
      @if (d.binary) {
        <div class="placeholder">Binary file — no text diff available</div>
      } @else if (!d.hunks.length) {
        <div class="placeholder">No content changes (mode or rename only)</div>
      }
      <div class="hunks" [class.selectable]="selectable()">
        @for (h of rendered(); track h.index) {
          <div class="hunk-header">
            <span class="hunk-title">{{ h.title }} <span class="ctx">{{ h.hunk.context }}</span></span>
            @if (selectable()) {
              @let count = selectedIn(h.index);
              @if (stageMode() === 'stage') {
                <button class="btn small danger-text" (click)="emit(h.index, 'discard')">
                  {{ count ? 'Discard ' + count + ' line' + (count > 1 ? 's' : '') : 'Discard hunk' }}
                </button>
                <button class="btn small primary" (click)="emit(h.index, 'stage')">
                  {{ count ? 'Stage ' + count + ' line' + (count > 1 ? 's' : '') : 'Stage hunk' }}
                </button>
              } @else {
                <button class="btn small" (click)="emit(h.index, 'unstage')">
                  {{ count ? 'Unstage ' + count + ' line' + (count > 1 ? 's' : '') : 'Unstage hunk' }}
                </button>
              }
            }
          </div>
          @if (mode() === 'unified') {
            <table class="code">
              @for (c of h.cells; track c.index) {
                <tr [attr.data-t]="c.line.t" [class.sel]="isSelected(h.index, c.index)" (mousedown)="toggle($event, h.index, c)">
                  <td class="no">{{ c.line.old ?? '' }}</td>
                  <td class="no">{{ c.line.new ?? '' }}</td>
                  <td class="sign">{{ c.line.t === '\\\\' ? '' : c.line.t }}</td>
                  <td class="text" [innerHTML]="c.html"></td>
                </tr>
              }
            </table>
          } @else {
            <table class="code split">
              @for (row of h.split; track $index) {
                <tr>
                  @if (row.left; as l) {
                    <td class="no" [attr.data-t]="l.line.t">{{ l.line.old ?? '' }}</td>
                    <td class="text half" [attr.data-t]="l.line.t" [class.sel]="isSelected(h.index, l.index)"
                      (mousedown)="toggle($event, h.index, l)" [innerHTML]="l.html"></td>
                  } @else {
                    <td class="no filler"></td><td class="text half filler"></td>
                  }
                  @if (row.right; as r) {
                    <td class="no" [attr.data-t]="r.line.t">{{ r.line.new ?? '' }}</td>
                    <td class="text half" [attr.data-t]="r.line.t" [class.sel]="isSelected(h.index, r.index)"
                      (mousedown)="toggle($event, h.index, r)" [innerHTML]="r.html"></td>
                  } @else {
                    <td class="no filler"></td><td class="text half filler"></td>
                  }
                </tr>
              }
            </table>
          }
        }
      </div>
      @if (d.truncated) {
        <div class="placeholder">Diff truncated — the file is too large to show completely.</div>
      }
      @if (selectable() && note()) {
        <div class="note">{{ note() }}</div>
      }
    }
  `,
  styleUrl: './diff-view.scss',
})
export class DiffView {
  private readonly settings = inject(SettingsStore);

  readonly diff = input<FileDiff | null>(null);
  readonly loading = input(false);
  readonly emptyText = input('Select a file to see its changes');
  /** null = read-only, 'stage' = unstaged changes, 'unstage' = staged changes. */
  readonly stageMode = input<'stage' | 'unstage' | null>(null);
  readonly allowPartial = input(true);
  readonly note = input<string | null>(null);
  readonly hunkAction = output<HunkRequest>();

  private readonly selection = signal<ReadonlySet<string>>(new Set());
  private anchor: { hunk: number; index: number } | null = null;

  protected readonly mode = computed(() => this.settings.value().diffMode);
  protected readonly selectable = computed(() => this.stageMode() !== null && this.allowPartial());

  protected readonly stats = computed(() => {
    let added = 0;
    let removed = 0;
    for (const hunk of this.diff()?.hunks ?? []) {
      for (const line of hunk.lines) {
        if (line.t === '+') added++;
        else if (line.t === '-') removed++;
      }
    }
    return { added, removed };
  });

  protected readonly rendered = computed<RenderedHunk[]>(() => {
    const diff = this.diff();
    if (!diff) return [];
    const total = diff.hunks.reduce((n, h) => n + h.lines.length, 0);
    const language =
      this.settings.value().syntaxHighlight && total <= HIGHLIGHT_LIMIT ? languageFor(diff.newPath ?? diff.oldPath) : null;
    return diff.hunks.map((hunk, index) => {
      const cells = hunk.lines.map((line, i) => ({
        line,
        index: i,
        html: line.t === '\\' ? `<i>${escapeHtml(line.text)}</i>` : highlightLine(line.text, language) || ' ',
      }));
      const title = `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`;
      return { hunk, index, title, cells, split: toSplit(cells) };
    });
  });

  constructor() {
    effect(() => {
      this.diff();
      this.selection.set(new Set());
      this.anchor = null;
    });
  }

  protected setMode(mode: 'unified' | 'split'): void {
    this.settings.update({ diffMode: mode });
  }

  protected isSelected(hunk: number, index: number): boolean {
    return this.selection().has(`${hunk}:${index}`);
  }

  protected selectedIn(hunk: number): number {
    let count = 0;
    for (const key of this.selection()) if (key.startsWith(hunk + ':')) count++;
    return count;
  }

  protected toggle(event: MouseEvent, hunk: number, cell: Cell): void {
    if (!this.selectable() || (cell.line.t !== '+' && cell.line.t !== '-') || event.button !== 0) return;
    event.preventDefault();
    const next = new Set(this.selection());
    if (event.shiftKey && this.anchor && this.anchor.hunk === hunk) {
      const lines = this.diff()!.hunks[hunk].lines;
      const [from, to] = [Math.min(this.anchor.index, cell.index), Math.max(this.anchor.index, cell.index)];
      for (let i = from; i <= to; i++) if (lines[i].t === '+' || lines[i].t === '-') next.add(`${hunk}:${i}`);
    } else {
      const key = `${hunk}:${cell.index}`;
      if (next.has(key)) next.delete(key);
      else next.add(key);
      this.anchor = { hunk, index: cell.index };
    }
    this.selection.set(next);
  }

  protected emit(hunk: number, mode: HunkMode): void {
    const lines = [...this.selection()]
      .filter((key) => key.startsWith(hunk + ':'))
      .map((key) => Number(key.split(':')[1]));
    this.hunkAction.emit({ hunk, lines: lines.length ? lines : null, mode });
  }
}

/** Pair removed/added runs side by side; context lines appear on both sides. */
function toSplit(cells: Cell[]): SplitRow[] {
  const rows: SplitRow[] = [];
  let removed: Cell[] = [];
  let added: Cell[] = [];
  const flush = () => {
    for (let i = 0; i < Math.max(removed.length, added.length); i++) {
      rows.push({ left: removed[i] ?? null, right: added[i] ?? null });
    }
    removed = [];
    added = [];
  };
  for (const cell of cells) {
    if (cell.line.t === '-') removed.push(cell);
    else if (cell.line.t === '+') added.push(cell);
    else {
      flush();
      rows.push({ left: cell, right: cell });
    }
  }
  flush();
  return rows;
}
