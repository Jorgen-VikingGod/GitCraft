import { ConflictChunk } from '../core/models';
import { buildResult } from './conflict-resolver';

describe('buildResult', () => {
  const chunks: ConflictChunk[] = [
    { type: 'common', lines: ['a'] },
    { type: 'conflict', ours: ['ours'], base: [], theirs: ['theirs'] },
    { type: 'common', lines: ['z', ''] },
  ];

  it('applies each choice and keeps the trailing newline', () => {
    expect(buildResult(chunks, ['ours'])).toBe('a\nours\nz\n');
    expect(buildResult(chunks, ['theirs'])).toBe('a\ntheirs\nz\n');
    expect(buildResult(chunks, ['ours-theirs'])).toBe('a\nours\ntheirs\nz\n');
    expect(buildResult(chunks, ['theirs-ours'])).toBe('a\ntheirs\nours\nz\n');
  });

  it('keeps conflict markers for unresolved chunks', () => {
    expect(buildResult(chunks, [null])).toBe('a\n<<<<<<< ours\nours\n=======\ntheirs\n>>>>>>> theirs\nz\n');
  });
});
