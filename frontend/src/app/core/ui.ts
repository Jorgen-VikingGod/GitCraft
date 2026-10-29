import { Injectable, signal } from '@angular/core';
import type { RepoState } from './repo-state';

export type ToastKind = 'info' | 'success' | 'warning' | 'error';

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  detail?: string;
}

export interface MenuItem {
  label?: string;
  action?: () => void;
  danger?: boolean;
  disabled?: boolean;
  separator?: boolean;
  hint?: string;
  /** Section title (not clickable). */
  header?: boolean;
  /** Submenu (one level). */
  children?: MenuItem[];
}

export interface FormField {
  key: string;
  label: string;
  type: 'text' | 'textarea' | 'checkbox' | 'select' | 'number';
  value?: string | number | boolean | null;
  options?: { value: string; label: string }[];
  placeholder?: string;
  required?: boolean;
  hint?: string;
}

export interface FormSpec {
  title: string;
  message?: string;
  fields?: FormField[];
  ok?: string;
  cancel?: string;
  danger?: boolean;
}

export type FormValues = Record<string, string | number | boolean | null>;

export type Overlay =
  | { kind: 'open' }
  | { kind: 'settings' }
  | { kind: 'rebase'; repo: RepoState; base: string | null }
  | { kind: 'conflict'; repo: RepoState; path: string }
  | { kind: 'file-history'; repo: RepoState; path: string }
  | { kind: 'blame'; repo: RepoState; path: string; rev: string | null }
  | { kind: 'reflog'; repo: RepoState };

/** App-wide UI state: toasts, modal forms, context menu and full-screen overlays. */
@Injectable({ providedIn: 'root' })
export class Ui {
  private nextId = 1;
  readonly toasts = signal<Toast[]>([]);
  readonly form = signal<{ spec: FormSpec; resolve: (values: FormValues | null) => void } | null>(null);
  readonly menu = signal<{ x: number; y: number; items: MenuItem[] } | null>(null);
  readonly overlay = signal<Overlay | null>(null);

  toast(message: string, kind: ToastKind = 'info', detail?: string): void {
    const id = this.nextId++;
    this.toasts.update((list) => [...list, { id, kind, message, detail }]);
    const timeout = kind === 'error' ? 12000 : kind === 'warning' ? 8000 : 4000;
    setTimeout(() => this.dismiss(id), timeout);
  }

  dismiss(id: number): void {
    this.toasts.update((list) => list.filter((t) => t.id !== id));
  }

  ask(spec: FormSpec): Promise<FormValues | null> {
    this.form()?.resolve(null);
    return new Promise((resolve) => this.form.set({ spec, resolve }));
  }

  async confirm(title: string, message: string, ok = 'OK', danger = false): Promise<boolean> {
    return (await this.ask({ title, message, ok, danger })) !== null;
  }

  closeForm(values: FormValues | null): void {
    const current = this.form();
    this.form.set(null);
    current?.resolve(values);
  }

  openMenu(event: MouseEvent, items: MenuItem[]): void {
    event.preventDefault();
    event.stopPropagation();
    this.menu.set({ x: event.clientX, y: event.clientY, items });
  }

  open(overlay: Overlay): void {
    this.overlay.set(overlay);
  }

  closeOverlay(): void {
    this.overlay.set(null);
  }
}

export async function copyText(text: string, ui: Ui): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    ui.toast('Copied to clipboard', 'success');
  } catch {
    ui.toast('Clipboard is not available', 'error');
  }
}
