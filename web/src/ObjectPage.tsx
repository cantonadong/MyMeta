import { Fragment, useEffect, useState } from 'react';
import { api, del, post, toast } from './api';
import { go } from './App';
import { ChangeTable, TableState, useApi } from './Catalog';
import { bytes, isTemporary, label, number, utc } from './format';
import { Icon } from './Icon';
import { t } from './i18n';
import { Pagination } from './Pagination';
import { RelationList } from './Relations';
import { Field, Graph, Modal, Tabs, usePaged } from './ui';

const LAYERS = ['dim', 'dtl', 'fct', 'tpc', 'app', 'tmp'];
const BIZ_FIELDS = ['cn_name', 'description', 'product', 'layer', 'grain', 'primary_key', 'source_tables', 'refresh_method', 'maintainer', 'is_temporary', 'date_column'] as const;
type Detail = { object: any; biz: any; columns: any[]; rules: any[]; freshness: any; date_column: string | null; refreshed_by: any[]; counts: any };

export function SqlModal({ title, sql, onClose }: { title: string; sql: string; onClose: () => void }) {
  return <Modal title={title} wide onClose={onClose} actions={<><button onClick={onClose}>{t('关闭')}</button><button className="primary" disabled={!sql} onClick={() => navigator.clipboard.writeText(sql).then(() => toast(t('已复制')))}>{t('复制')}</button></>}>
    {sql ? <pre className="sql-block">{sql}</pre> : <p className="source-hint">{t('没有需要生成的语句：工具内的说明与数据库注释一致。')}</p>}
  </Modal>;
}

export function ObjectPage({ objectKey, tab: initialTab }: { objectKey: string; tab?: string | null }) {
  const [version, setVersion] = useState(0);
  const detail = useApi<Detail>('/api/object?key=' + encodeURIComponent(objectKey), version);
  const [tab, setTab] = useState(initialTab || 'overview');
  const [sql, setSql] = useState<{ title: string; sql: string } | null>(null);
  const reload = () => setVersion(v => v + 1);
  const object = detail.data?.object;
  const showSql = async (path: string, title: string) => {
    try { const r = await api<{ sql: string }>(path + '?key=' + encodeURIComponent(objectKey)); setSql({ title, sql: r.sql }); }
    catch (e) { toast((e as Error).message); }
  };
  return <main className="reports-page">
    <header className="reports-heading">
      <h2 className="object-title"><button className="icon-button" onClick={() => history.length > 1 ? history.back() : go('catalog')} aria-label={t('返回')} title={t('返回')}><Icon name="back" /></button>
        {object?.name || objectKey}
        {detail.data?.biz?.cn_name && <small className="title-sub">{detail.data.biz.cn_name}</small>}
        {object?.is_deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}{object && isTemporary(object.name) && <span className="tag">{t('临时')}</span>}
      </h2>
      {object && <div className="report-actions">
        <button onClick={() => showSql('/api/sql-template', t('标准取数 SQL'))}>{t('标准取数 SQL')}</button>
        <button onClick={() => showSql('/api/comment-sql', t('COMMENT ON 语句'))}>{t('生成 COMMENT ON')}</button>
      </div>}
    </header>
    {detail.error && <p role="alert" className="source-error">{detail.error}</p>}
    {detail.loading && !object && <p className="source-hint">{t('加载中…')}</p>}
    {object && detail.data && <>
      <div className="toolbar-row"><Tabs value={tab} onChange={setTab} tabs={[['overview', t('概览')], ['columns', t('字段')], ['rules', t('规则与取数')], ['lineage', t('血缘')], ['relations', t('关联')], ['history', t('变更历史')]]} /></div>
      {tab === 'overview' && <Overview detail={detail.data} reload={reload} />}
      {tab === 'columns' && <Columns detail={detail.data} reload={reload} />}
      {tab === 'rules' && <Rules detail={detail.data} reload={reload} version={version} />}
      {tab === 'lineage' && <ObjectLineage objectKey={objectKey} columns={detail.data.columns} />}
      {tab === 'relations' && <RelationList objectKey={objectKey} />}
      {tab === 'history' && <History objectKey={objectKey} />}
    </>}
    {sql && <SqlModal title={sql.title} sql={sql.sql} onClose={() => setSql(null)} />}
  </main>;
}

function Overview({ detail, reload }: { detail: Detail; reload: () => void }) {
  const { object, biz, freshness } = detail;
  const naming = object.naming || {};
  const [form, setForm] = useState<Record<string, any>>({});
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => setForm(Object.fromEntries(BIZ_FIELDS.map(f => [f, biz?.[f] ?? '']))), [biz]);
  // Parsed naming values and the database comment are shown as defaults until overridden.
  const defaults: Record<string, string | null | undefined> = { product: naming.product, layer: naming.layer, grain: naming.grain, maintainer: naming.maintainer, description: object.comment, primary_key: object.props?.primary_key, date_column: detail.date_column, is_temporary: naming.temporary ? '1' : '0' };
  const save = async () => {
    setBusy(true);
    try { await post('/api/m/object_biz', { key: object.key, ...form }, 'PUT'); toast(t('已保存')); setEditing(false); reload(); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const checkFreshness = async () => {
    setBusy(true);
    try { await post('/api/freshness', { key: object.key }); reload(); } catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const value = (f: string) => {
    const v = biz?.[f];
    if (v !== null && v !== undefined && v !== '') return f === 'is_temporary' ? label(String(v === 1)) : f === 'layer' ? label(v) : String(v);
    const d = defaults[f];
    if (!d) return '—';
    return <span className="inherited" title={t('自动识别，可编辑覆盖')}>{f === 'is_temporary' ? label(String(d === '1')) : f === 'layer' ? label(d) : d}</span>;
  };
  const input = (f: string) => f === 'layer'
    ? <select value={form[f]} onChange={e => setForm({ ...form, [f]: e.target.value })}><option value="">{t('自动')}{defaults.layer ? `（${defaults.layer}）` : ''}</option>{LAYERS.map(l => <option key={l} value={l}>{label(l)}</option>)}</select>
    : f === 'is_temporary' ? <select value={String(form[f])} onChange={e => setForm({ ...form, [f]: e.target.value === '' ? '' : Number(e.target.value) })}><option value="">{t('自动')}</option><option value="1">{t('是')}</option><option value="0">{t('否')}</option></select>
    : f === 'description' ? <textarea rows={3} value={form[f]} placeholder={defaults[f] || ''} onChange={e => setForm({ ...form, [f]: e.target.value })} />
    : <input value={form[f]} placeholder={f === 'date_column' ? (defaults[f] || t('未识别；填 - 表示不检查')) : defaults[f] || ''} onChange={e => setForm({ ...form, [f]: e.target.value })} />;
  const props = Object.entries(object.props || {}).filter(([k, v]) => v !== null && v !== '' && !['last_refresh', 'auto_refresh'].includes(k));
  const propValue = (key: string, v: unknown) => key === 'size_bytes' ? bytes(v) : key === 'rows' || key === 'rows_estimate' ? number(v) : typeof v === 'boolean' ? label(String(v)) : String(v);
  return <>
    <section className="settings-card">
      <div className="card-heading"><h3>{t('业务信息')}</h3>
        {editing ? <div className="report-actions"><button onClick={() => setEditing(false)}>{t('取消')}</button><button className="primary" disabled={busy} onClick={save}>{t('保存')}</button></div> : <button onClick={() => setEditing(true)}>{t('编辑')}</button>}
      </div>
      {editing
        ? <div className="form-grid">{BIZ_FIELDS.map(f => <Field key={f} label={label(f)} wide={f === 'description'}>{input(f)}</Field>)}</div>
        : <dl className="detail-grid">{BIZ_FIELDS.map(f => <Fragment key={f}><dt>{label(f)}</dt><dd className={f === 'description' ? 'pre' : ''}>{value(f)}</dd></Fragment>)}</dl>}
      {naming.issues?.length > 0 && <div className="naming-issues"><strong>{t('命名检查')}</strong>{naming.issues.map((i: string) => <span key={i} className="tag tag-warn">{label(i)}</span>)}</div>}
    </section>
    <section className="settings-card">
      <div className="card-heading"><h3>{t('数据日期与刷新')}</h3><button disabled={busy || object.kind === 'view'} onClick={checkFreshness}>{busy ? <><span className="busy-spinner" />{t('检查中…')}</> : t('立即检查')}</button></div>
      <dl className="detail-grid">
        <dt>{t('日期字段')}</dt><dd>{freshness?.date_column || detail.date_column || '—'}</dd>
        <dt>{t('最新数据日期')}</dt><dd>{freshness?.max_date || '—'}{freshness?.delayed ? <span className="tag tag-danger">{t('未按 T-1 更新')}</span> : freshness?.max_date ? <span className="tag tag-ok">{t('正常')}</span> : null}</dd>
        <dt>{t('应到日期（T-1）')}</dt><dd>{freshness?.expected || '—'}</dd>
        <dt>{t('检查时间')}</dt><dd>{utc(freshness?.checked_at)}{freshness?.error && <span className="source-error"> {freshness.error}</span>}</dd>
        {object.kind === 'mv' && <><dt>{t('自动刷新')}</dt><dd>{label(String(!!object.props?.auto_refresh))}</dd><dt>{t('最近刷新')}</dt><dd>{object.props?.last_refresh ? object.props.last_refresh.slice(0, 16) + ' UTC' : '—'}</dd></>}
        <dt>{t('刷新来源')}</dt><dd className="pre">{detail.refreshed_by.length ? detail.refreshed_by.map(r => `${label(r.source)}：${r.name || r.origin}`).join('\n') : object.kind === 'view' ? t('视图实时计算') : '—'}</dd>
      </dl>
    </section>
    <section className="settings-card">
      <h3>{t('技术元数据')}</h3>
      <dl className="detail-grid">
        <dt>{t('业务键')}</dt><dd>{object.key}</dd>
        <dt>{t('类型')}</dt><dd>{label(object.kind)}</dd>
        <dt>{t('数据库注释')}</dt><dd>{object.comment || '—'}</dd>
        <dt>{t('首次发现')}</dt><dd>{utc(object.first_seen)}</dd>
        <dt>{t('最近同步')}</dt><dd>{utc(object.synced_at)}</dd>
        <dt>{t('上下游')}</dt><dd>{t('上游 {0} · 下游 {1} · 关联 {2}', detail.counts?.upstream ?? 0, detail.counts?.downstream ?? 0, detail.counts?.relations ?? 0)}</dd>
        {props.map(([k, v]) => <Fragment key={k}><dt>{label(k)}</dt><dd className={k === 'indexes' ? 'pre' : ''}>{propValue(k, v)}</dd></Fragment>)}
      </dl>
      {object.definition && <><h3 className="section-heading">{t('定义 SQL')}</h3><pre className="sql-block">{object.definition}</pre></>}
    </section>
  </>;
}

function Columns({ detail, reload }: { detail: Detail; reload: () => void }) {
  const { rows, page, setPage, total } = usePaged(detail.columns);
  const [editing, setEditing] = useState<any>(null);
  return <>
    <table className="reports-table column-table">
      <thead><tr><th className="narrow">#</th><th>{t('字段')}</th><th>{t('字段类型')}</th><th>{t('业务含义')}</th><th>{t('单位 / 币种')}</th><th>{t('规则')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={7} loading={false} empty={!rows.length} />
        {rows.map(c => <tr key={c.key} className={c.is_deleted ? 'row-deleted' : ''}>
          <td>{c.ordinal}</td>
          <td>{c.name}{c.is_sensitive ? <span className="tag tag-warn">{t('敏感')}</span> : null}{c.is_deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}</td>
          <td>{c.data_type}</td>
          <td title={c.meaning || c.comment || ''}>{c.meaning || (c.comment ? <span className="inherited">{c.comment}</span> : '')}</td>
          <td>{[c.unit, c.currency].filter(Boolean).join(' · ')}</td>
          <td>{c.rule_count || ''}</td>
          <td><div className="report-actions"><button onClick={() => setEditing(c)}>{t('编辑')}</button></div></td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
    {editing && <ColumnModal column={editing} rules={detail.rules.filter(r => r.target_key === editing.key)} onClose={() => setEditing(null)} onSaved={reload} />}
  </>;
}

function ColumnModal({ column, rules, onClose, onSaved }: { column: any; rules: any[]; onClose: () => void; onSaved: () => void }) {
  const [form, setForm] = useState({ meaning: column.meaning ?? '', unit: column.unit ?? '', currency: column.currency ?? '', timezone: column.timezone ?? '', is_sensitive: !!column.is_sensitive, notes: column.notes ?? '' });
  const [busy, setBusy] = useState(false);
  const [sample, setSample] = useState<{ value: string | null; count: number; checked: boolean }[] | null>(null);
  const save = async () => {
    setBusy(true);
    try { await post('/api/m/column_biz', { key: column.key, ...form, is_sensitive: form.is_sensitive ? 1 : 0 }, 'PUT'); toast(t('已保存')); onSaved(); onClose(); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const loadSample = async () => {
    setBusy(true);
    try { const values = await post<any[]>('/api/sample', { key: column.key, n: 20 }); const existing = new Set(rules.filter(r => r.kind === 'valid').map(r => r.value)); setSample(values.map(v => ({ ...v, checked: v.value !== null && !existing.has(v.value) }))); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const importSample = async () => {
    setBusy(true);
    try {
      for (const v of sample!.filter(v => v.checked)) await post('/api/m/rule', { target_key: column.key, kind: 'valid', value: v.value ?? 'NULL', note: '' }, 'PUT');
      toast(t('已导入')); setSample(null); onSaved();
    } catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  return <Modal wide title={t('字段：{0}', column.name)} onClose={onClose} actions={<><button onClick={onClose}>{t('取消')}</button><button className="primary" disabled={busy} onClick={save}>{t('保存')}</button></>}>
    <p className="source-hint">{column.data_type}{column.comment ? ` · ${t('数据库注释')}：${column.comment}` : ''}</p>
    <div className="form-grid">
      <Field label={label('meaning')} wide><textarea rows={2} value={form.meaning} placeholder={column.comment || ''} onChange={e => setForm({ ...form, meaning: e.target.value })} /></Field>
      <Field label={label('unit')}><input value={form.unit} onChange={e => setForm({ ...form, unit: e.target.value })} /></Field>
      <Field label={label('currency')}><select value={form.currency} onChange={e => setForm({ ...form, currency: e.target.value })}><option value="">—</option><option value="原币种">{t('原币种')}</option><option value="USD">USD</option></select></Field>
      <Field label={label('timezone')}><input value={form.timezone} placeholder={t('如 UTC，按 UTC 0 点切日')} onChange={e => setForm({ ...form, timezone: e.target.value })} /></Field>
      <Field label={label('is_sensitive')}><input type="checkbox" checked={form.is_sensitive} onChange={e => setForm({ ...form, is_sensitive: e.target.checked })} /></Field>
      <Field label={label('notes')} wide><textarea rows={2} value={form.notes} onChange={e => setForm({ ...form, notes: e.target.value })} /></Field>
    </div>
    <div className="card-heading"><h3>{t('字段规则')}</h3><button disabled={busy} onClick={loadSample}>{t('采样 Top 20')}</button></div>
    {sample && <div className="sample-box">
      <p className="source-hint">{t('勾选要导入为有效值的取值：')}</p>
      <div className="sample-list">{sample.map((v, i) => <label key={i} className="check"><input type="checkbox" checked={v.checked} onChange={e => setSample(sample.map((x, j) => j === i ? { ...x, checked: e.target.checked } : x))} />{v.value ?? 'NULL'}<small>{number(v.count)}</small></label>)}</div>
      <div className="report-actions"><button onClick={() => setSample(null)}>{t('取消')}</button><button disabled={busy || !sample.some(v => v.checked)} onClick={importSample}>{t('导入为有效值')}</button></div>
    </div>}
    <RuleEditor target={column.key} rules={rules} onChanged={onSaved} kinds={['valid', 'invalid', 'filter', 'note']} />
  </Modal>;
}

/** List + add form for rules of one target (object or column). */
export function RuleEditor({ target, rules, onChanged, kinds }: { target: string; rules: any[]; onChanged: () => void; kinds: string[] }) {
  const [items, setItems] = useState(rules);
  useEffect(() => setItems(rules), [rules]);
  const [form, setForm] = useState({ kind: kinds[0], value: '', condition: '', note: '' });
  const add = async () => {
    if (form.kind === 'filter' && !form.condition.trim()) { toast(t('必加过滤需要填写 SQL 条件')); return; }
    if ((form.kind === 'valid' || form.kind === 'invalid') && !form.value.trim() && !form.condition.trim()) { toast(t('请填写取值或 SQL 条件')); return; }
    if (form.kind === 'note' && !form.note.trim()) { toast(t('请填写说明')); return; }
    try { const r = await post('/api/m/rule', { target_key: target, ...form }, 'PUT'); setItems([...items, { id: r.id, target_key: target, ...form }]); setForm({ ...form, value: '', condition: '', note: '' }); onChanged(); }
    catch (e) { toast((e as Error).message); }
  };
  const remove = async (id: number) => {
    try { await del('/api/m/rule/' + id); setItems(items.filter(r => r.id !== id)); onChanged(); } catch (e) { toast((e as Error).message); }
  };
  return <>
    <table className="reports-table rule-table">
      <thead><tr><th>{t('规则类型')}</th><th>{label('value')}</th><th>{label('condition')}</th><th>{t('说明')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={5} loading={false} empty={!items.length} />
        {items.map(r => <tr key={r.id}><td>{label(r.kind)}</td><td title={r.value || ''}>{r.value}</td><td title={r.condition || ''}><code>{r.condition}</code></td><td title={r.note || ''}>{r.note}</td><td><div className="report-actions"><button className="report-delete" onClick={() => remove(r.id)}>{t('删除')}</button></div></td></tr>)}
      </tbody>
    </table>
    <div className="rule-form">
      <select value={form.kind} onChange={e => setForm({ ...form, kind: e.target.value })}>{kinds.map(k => <option key={k} value={k}>{label(k)}</option>)}</select>
      {form.kind !== 'note' && form.kind !== 'filter' && <input value={form.value} placeholder={label('value')} onChange={e => setForm({ ...form, value: e.target.value })} />}
      {form.kind !== 'note' && <input value={form.condition} placeholder={form.kind === 'filter' ? t('如 is_test = 0') : t('SQL 条件（可选）')} onChange={e => setForm({ ...form, condition: e.target.value })} />}
      <input className="rule-note" value={form.note} placeholder={form.kind === 'filter' ? t('适用场景') : form.kind === 'invalid' ? t('原因') : t('说明')} onChange={e => setForm({ ...form, note: e.target.value })} />
      <button onClick={add}>{t('添加')}</button>
    </div>
  </>;
}

function Rules({ detail, reload, version }: { detail: Detail; reload: () => void; version: number }) {
  const key = detail.object.key;
  const tableRules = detail.rules.filter(r => r.target_key === key);
  const columnRules = detail.rules.filter(r => r.target_key !== key);
  const template = useApi<{ sql: string }>('/api/sql-template?key=' + encodeURIComponent(key), version);
  const { rows, page, setPage, total } = usePaged(columnRules);
  return <>
    <section className="settings-card">
      <h3>{t('表级规则')}</h3>
      <p className="source-hint">{t('必加过滤以 SQL 条件片段保存，会自动带入标准取数 SQL；注意事项用于去重方式、币种换算、已知数据问题等。')}</p>
      <RuleEditor target={key} rules={tableRules} onChanged={reload} kinds={['filter', 'note']} />
    </section>
    <section className="settings-card">
      <h3>{t('字段规则')}</h3>
      <table className="reports-table rule-table">
        <thead><tr><th>{t('字段')}</th><th>{t('规则类型')}</th><th>{label('value')}</th><th>{label('condition')}</th><th>{t('说明')}</th></tr></thead>
        <tbody>
          <TableState columns={5} loading={false} empty={!rows.length} />
          {rows.map(r => <tr key={r.id}><td>{r.target_key.slice(key.length + 1)}</td><td>{label(r.kind)}</td><td>{r.value}</td><td><code>{r.condition}</code></td><td>{r.note}</td></tr>)}
        </tbody>
      </table>
      <Pagination page={page} total={total} onChange={setPage} />
      <p className="source-hint">{t('字段规则在“字段”页签中编辑。')}</p>
    </section>
    <section className="settings-card">
      <div className="card-heading"><h3>{t('标准取数 SQL')}</h3><button disabled={!template.data} onClick={() => navigator.clipboard.writeText(template.data!.sql).then(() => toast(t('已复制')))}>{t('复制')}</button></div>
      {template.error ? <p role="alert" className="source-error">{template.error}</p> : <pre className="sql-block">{template.data?.sql || t('加载中…')}</pre>}
    </section>
  </>;
}

function ObjectLineage({ objectKey, columns }: { objectKey: string; columns: any[] }) {
  const [column, setColumn] = useState('');
  const graph = useApi<{ nodes: any[]; edges: any[]; truncated: boolean; column_edges: any[] }>(`/api/lineage/graph?key=${encodeURIComponent(objectKey)}&up=2&down=2&column=${encodeURIComponent(column)}`);
  return <>
    <div className="toolbar-row">
      <span className="source-hint">{t('上下游各展开 2 层；完整分析请打开血缘页。')}</span>
      <button onClick={() => go('lineage?key=' + encodeURIComponent(objectKey))}>{t('打开血缘页')}</button>
    </div>
    <section className="settings-card">
      {graph.error ? <p role="alert" className="source-error">{graph.error}</p> : graph.loading && !graph.data ? <p className="source-hint">{t('加载中…')}</p>
        : <Graph nodes={graph.data?.nodes || []} edges={graph.data?.edges || []} center={objectKey} onOpen={n => !n.external && go('object?key=' + encodeURIComponent(n.key) + '&tab=lineage')} />}
    </section>
    <section className="settings-card">
      <div className="card-heading"><h3>{t('字段级血缘')}</h3>
        <select value={column} onChange={e => setColumn(e.target.value)} aria-label={t('字段')}><option value="">{t('选择字段')}</option>{columns.filter(c => !c.is_deleted).map(c => <option key={c.name} value={c.name}>{c.name}</option>)}</select>
      </div>
      {column && <table className="reports-table">
        <thead><tr><th>{t('上游字段')}</th><th>{t('下游字段')}</th><th>{t('表达式')}</th></tr></thead>
        <tbody>
          <TableState columns={3} loading={graph.loading} empty={!graph.data?.column_edges.length} />
          {graph.data?.column_edges.map((e, i) => <tr key={i}><td>{e.upstream.split('.').pop()}.{e.upstream_column}</td><td>{e.downstream.split('.').pop()}.{e.downstream_column}</td><td title={e.expression || ''}><code>{e.expression || '—'}</code></td></tr>)}
        </tbody>
      </table>}
    </section>
  </>;
}

function History({ objectKey }: { objectKey: string }) {
  const [page, setPage] = useState(0);
  const changes = useApi<any>(`/api/changes?key=${encodeURIComponent(objectKey)}&page=${page}`);
  return <>
    <ChangeTable data={changes.data} loading={changes.loading} error={changes.error} showKey />
    <Pagination page={page} total={changes.data?.total || 0} onChange={setPage} disabled={changes.loading} />
  </>;
}
