import { useEffect, useState } from 'react';
import { api, del, post, toast } from './api';
import { openObject, TableState, useApi } from './Catalog';
import { confirmAction } from './confirmAction';
import { label } from './format';
import { t } from './i18n';
import { Pagination } from './Pagination';
import { Field, Graph, Modal, ObjectPicker, Tabs, usePaged } from './ui';

type Relation = { id?: number; left_key: string; right_key: string; key_pairs: [string, string][]; kind?: string | null; note?: string | null; diff_note?: string | null; status?: string; left_name?: string; right_name?: string; cross_db?: number };
const KINDS = ['1:1', '1:n', 'n:n'];
const short = (key: string) => key.split('.').slice(-2).join('.');

function useColumns(key: string) {
  const [columns, setColumns] = useState<string[]>([]);
  useEffect(() => { if (!key) { setColumns([]); return; } api<any>('/api/object?key=' + encodeURIComponent(key)).then(d => setColumns(d.columns.filter((c: any) => !c.is_deleted).map((c: any) => c.name)), () => setColumns([])); }, [key]);
  return columns;
}

/** Create / edit a relation; key pairs are stored as full column keys 库.模式.表.字段 (supports cross-database pairs). */
export function RelationModal({ relation, onClose, onSaved }: { relation: Relation; onClose: () => void; onSaved: () => void }) {
  const [form, setForm] = useState<Relation>({ ...relation, key_pairs: relation.key_pairs?.length ? relation.key_pairs : [['', '']] });
  const leftColumns = useColumns(form.left_key), rightColumns = useColumns(form.right_key);
  const [busy, setBusy] = useState(false);
  const cross = form.left_key && form.right_key && form.left_key.split('.').slice(0, 2).join('.') !== form.right_key.split('.').slice(0, 2).join('.');
  const column = (key: string) => key ? key.split('.').pop() : '';
  const setPair = (i: number, side: 0 | 1, col: string) => setForm({ ...form, key_pairs: form.key_pairs.map((p, j) => j === i ? (side === 0 ? [`${form.left_key}.${col}`, p[1]] : [p[0], `${form.right_key}.${col}`]) as [string, string] : p) });
  const save = async (status = form.status || 'confirmed') => {
    const pairs = form.key_pairs.filter(p => column(p[0]) && column(p[1]));
    if (!form.left_key || !form.right_key || !pairs.length) { toast(t('请选择两张表并至少填写一组关联键')); return; }
    setBusy(true);
    try {
      await post('/api/m/relation', { id: form.id, left_key: form.left_key, right_key: form.right_key, key_pairs: pairs, kind: form.kind || null, note: form.note || null, diff_note: form.diff_note || null, status }, 'PUT');
      toast(t('已保存')); onSaved(); onClose();
    } catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  return <Modal wide title={form.id ? (form.status === 'suggested' ? t('确认推荐关联') : t('编辑关联')) : t('新建关联')} onClose={onClose}
    actions={<><button onClick={onClose}>{t('取消')}</button>{form.status === 'suggested' && <button disabled={busy} onClick={() => save('rejected')}>{t('拒绝')}</button>}<button className="primary" disabled={busy} onClick={() => save(form.status === 'suggested' ? 'confirmed' : undefined)}>{form.status === 'suggested' ? t('确认') : t('保存')}</button></>}>
    <div className="form-grid">
      <Field label={t('左表')}><ObjectPicker value={form.left_key} onChange={key => setForm({ ...form, left_key: key, key_pairs: form.key_pairs.map(p => ['', p[1]]) })} /></Field>
      <Field label={t('右表')}><ObjectPicker value={form.right_key} onChange={key => setForm({ ...form, right_key: key, key_pairs: form.key_pairs.map(p => [p[0], '']) })} /></Field>
    </div>
    {cross && <p className="source-hint"><span className="tag tag-warn">{t('跨库')}</span> {t('两张表在不同库，请在“口径差异”中说明差异。')}</p>}
    <h3 className="section-heading">{t('关联键')}</h3>
    {form.key_pairs.map((pair, i) => <div key={i} className="scope-row">
      <select value={column(pair[0])} onChange={e => setPair(i, 0, e.target.value)}><option value="">{t('左表字段')}</option>{leftColumns.map(c => <option key={c}>{c}</option>)}</select>
      <span className="pair-eq">=</span>
      <select value={column(pair[1])} onChange={e => setPair(i, 1, e.target.value)}><option value="">{t('右表字段')}</option>{rightColumns.map(c => <option key={c}>{c}</option>)}</select>
      <button className="report-delete" disabled={form.key_pairs.length === 1} onClick={() => setForm({ ...form, key_pairs: form.key_pairs.filter((_, j) => j !== i) })}>{t('移除')}</button>
    </div>)}
    <button onClick={() => setForm({ ...form, key_pairs: [...form.key_pairs, ['', '']] })}>{t('添加关联键')}</button>
    <div className="form-grid">
      <Field label={t('关系类型')}><select value={form.kind || ''} onChange={e => setForm({ ...form, kind: e.target.value })}><option value="">—</option>{KINDS.map(k => <option key={k} value={k}>{label(k)}</option>)}</select></Field>
      <Field label={t('关联说明')} wide><textarea rows={2} value={form.note || ''} onChange={e => setForm({ ...form, note: e.target.value })} /></Field>
      <Field label={label('diff_note')} wide><textarea rows={2} value={form.diff_note || ''} onChange={e => setForm({ ...form, diff_note: e.target.value })} /></Field>
    </div>
  </Modal>;
}

function RelationTable({ items, loading, error, onEdit, onDelete }: { items: Relation[]; loading: boolean; error?: string; onEdit: (r: Relation) => void; onDelete: (r: Relation) => void }) {
  const { rows, page, setPage, total } = usePaged(items);
  return <>
    <table className="reports-table relation-table">
      <thead><tr><th>{t('左表')}</th><th>{t('右表')}</th><th>{t('关联键')}</th><th>{t('关系类型')}</th><th>{t('说明 / 口径差异')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={6} loading={loading} error={error} empty={!rows.length} />
        {rows.map(r => <tr key={r.id}>
          <td title={r.left_key}><button className="report-name" onClick={() => openObject(r.left_key, 'relations')}>{r.left_name || short(r.left_key)}</button></td>
          <td title={r.right_key}><button className="report-name" onClick={() => openObject(r.right_key, 'relations')}>{r.right_name || short(r.right_key)}</button>{r.cross_db ? <span className="tag tag-warn">{t('跨库')}</span> : null}</td>
          <td title={r.key_pairs.map(p => p.join(' = ')).join('\n')}>{r.key_pairs.map(p => `${p[0].split('.').pop()} = ${p[1].split('.').pop()}`).join(', ')}</td>
          <td>{r.kind ? label(r.kind) : '—'}</td>
          <td title={[r.note, r.diff_note].filter(Boolean).join('\n')}>{[r.note, r.diff_note].filter(Boolean).join(' · ')}</td>
          <td><div className="report-actions"><button onClick={() => onEdit(r)}>{r.status === 'suggested' ? t('确认') : t('编辑')}</button><button className="report-delete" onClick={() => onDelete(r)}>{r.status === 'suggested' ? t('拒绝') : t('删除')}</button></div></td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
  </>;
}

function useRelationActions(reload: () => void) {
  return async (r: Relation) => {
    if (r.status === 'suggested') { await post('/api/m/relation', { id: r.id, status: 'rejected' }, 'PUT'); toast(t('已拒绝，不会再推荐')); reload(); return; }
    if (!await confirmAction(t('删除这条关联？'))) return;
    try { await del('/api/m/relation/' + r.id); toast(t('已删除')); reload(); } catch (e) { toast((e as Error).message); }
  };
}

/** Relations tab inside the object page: graph (1 or 2 degrees) and the list touching this object. */
export function RelationList({ objectKey }: { objectKey: string }) {
  const [version, setVersion] = useState(0);
  const [depth, setDepth] = useState('1');
  const [editing, setEditing] = useState<Relation | null>(null);
  const reload = () => setVersion(v => v + 1);
  const list = useApi<Relation[]>(`/api/relations?key=${encodeURIComponent(objectKey)}`, version);
  const suggested = useApi<Relation[]>(`/api/relations?status=suggested&key=${encodeURIComponent(objectKey)}`, version);
  const graph = useApi<{ nodes: any[]; edges: any[] }>(`/api/relations/graph?key=${encodeURIComponent(objectKey)}&depth=${depth}`, version);
  const remove = useRelationActions(reload);
  const edges = (graph.data?.edges || []).map(e => ({ from: e.left_key, to: e.right_key, label: e.kind || '' }));
  return <>
    <div className="toolbar-row">
      <Tabs value={depth} onChange={setDepth} tabs={[['1', t('一度关联')], ['2', t('二度关联')]]} />
      <button className="primary" onClick={() => setEditing({ left_key: objectKey, right_key: '', key_pairs: [] })}>{t('新建关联')}</button>
    </div>
    <section className="settings-card"><Graph nodes={(graph.data?.nodes || []).map(n => ({ ...n, depth: Math.abs(n.depth) }))} edges={edges} center={objectKey} onOpen={n => !n.external && openObject(n.key, 'relations')} /></section>
    <RelationTable items={list.data || []} loading={list.loading} error={list.error} onEdit={setEditing} onDelete={remove} />
    {!!suggested.data?.length && <><h3 className="section-heading">{t('推荐关联（来自 JOIN 条件）')}</h3><RelationTable items={suggested.data} loading={false} onEdit={setEditing} onDelete={remove} /></>}
    {editing && <RelationModal relation={editing} onClose={() => setEditing(null)} onSaved={reload} />}
  </>;
}

export function RelationsPage() {
  const [status, setStatus] = useState('confirmed');
  const [version, setVersion] = useState(0);
  const [editing, setEditing] = useState<Relation | null>(null);
  const [filter, setFilter] = useState('');
  const reload = () => setVersion(v => v + 1);
  const list = useApi<Relation[]>(`/api/relations?status=${status}`, version);
  const remove = useRelationActions(reload);
  const items = (list.data || []).filter(r => !filter || [r.left_key, r.right_key, r.note, r.diff_note].some(v => v?.toLowerCase().includes(filter.toLowerCase())));
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('关联关系')}</h2><button className="primary" onClick={() => setEditing({ left_key: '', right_key: '', key_pairs: [] })}>{t('新建关联')}</button></header>
    <p className="source-hint">{t('推荐关联来自 View、脚本和 MyBI 数据集中的 JOIN 条件，确认后生效；拒绝后不再推荐。')}</p>
    <div className="toolbar-row">
      <Tabs value={status} onChange={setStatus} tabs={[['confirmed', label('confirmed')], ['suggested', label('suggested')], ['rejected', label('rejected')]]} />
      <input type="search" className="search-input" value={filter} placeholder={t('筛选表名或说明')} onChange={e => setFilter(e.target.value)} />
    </div>
    <RelationTable items={items} loading={list.loading} error={list.error} onEdit={r => status === 'rejected' ? setEditing({ ...r, status: 'suggested' }) : setEditing(r)} onDelete={remove} />
    {editing && <RelationModal relation={editing} onClose={() => setEditing(null)} onSaved={reload} />}
  </main>;
}
