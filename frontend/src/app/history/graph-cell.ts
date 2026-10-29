import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { GraphRow } from '../core/models';

export const LANE_WIDTH = 14;
export const GRAPH_PAD = 10;

const laneX = (lane: number) => GRAPH_PAD + lane * LANE_WIDTH;

/** One row of the commit graph, drawn from the backend's lane layout. */
@Component({
  selector: 'app-graph-cell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg [attr.width]="width()" [attr.height]="height()" [attr.viewBox]="'0 0 ' + width() + ' ' + height()">
      @for (p of paths(); track $index) {
        <path [attr.d]="p.d" [style.stroke]="color(p.color)" />
      }
      @if (row(); as r) {
        @if (merge()) {
          <circle [attr.cx]="dotX()" [attr.cy]="height() / 2" r="4" class="merge" [style.stroke]="color(r.color)" />
        } @else {
          <circle [attr.cx]="dotX()" [attr.cy]="height() / 2" [attr.r]="head() ? 5.5 : 4.5" [style.fill]="color(r.color)"
            [class.head]="head()" />
        }
      }
    </svg>
  `,
  styles: `
    :host { display: block; flex: none; }
    svg { display: block; overflow: visible; }
    path { fill: none; stroke-width: 2; }
    circle.merge { fill: var(--bg); stroke-width: 2.5; }
    circle.head { stroke: var(--bg); stroke-width: 2; }
  `,
})
export class GraphCell {
  readonly row = input<GraphRow | undefined>();
  readonly width = input(40);
  readonly height = input(26);
  readonly merge = input(false);
  readonly head = input(false);

  protected readonly dotX = computed(() => laneX(this.row()?.col ?? 0));

  protected readonly paths = computed(() => {
    const row = this.row();
    if (!row) return [];
    const h = this.height();
    const mid = h / 2;
    return row.segments.map(([from, to, kind, color]) => {
      const x1 = laneX(from);
      const x2 = laneX(to);
      let d: string;
      if (kind === 0) d = `M${x1} 0V${h}`;
      else if (kind === 1) d = x1 === x2 ? `M${x1} 0V${mid}` : `M${x1} 0C${x1} ${mid * 0.8} ${x2} ${mid * 0.4} ${x2} ${mid}`;
      else d = x1 === x2 ? `M${x1} ${mid}V${h}` : `M${x1} ${mid}C${x1} ${mid + mid * 0.6} ${x2} ${mid + mid * 0.2} ${x2} ${h}`;
      return { d, color };
    });
  });

  protected color(index: number): string {
    return `var(--lane-${index % 8})`;
  }
}
