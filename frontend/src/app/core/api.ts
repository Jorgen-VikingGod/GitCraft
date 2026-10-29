import { HttpClient, HttpErrorResponse, HttpParams } from '@angular/common/http';
import { Injectable, inject, signal } from '@angular/core';
import { Observable, firstValueFrom } from 'rxjs';

export type Params = Record<string, string | number | boolean | null | undefined>;

const TOKEN_KEY = 'gitcraft-token';

function safeSession(action: (s: Storage) => string | null | void): string | null {
  try {
    return action(sessionStorage) ?? null;
  } catch {
    return null;
  }
}

/** Reads (and strips) the launch parameters `gitcraft` puts into the URL. */
function readLaunchParams(): { token: string; open: string | null } {
  const url = new URL(location.href);
  const token = url.searchParams.get('token');
  const open = url.searchParams.get('open');
  if (token) {
    safeSession((s) => s.setItem(TOKEN_KEY, token));
  }
  if (token || open) {
    url.searchParams.delete('token');
    url.searchParams.delete('open');
    history.replaceState(null, '', url.pathname + url.search + url.hash);
  }
  return { token: token ?? safeSession((s) => s.getItem(TOKEN_KEY)) ?? '', open };
}

export const launch = readLaunchParams();

export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly command?: string,
  ) {
    super(message);
  }
}

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

function toParams(params: Params): HttpParams {
  let result = new HttpParams();
  for (const [key, value] of Object.entries(params)) {
    if (value !== null && value !== undefined) result = result.set(key, String(value));
  }
  return result;
}

@Injectable({ providedIn: 'root' })
export class Api {
  private readonly http = inject(HttpClient);
  readonly token = launch.token;
  /** Set when the backend rejects our session token. */
  readonly unauthorized = signal(false);

  get<T>(path: string, params: Params = {}): Promise<T> {
    return this.wrap(this.http.get<T>('/api' + path, { params: toParams(params), headers: this.headers() }));
  }

  getText(path: string, params: Params = {}): Promise<string> {
    return this.wrap(
      this.http.get('/api' + path, { params: toParams(params), headers: this.headers(), responseType: 'text' }),
    );
  }

  post<T>(path: string, body: unknown = {}, params: Params = {}): Promise<T> {
    return this.wrap(
      this.http.post<T>('/api' + path, body, { params: toParams(params), headers: this.headers() }),
    );
  }

  socketUrl(): string {
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    return `${scheme}://${location.host}/api/ws?token=${encodeURIComponent(this.token)}`;
  }

  private headers(): Record<string, string> {
    return { 'x-gitcraft-token': this.token };
  }

  private async wrap<T>(request: Observable<unknown>): Promise<T> {
    try {
      return (await firstValueFrom(request)) as T;
    } catch (error) {
      if (error instanceof HttpErrorResponse) {
        if (error.status === 401) this.unauthorized.set(true);
        let body = error.error;
        if (typeof body === 'string') {
          try {
            body = JSON.parse(body);
          } catch {
            // Plain-text error body.
          }
        }
        const message =
          (body && typeof body === 'object' && (body.error || body.detail)) ||
          (error.status === 0 ? 'Cannot reach the GitCraft server' : error.message);
        throw new ApiError(String(message), error.status, body?.command);
      }
      throw error;
    }
  }
}
