import { useSyncExternalStore } from 'react';
import english from './locales/en.json';

// Adapted from MyBI's shared localization module: Simplified Chinese is the source text, English comes from en.json.
export type Language = 'auto' | 'zh-CN' | 'en';
const key = 'mymeta.language';
function readLanguage(): Language {
  try { const value = localStorage.getItem(key); return value === 'zh-CN' || value === 'en' ? value : 'auto'; }
  catch { return 'auto'; }
}
let language = readLanguage();
const listeners = new Set<() => void>();
export function getLocale(): 'zh-CN' | 'en' {
  if (language !== 'auto') return language;
  const preferred = (navigator.languages || [navigator.language]).find(value => /^(zh|en)(-|$)/i.test(value)) || navigator.language;
  return /^zh(-|$)/i.test(preferred) ? 'zh-CN' : 'en';
}
function notify() { document.documentElement.lang = getLocale(); listeners.forEach(listener => listener()); }
export function setLanguage(value: Language) {
  try { localStorage.setItem(key, value); } catch { /* private mode: keep for this session only */ }
  language = value; notify();
}
window.addEventListener('languagechange', notify);
window.addEventListener('storage', event => { if (event.key === key || event.key === null) { language = readLanguage(); notify(); } });
document.documentElement.lang = getLocale();
export function useLanguage() {
  useSyncExternalStore(listener => { listeners.add(listener); return () => { listeners.delete(listener); }; }, () => language + ':' + getLocale());
  return { language, locale: getLocale() };
}
export function t(text: string, ...values: unknown[]): string {
  const translated = getLocale() === 'en' ? (english as Record<string, string>)[text] ?? text : text;
  return translated.replace(/\{(\d+)\}/g, (_, index: string) => String(values[Number(index)] ?? ''));
}
