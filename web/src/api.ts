import { t } from './i18n';

const mock = new URLSearchParams(location.search).get('mock');

/** JSON request; `?mock` in the page URL serves built-in sample data instead (UI preview without a database). */
export async function api<T = any>(path: string, init?: RequestInit): Promise<T> {
  if (mock !== null) return (await import('./mock')).mockApi(path, init, mock) as T;
  const headers = new Headers(init?.headers);
  if (init?.body != null && !headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
  const response = await fetch(path, { ...init, headers });
  const text = await response.text();
  if (!response.ok) throw new Error(t(text || response.statusText));
  return JSON.parse(text);
}

export async function confirmBeforeLeaving<T>(task: () => Promise<T>): Promise<T> {
  const confirm = (event: BeforeUnloadEvent) => {
    event.preventDefault();
    event.returnValue = true;
  };
  window.addEventListener('beforeunload', confirm);
  try { return await task(); }
  finally { window.removeEventListener('beforeunload', confirm); }
}

export const post = <T = any>(path: string, body?: unknown, method = 'POST') => {
  const request = () => api<T>(path, { method, body: body === undefined ? undefined : JSON.stringify(body) });
  return path === '/api/sync' || path === '/api/table-samples' ? confirmBeforeLeaving(request) : request();
};

export type Page<T> = { total: number; rows: T[] };

export function toast(message: string) { window.dispatchEvent(new CustomEvent('mymeta-toast', { detail: message })); }

export const del = (path: string) => api(path, { method: 'DELETE' });
