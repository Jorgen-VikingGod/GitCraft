import { Injectable, inject, signal } from '@angular/core';
import { Subject } from 'rxjs';
import { Api } from './api';

export interface ChangeEvent {
  type: 'changed';
  repo: string;
  git: boolean;
  worktree: boolean;
}

export interface ProgressEvent {
  type: 'progress';
  opId: string;
  repo: string | null;
  line: string;
}

/** WebSocket connection to the backend: file watching and operation progress. */
@Injectable({ providedIn: 'root' })
export class Events {
  private readonly api = inject(Api);
  private socket: WebSocket | null = null;
  private watched: string[] = [];
  private retry = 1000;

  readonly connected = signal(false);
  readonly changes = new Subject<ChangeEvent>();
  readonly progress = new Subject<ProgressEvent>();

  connect(): void {
    if (this.socket || !this.api.token) return;
    const socket = new WebSocket(this.api.socketUrl());
    this.socket = socket;
    socket.onopen = () => {
      this.connected.set(true);
      this.retry = 1000;
      this.sendWatch();
    };
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.type === 'changed') this.changes.next(message);
      else if (message.type === 'progress') this.progress.next(message);
    };
    socket.onclose = () => {
      this.socket = null;
      this.connected.set(false);
      setTimeout(() => this.connect(), this.retry);
      this.retry = Math.min(this.retry * 2, 15000);
    };
  }

  watch(repos: string[]): void {
    this.watched = repos;
    this.sendWatch();
  }

  private sendWatch(): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify({ type: 'watch', repos: this.watched }));
    }
  }
}
