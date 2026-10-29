import { buildTree } from './sidebar';

describe('buildTree', () => {
  const items = ['main', 'feature/login', 'feature/ui/menu', 'bugfix/a'].map((name) => ({ name, item: name }));

  it('groups slash-separated names into folders before leaves', () => {
    const rows = buildTree(items, 'local', new Set());
    expect(rows.map((r) => `${'  '.repeat(r.depth)}${r.folder ? r.label + '/' : r.label}`)).toEqual([
      'bugfix/',
      '  a',
      'feature/',
      '  ui/',
      '    menu',
      '  login',
      'main',
    ]);
    expect(rows.find((r) => r.label === 'menu')?.item).toBe('feature/ui/menu');
  });

  it('hides children of collapsed folders', () => {
    const rows = buildTree(items, 'local', new Set(['local/feature']));
    expect(rows.map((r) => r.label)).toEqual(['bugfix', 'a', 'feature', 'main']);
  });
});
