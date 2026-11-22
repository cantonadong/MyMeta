import { useEffect, useState } from 'react';
import { api, Page } from './api';
import { go } from './App';
import { bytes, isTemporary, label, number, utc } from './format';
import { t } from './i18n';
import { Pagination } from './Pagination';

/** Loads `path` whenever it changes; keeps the previous rows visible while the next page loads. */
export function useApi<T>(path: string | null, version = 0) {
  const [state, setState] = useState<{ data?: T; loading: boolean; error?: string }>({ loading: true });
  useEffect(() => {
    if (!path) { setState({ loading: false }); return; }
    let live = true;
    setState(previous => ({ data: previous.data, loading: true }));
    api<T>(path).then(data => live && setState({ data, loading: false }), (error: Error) => live && setState({ loading: false, error: error.message }));
    return () => { live = false; };
  }, [path, version]);
  return state;
}

/** Empty / loading / error row for a table body. */
export function TableState({ columns, loading, error, empty }: { columns: number; loading: boolean; error?: string; empty: boolean }) {
  if (error) return <tr><td colSpan={columns} className="reports-empty"><p role="alert" className="source-error">{error}</p></td></tr>;
  if (loading && empty) return <tr><td colSpan={columns} className="reports-empty">{t('加载中…')}</td></tr>;
  if (empty) return <tr><td colSpan={columns} className="reports-empty">{t('暂无数据')}</td></tr>;
  return null;
}

export const openObject = (key: string, tab = '') => go('object?key=' + encodeURIComponent(key) + (tab ? '&tab=' + tab : ''));

type ObjectRow = { key: string; scope: string; name: string; kind: string; comment: string | null; cn_name: string | null; layer: string | null; product: string | null; props: Record<string, any>; is_deleted: number; column_count: number };
type ColumnRow = { key: string; name: string; data_type: string; comment: string | null; meaning: string | null; is_deleted: number; object_key: string; object_name: string; scope: string };

// Filters survive navigating to an object and back.
const saved = { tab: 'objects', q: '', scope: '', kind: '', product: '', layer: '', tmp: false, deleted: false, page: 0 };

export function CatalogPage() {
  const [filters, setFilters] = useState(saved);
  const [q, setQ] = useState(saved.q);
  const update = (change: Partial<typeof saved>) => setFilters(current => ({ ...Object.assign(saved, current, { page: 0 }, change) }));
  useEffect(() => { const timer = setTimeout(() => q !== filters.q && update({ q }), 250); return () => clearTimeout(timer); }, [q]);
  const status = useApi<any>('/api/sync/status');
  const settings = useApi<any>('/api/settings');
  const scopes: string[] = (status.data?.scopes || []).map((s: any) => s.label);
  const products: string[] = [...new Set(Object.values(settings.data?.naming?.products || {}) as string[])];
  const query = new URLSearchParams({ q: filters.q, scope: filters.scope, kind: filters.kind, product: filters.product, layer: filters.layer, tmp: String(filters.tmp), deleted: String(filters.deleted), page: String(filters.page) });
  const objects = filters.tab === 'objects';
  const list = useApi<Page<any>>(`/api/${objects ? 'objects' : 'columns'}?${query}`);
  const rows = list.data?.rows || [];
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('数据目录')}</h2></header>
    <div className="toolbar-row">
      <input className="search-input" type="search" value={q} onChange={event => setQ(event.target.value)} placeholder={t('搜索表名、字段名、注释、中文名、枚举值')} aria-label={t('搜索')} autoFocus />
      <div className="segmented" role="tablist">
        {['objects', 'columns'].map(tab => <button key={tab} role="tab" aria-selected={filters.tab === tab} onClick={() => update({ tab })}>{t(tab === 'objects' ? '对象' : '字段')}</button>)}
      </div>
      <select value={filters.scope} onChange={event => update({ scope: event.target.value })} aria-label={t('范围')}>
        <option value="">{t('全部范围')}</option>
        {scopes.map(scope => <option key={scope}>{scope}</option>)}
      </select>
      <select value={filters.kind} onChange={event => update({ kind: event.target.value })} aria-label={t('对象类型')}>
        <option value="">{t('全部类型')}</option>
        {['table', 'view', 'mv'].map(kind => <option key={kind} value={kind}>{label(kind)}</option>)}
      </select>
      <select value={filters.product} onChange={event => update({ product: event.target.value })} aria-label={t('产品')}>
        <option value="">{t('全部产品')}</option>
        {products.map(p => <option key={p}>{p}</option>)}
      </select>
      <select value={filters.layer} onChange={event => update({ layer: event.target.value })} aria-label={t('层级')}>
        <option value="">{t('全部层级')}</option>
        {['dim', 'dtl', 'fct', 'tpc', 'app', 'tmp'].map(l => <option key={l} value={l}>{label(l)}</option>)}
      </select>
      <label className="check"><input type="checkbox" checked={filters.tmp} onChange={event => update({ tmp: event.target.checked })} />{t('显示临时对象')}</label>
      <label className="check"><input type="checkbox" checked={filters.deleted} onChange={event => update({ deleted: event.target.checked })} />{t('显示已删除')}</label>
    </div>
    <table className="reports-table catalog-table">
      <thead>{objects
        ? <tr><th>{t('名称')}</th><th>{t('中文名 / 注释')}</th><th>{t('范围')}</th><th>{t('类型')}</th><th>{t('层级')}</th><th>{t('字段数')}</th><th>{t('行数')}</th><th>{t('大小')}</th></tr>
        : <tr><th>{t('字段')}</th><th>{t('业务含义 / 注释')}</th><th>{t('字段类型')}</th><th>{t('所在对象')}</th><th>{t('范围')}</th></tr>}
      </thead>
      <tbody>
        <TableState columns={objects ? 8 : 5} loading={list.loading} error={list.error} empty={!rows.length} />
        {objects ? (rows as ObjectRow[]).map(row => <tr key={row.key} className={row.is_deleted ? 'row-deleted' : ''}>
          <td title={row.name}><button className="report-name" onClick={() => openObject(row.key)}>{row.name}</button>{isTemporary(row.name) && <span className="tag">{t('临时')}</span>}{row.is_deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}</td>
          <td title={row.cn_name || row.comment || ''}>{row.cn_name || (row.comment ? <span className="inherited">{row.comment}</span> : '')}</td>
          <td>{row.scope}</td><td>{label(row.kind)}</td><td>{row.layer || ''}</td><td>{row.column_count}</td>
          <td>{number(row.props?.rows ?? row.props?.rows_estimate)}</td><td>{bytes(row.props?.size_bytes)}</td>
        </tr>) : (rows as ColumnRow[]).map(row => <tr key={row.key} className={row.is_deleted ? 'row-deleted' : ''}>
          <td>{row.name}</td>
          <td title={row.meaning || row.comment || ''}>{row.meaning || (row.comment ? <span className="inherited">{row.comment}</span> : '')}</td>
          <td>{row.data_type}</td>
          <td title={row.object_name}><button className="report-name" onClick={() => openObject(row.object_key, 'columns')}>{row.object_name}</button></td>
          <td>{row.scope}</td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={filters.page} total={list.data?.total || 0} onChange={page => setFilters(current => ({ ...Object.assign(saved, current, { page }) }))} disabled={list.loading} />
  </main>;
}

export function ChangeTable({ data, loading, error, showKey }: { data?: Page<any>; loading: boolean; error?: string; showKey?: boolean }) {
  const rows = data?.rows || [];
  const value = (field: string, text: string | null) => ['status', 'kind', 'nullable', 'layer'].includes(field) ? label(text) : text ?? '—';
  return <table className="reports-table change-table">
    <thead><tr><th>{t('时间')}</th>{showKey && <th>{t('对象 / 字段')}</th>}<th>{t('变更项')}</th><th>{t('修改前')}</th><th>{t('修改后')}</th><th>{t('来源')}</th></tr></thead>
    <tbody>
      <TableState columns={showKey ? 6 : 5} loading={loading} error={error} empty={!rows.length} />
      {rows.map(row => {
        const parts = row.key.split('.');
        const isObjectKey = parts.length >= 3 && !['system', 'rule', 'relation', 'flow_step', 'metric'].includes(row.entity);
        return <tr key={row.id}>
          <td>{utc(row.at)}</td>
          {showKey && <td title={row.key}>{isObjectKey ? <button className="report-name" onClick={() => openObject(parts.slice(0, 3).join('.'))}>{parts.slice(2).join('.')}</button> : row.key}</td>}
          <td>{label(row.entity)} · {label(row.field)}</td>
          <td title={row.before || ''}><span className="clip">{value(row.field, row.before)}</span></td>
          <td title={row.after || ''}><span className="clip">{value(row.field, row.after)}</span></td>
          <td>{label(row.source)}</td>
        </tr>;
      })}
    </tbody>
  </table>;
}
