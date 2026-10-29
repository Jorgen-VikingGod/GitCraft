import { Injectable, effect, signal } from '@angular/core';

export interface Settings {
  theme: 'system' | 'light' | 'dark';
  diffContext: number;
  ignoreWhitespace: boolean;
  diffMode: 'unified' | 'split';
  syntaxHighlight: boolean;
  logLimit: number;
  fetchOnOpen: boolean;
}

const DEFAULTS: Settings = {
  theme: 'system',
  diffContext: 3,
  ignoreWhitespace: false,
  diffMode: 'unified',
  syntaxHighlight: true,
  logLimit: 2000,
  fetchOnOpen: false,
};

const KEY = 'gitcraft-settings';

export function readStorage<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? { ...fallback, ...JSON.parse(raw) } : fallback;
  } catch {
    return fallback;
  }
}

export function readStorageValue<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function writeStorage(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Storage may be unavailable (private mode); settings then last for the session.
  }
}

@Injectable({ providedIn: 'root' })
export class SettingsStore {
  readonly value = signal<Settings>(readStorage(KEY, DEFAULTS));

  constructor() {
    effect(() => {
      const settings = this.value();
      writeStorage(KEY, settings);
      const root = document.documentElement;
      if (settings.theme === 'system') delete root.dataset['theme'];
      else root.dataset['theme'] = settings.theme;
    });
  }

  update(patch: Partial<Settings>): void {
    this.value.update((current) => ({ ...current, ...patch }));
  }
}
