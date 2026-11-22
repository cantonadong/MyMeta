import { useState } from 'react';
import { del, post, toast } from './api';
import { openObject, TableState, useApi } from './Catalog';
import { confirmAction } from './confirmAction';
import { t } from './i18n';
import { Pagination } from './Pagination';
import { Field, Modal, ObjectPicker, usePaged } from './ui';

type Metric = { id?: number; name_cn: string; name_en?: string | null; definition?: string | null; formula?: string | null; numerator?: string | null; denominator?: string | null; object_key?: string | null; column_name?: string | null; used_by?: string[] | null; builtin?: number };

/** Metric definitions (F7): one authoritative definition per metric, where it is defined and who uses it. */
export function MetricsPage() {
  const [version, setVersion] = useState(0);
  const [filter, setFilter] = useState('');
  const [editing, setEditing] = useState<Metric | null>(null);
  const list = useApi<Metric[]>('/api/m/metric', version);
  const items = (list.data || []).filter(m => !filter || [m.name_cn, m.name_en, m.definition, m.formula].some(v => v?.toLowerCase().includes(filter.toLowerCase())));
  const { rows, page, setPage, total } = usePaged(items);
  const reload = () => setVersion(v => v + 1);
  const remove = async (m: Metric) => {
    if (!await confirmAction(t('删除指标“{0}”？', m.name_cn))) return;
    try { await del('/api/m/metric/' + m.id); reload(); } catch (e) { toast((e as Error).message); }
  };
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('指标口径')}</h2><button className="primary" onClick={() => setEditing({ name_cn: '' })}>{t('新建指标')}</button></header>
    <div className="toolbar-row"><input type="search" className="search-input" value={filter} placeholder={t('搜索指标名、定义、公式')} onChange={e => setFilter(e.target.value)} /></div>
    <table className="reports-table metric-table">
      <thead><tr><th>{t('指标')}</th><th>{t('定义')}</th><th>{t('计算公式')}</th><th>{t('定义位置')}</th><th>{t('使用方')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={6} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map(m => <tr key={m.id}>
          <td title={m.name_en || ''}>{m.name_cn}{m.name_en && <small className="muted"> {m.name_en}</small>}{m.builtin ? <span className="tag">{t('内置')}</span> : null}</td>
          <td title={m.definition || ''}>{m.definition}</td>
          <td title={m.formula || ''}><code>{m.formula}</code></td>
          <td title={m.object_key || ''}>{m.object_key ? <button className="report-name" onClick={() => openObject(m.object_key!)}>{m.object_key.split('.').pop()}{m.column_name ? '.' + m.column_name : ''}</button> : '—'}</td>
          <td title={(m.used_by || []).join('\n')}>{(m.used_by || []).length || '—'}</td>
          <td><div className="report-actions"><button onClick={() => setEditing(m)}>{t('编辑')}</button><button className="report-delete" onClick={() => remove(m)}>{t('删除')}</button></div></td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
    {editing && <MetricModal metric={editing} onClose={() => setEditing(null)} onSaved={reload} />}
  </main>;
}

function MetricModal({ metric, onClose, onSaved }: { metric: Metric; onClose: () => void; onSaved: () => void }) {
  const [form, setForm] = useState<Metric>({ ...metric, used_by: metric.used_by || [] });
  const [user, setUser] = useState('');
  const [busy, setBusy] = useState(false);
  const columns = useApi<any>(form.object_key ? '/api/object?key=' + encodeURIComponent(form.object_key) : null);
  const reports = useApi<any[]>('/api/refresh/mybi');
  const set = (k: keyof Metric, v: any) => setForm({ ...form, [k]: v });
  const save = async () => {
    if (!form.name_cn.trim()) { toast(t('指标中文名不能为空')); return; }
    setBusy(true);
    try { const { builtin, ...body } = form; void builtin; await post('/api/m/metric', body, 'PUT'); toast(t('已保存')); onSaved(); onClose(); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const addUser = (value: string) => { if (value && !form.used_by!.includes(value)) set('used_by', [...form.used_by!, value]); setUser(''); };
  return <Modal wide title={metric.id ? t('编辑指标') : t('新建指标')} onClose={onClose} actions={<><button onClick={onClose}>{t('取消')}</button><button className="primary" disabled={busy} onClick={save}>{t('保存')}</button></>}>
    <div className="form-grid">
      <Field label={t('中文名')}><input value={form.name_cn} onChange={e => set('name_cn', e.target.value)} /></Field>
      <Field label={t('英文名')}><input value={form.name_en || ''} onChange={e => set('name_en', e.target.value)} /></Field>
      <Field label={t('定义')} wide><textarea rows={2} value={form.definition || ''} onChange={e => set('definition', e.target.value)} /></Field>
      <Field label={t('计算公式')} wide><input value={form.formula || ''} onChange={e => set('formula', e.target.value)} /></Field>
      <Field label={t('分子')}><input value={form.numerator || ''} onChange={e => set('numerator', e.target.value)} /></Field>
      <Field label={t('分母')}><input value={form.denominator || ''} onChange={e => set('denominator', e.target.value)} /></Field>
      <Field label={t('定义所在对象')}><ObjectPicker value={form.object_key || ''} onChange={key => setForm({ ...form, object_key: key, column_name: null })} /></Field>
      <Field label={t('定义所在字段')}><select value={form.column_name || ''} onChange={e => set('column_name', e.target.value)}><option value="">—</option>{(columns.data?.columns || []).map((c: any) => <option key={c.name}>{c.name}</option>)}</select></Field>
    </div>
    <h3 className="section-heading">{t('使用方（对象或 MyBI 数据集）')}</h3>
    <div className="chip-list">{form.used_by!.map(u => <span key={u} className="tag">{u.startsWith('mybi:') ? (reports.data || []).find(r => r.key === u)?.name || u : u.split('.').pop()}<button className="chip-remove" aria-label={t('移除')} onClick={() => set('used_by', form.used_by!.filter(x => x !== u))}>×</button></span>)}</div>
    <div className="scope-row">
      <ObjectPicker value={user} onChange={addUser} placeholder={t('添加使用该指标的对象')} />
      <select value="" onChange={e => addUser(e.target.value)} aria-label={t('MyBI 数据集')}><option value="">{t('添加 MyBI 数据集')}</option>{(reports.data || []).filter(r => r.kind === 'mybi_dataset').map(r => <option key={r.key} value={r.key}>{r.name}</option>)}</select>
    </div>
  </Modal>;
}
