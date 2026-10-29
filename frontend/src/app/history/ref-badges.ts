import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { RefLabel } from '../core/models';
import { Icon } from '../shared/icon';

@Component({
  selector: 'app-ref-badges',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (ref of refs(); track ref.kind + ref.name) {
      <span class="badge" [attr.data-kind]="ref.kind" [class.current]="ref.current"
        (contextmenu)="menu.emit({ ref, event: $event })" (dblclick)="$event.stopPropagation(); activate.emit(ref)">
        @switch (ref.kind) {
          @case ('tag') { <app-icon name="tag" [size]="11" /> }
          @case ('remote') { <app-icon name="cloud" [size]="11" /> }
          @case ('head') { <app-icon name="commit" [size]="11" /> }
          @default { @if (ref.current) { <app-icon name="check" [size]="11" /> } @else { <app-icon name="branch" [size]="11" /> } }
        }
        <span>{{ ref.name }}</span>
      </span>
    }
  `,
  styles: `
    :host { display: inline-flex; gap: 4px; flex: none; max-width: 55%; overflow: hidden; }
    .badge {
      display: inline-flex; align-items: center; gap: 3px; padding: 0 6px; height: 18px; border-radius: 9px;
      font-size: 11px; font-weight: 500; white-space: nowrap; border: 1px solid transparent;
      background: var(--badge-local-bg); color: var(--badge-local-text);
    }
    .badge span { overflow: hidden; text-overflow: ellipsis; max-width: 180px; }
    .badge.current { font-weight: 700; border-color: var(--badge-local-text); }
    .badge[data-kind='remote'] { background: var(--badge-remote-bg); color: var(--badge-remote-text); }
    .badge[data-kind='tag'] { background: var(--badge-tag-bg); color: var(--badge-tag-text); }
    .badge[data-kind='head'] { background: var(--badge-head-bg); color: var(--badge-head-text); }
  `,
})
export class RefBadges {
  readonly refs = input.required<RefLabel[]>();
  readonly menu = output<{ ref: RefLabel; event: MouseEvent }>();
  readonly activate = output<RefLabel>();
}
