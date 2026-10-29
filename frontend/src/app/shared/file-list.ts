import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { FileChange } from '../core/models';
import { STATUS_LABELS, splitPath } from '../shared/format';
import { Icon } from './icon';

export interface FilePick {
  file: FileChange;
  event: MouseEvent;
}

@Component({
  selector: 'app-file-list',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (file of files(); track file.path) {
      <div
        class="file"
        [class.selected]="selected().includes(file.path)"
        (click)="pick.emit({ file, event: $event })"
        (dblclick)="activate.emit(file)"
        (contextmenu)="menu.emit({ file, event: $event })"
        [title]="title(file)"
      >
        <span class="status" [attr.data-status]="file.status">{{ file.status === '?' ? '+' : file.status }}</span>
        <span class="path">
          @if (file.origPath) {
            <span class="dir">{{ file.origPath }} → </span>
          }
          <span class="dir">{{ split(file.path).dir }}</span><span class="name">{{ split(file.path).name }}</span>
        </span>
        @if (actionIcon()) {
          <button class="action icon-btn" [title]="actionTitle()" (click)="$event.stopPropagation(); action.emit(file)">
            <app-icon [name]="actionIcon()!" [size]="14" />
          </button>
        }
      </div>
    } @empty {
      <div class="empty">{{ emptyText() }}</div>
    }
  `,
  styles: `
    :host { display: block; }
    .file {
      display: flex; align-items: center; gap: 8px; height: 26px; padding: 0 8px 0 10px;
      cursor: default; white-space: nowrap; border-radius: 4px; margin: 0 4px;
    }
    .file:hover { background: var(--hover); }
    .file.selected { background: var(--selection); color: var(--selection-text); }
    .file.selected .dir { color: inherit; opacity: .75; }
    .path { overflow: hidden; text-overflow: ellipsis; flex: 1; direction: ltr; }
    .dir { color: var(--text-muted); }
    .action { visibility: hidden; }
    .file:hover .action, .file.selected .action { visibility: visible; }
    .empty { padding: 10px 14px; color: var(--text-muted); font-style: italic; }
  `,
})
export class FileList {
  readonly files = input.required<FileChange[]>();
  readonly selected = input<string[]>([]);
  readonly actionIcon = input<string | null>(null);
  readonly actionTitle = input('');
  readonly emptyText = input('No files');
  readonly pick = output<FilePick>();
  readonly activate = output<FileChange>();
  readonly menu = output<FilePick>();
  readonly action = output<FileChange>();

  protected split = splitPath;

  protected title(file: FileChange): string {
    return `${STATUS_LABELS[file.status] ?? file.status}: ${file.origPath ? file.origPath + ' → ' : ''}${file.path}`;
  }
}
