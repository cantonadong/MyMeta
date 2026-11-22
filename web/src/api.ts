import { t } from './i18n';

const mock = new URLSearchParams(location.search).get('mock');

/** JSON request; `?mock` in the page URL serves built-in sample data instead (UI preview without a database). */
export async function api<T = any>(path: string, init?: RequestInit): Promise<T> {
  if (mock !== null) return (await import('./mock')).mockApi(path, init, mock) as T;
  const response = await fetch(path, { ...init, headers: { 'Content-Type': 'application/json', ...init?.headers } });
  const text = await response.text();
  if (!response.ok) throw new Error(t(text || response.statusText));
  return JSON.parse(text);
}

export const post = <T = any>(path: string, body?: unknown, method = 'POST') => api<T>(path, { method, body: body === undefined ? undefined : JSON.stringify(body) });

export type Page<T> = { total: number; rows: T[] };

export function toast(message: string) { window.dispatchEvent(new CustomEvent('mymeta-toast', { detail: message })); }

export const del = (path: string) => api(path, { method: 'DELETE' });
