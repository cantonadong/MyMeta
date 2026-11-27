export const defaultFilters = { tab: 'objects', q: '', scope: '', kind: '', layer: '', expanded: false };
export type CatalogFilters = typeof defaultFilters;
export type QuickFilters = { order: string[]; hidden: string[]; bottom: string[]; presets: { name: string; filters: CatalogFilters; terms: string[] }[] };

const storageKey = 'mymeta-catalog-filters' + (new URLSearchParams(location.search).has('mock') ? '-mock' : '');

export function readQuickFilters(): QuickFilters {
  try {
    const saved = JSON.parse(localStorage.getItem(storageKey) ?? '{}') as Partial<QuickFilters> & { pinned?: unknown };
    const presets = Array.isArray(saved.presets) ? saved.presets.flatMap(preset => {
      const filters = preset?.filters;
      const fields = Object.entries(defaultFilters);
      if (typeof preset?.name !== 'string' || !preset.name.trim() || !filters || !['objects', 'columns'].includes(filters.tab) ||
        !fields.every(([key, value]) => typeof filters[key as keyof CatalogFilters] === typeof value)) return [];
      const terms = Array.isArray(preset.terms) ? cleanKeys(preset.terms) : filters.q.trim() ? [filters.q.trim()] : [];
      return [{ name: preset.name, filters: { ...Object.fromEntries(fields.map(([key]) => [key, filters[key as keyof CatalogFilters]])), q: '' } as CatalogFilters, terms }];
    }) : [];
    const order = [...new Set([...cleanKeys(saved.pinned), ...cleanKeys(saved.order)])];
    return { order, hidden: cleanKeys(saved.hidden), bottom: cleanKeys(saved.bottom), presets };
  } catch { return { order: [], hidden: [], bottom: [], presets: [] }; }
}

const cleanKeys = (value: unknown) => Array.isArray(value) ? [...new Set(value.filter((key): key is string => typeof key === 'string' && !!key))] : [];

export function writeQuickFilters(preferences: QuickFilters) {
  localStorage.setItem(storageKey, JSON.stringify(preferences));
}

export function tableStructureTsv(table: string, headers: string[], columns: {
  name: string; data_type: string; sample: string | null; is_deleted: number; is_sensitive?: number | null;
}[]) {
  const rows = columns.filter(column => !column.is_deleted).map(column =>
    [table, column.name, column.data_type, column.is_sensitive ? '' : column.sample ?? '']);
  if (!rows.length) rows.push([table, '', '', '']);
  return [headers, ...rows].map(row => row.map(value => /[\t\r\n"]/.test(value) ? '"' + value.replaceAll('"', '""') + '"' : value).join('\t')).join('\n');
}

export async function matchingObjects(loadPage: (page: number) => Promise<{ total: number; rows: { key: string; object_key?: string }[] }>) {
  const keys = new Set<string>();
  for (let page = 0; ; page++) {
    const data = await loadPage(page);
    for (const row of data.rows) keys.add(row.object_key ?? row.key);
    if (!data.rows.length || (page + 1) * 10 >= data.total) return keys;
  }
}

export function searchPattern(query: string): RegExp | undefined {
  const words = [...new Set(query.split(/\s+/).filter(Boolean))];
  if (!words.length) return;
  const literals = words.sort((a, b) => b.length - a.length).map(word => word.replace(/[.*+?^$(){}|[\]\\]/g, '\\$&'));
  return new RegExp('(' + literals.join('|') + ')', 'giu');
}

// Replace the visible ranks in place so filtering does not shuffle hidden objects.
export function moveObject<T extends { key: string }>(previous: string[], rows: T[], index: number, target: number) {
  if (index === target || !rows[index] || !rows[target]) return { order: previous, rows };
  const reordered = [...rows];
  const [moved] = reordered.splice(index, 1);
  reordered.splice(target, 0, moved);
  const prefix = reordered.slice(0, Math.max(index, target) + 1).map(row => row.key);
  const visible = new Set(prefix);
  let next = 0;
  const order = previous.map(key => visible.has(key) ? prefix[next++] : key);
  return { order: [...order, ...prefix.slice(next)], rows: reordered };
}
