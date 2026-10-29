import { TestBed } from '@angular/core/testing';
import { FileDiff } from '../core/models';
import { DiffView, HunkRequest } from './diff-view';
import { languageFor } from './highlight';

const DIFF: FileDiff = {
  header: ['diff --git a/f.ts b/f.ts'],
  oldPath: 'f.ts',
  newPath: 'f.ts',
  status: 'M',
  binary: false,
  truncated: false,
  hunks: [
    {
      header: '@@ -1,3 +1,3 @@',
      oldStart: 1,
      oldLines: 3,
      newStart: 1,
      newLines: 3,
      context: '',
      lines: [
        { t: ' ', text: 'const a = 1;', old: 1, new: 1 },
        { t: '-', text: 'const b = 2;', old: 2, new: null },
        { t: '+', text: 'const b = 3;', old: null, new: 2 },
        { t: ' ', text: 'const c = 4;', old: 3, new: 3 },
      ],
    },
  ],
};

describe('DiffView', () => {
  beforeEach(() => localStorage.clear());

  async function render(stageMode: 'stage' | 'unstage' | null) {
    const fixture = TestBed.createComponent(DiffView);
    fixture.componentRef.setInput('diff', DIFF);
    fixture.componentRef.setInput('stageMode', stageMode);
    const requests: HunkRequest[] = [];
    fixture.componentInstance.hunkAction.subscribe((r) => requests.push(r));
    await fixture.whenStable();
    return { fixture, el: fixture.nativeElement as HTMLElement, requests };
  }

  it('renders lines with numbers and stats', async () => {
    const { el } = await render(null);
    expect(el.querySelectorAll('tr').length).toBe(4);
    expect(el.querySelector('.stats')?.textContent).toContain('+1');
    expect(el.querySelector('.hunk-header button')).toBeNull();
  });

  it('stages the whole hunk or only the selected lines', async () => {
    const { fixture, el, requests } = await render('stage');
    const buttons = () => [...el.querySelectorAll<HTMLButtonElement>('.hunk-header button')];
    expect(buttons().map((b) => b.textContent?.trim())).toEqual(['Discard hunk', 'Stage hunk']);
    buttons()[1].click();
    expect(requests[0]).toEqual({ hunk: 0, lines: null, mode: 'stage' });

    const added = el.querySelector<HTMLElement>('tr[data-t="+"]')!;
    added.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
    await fixture.whenStable();
    expect(buttons()[1].textContent?.trim()).toBe('Stage 1 line');
    buttons()[1].click();
    expect(requests[1]).toEqual({ hunk: 0, lines: [2], mode: 'stage' });
  });

  it('ignores clicks on context lines', async () => {
    const { fixture, el } = await render('unstage');
    el.querySelector<HTMLElement>('tr[data-t=" "]')!.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true }));
    await fixture.whenStable();
    expect(el.querySelector('tr.sel')).toBeNull();
  });
});

describe('languageFor', () => {
  it('maps file names and extensions to highlight.js languages', () => {
    expect(languageFor('src/app.ts')).toBe('ts'); // highlight.js alias
    expect(languageFor('include/x.h')).toBe('cpp');
    expect(languageFor('Makefile')).toBe('makefile');
    expect(languageFor('notes.unknownext')).toBeNull();
    expect(languageFor(null)).toBeNull();
  });
});
