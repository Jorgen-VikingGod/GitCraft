import hljs from 'highlight.js/lib/common';

const BY_NAME: Record<string, string> = {
  makefile: 'makefile',
  dockerfile: 'dockerfile',
  'cmakelists.txt': 'cmake',
};

const BY_EXTENSION: Record<string, string> = {
  h: 'cpp',
  hpp: 'cpp',
  cc: 'cpp',
  ino: 'cpp',
  html: 'xml',
  htm: 'xml',
  svg: 'xml',
  vue: 'xml',
  yml: 'yaml',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript',
  tsx: 'typescript',
  pyi: 'python',
  toml: 'ini',
  cfg: 'ini',
  conf: 'ini',
  gradle: 'groovy',
  kts: 'kotlin',
};

export function languageFor(path: string | null): string | null {
  if (!path) return null;
  const file = path.split('/').pop()!.toLowerCase();
  if (BY_NAME[file]) return hljs.getLanguage(BY_NAME[file]) ? BY_NAME[file] : null;
  const ext = file.includes('.') ? file.split('.').pop()! : '';
  const language = BY_EXTENSION[ext] ?? ext;
  return language && hljs.getLanguage(language) ? language : null;
}

export function escapeHtml(text: string): string {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** Highlight a single line; returns HTML. Falls back to escaped text. */
export function highlightLine(text: string, language: string | null): string {
  if (!language || text.length > 2000) return escapeHtml(text);
  try {
    return hljs.highlight(text, { language, ignoreIllegals: true }).value;
  } catch {
    return escapeHtml(text);
  }
}
