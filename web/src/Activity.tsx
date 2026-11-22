import { useState } from 'react';
import { Page, post, toast } from './api';
import { go } from './App';
import { ChangeTable, openObject, TableState, useApi } from './Catalog';
import { confirmAction } from './confirmAction';
import { label, utc } from './format';
import { t } from './i18n';
import { SqlModal } from './ObjectPage';
import { Pagination } from './Pagination';

const kinds = ['new_object', 'new_column', 'deleted_object', 'deleted_column', 'comment_conflict', 'check_downstream', 'possible_rename', 'tmp_unrecognized', 'tmp_drop_failed'];

export function TodosPage() {
  const [status, setStatus] = useState('open');
  const [kind, setKind] = useState('');
  const [page, setPage] = useState(0);
  const [version, setVersion] = useState(0);
  const [busy, setBusy] = useState<number | null>(null);
  const [sql, setSql] = useState<string | null>(null);
  const list = useApi<Page<any>>(`/api/todos?status=${status}&kind=${kind}&page=${page}`, version);
  const rows = list.data?.rows || [];
  const run = async (id: number, action: () => Promise<unknown>, message: string) => {
    setBusy(id);
    try { await action(); toast(message); setVersion(v => v + 1); }
    catch (error) { toast((error as Error).message); }
    finally { setBusy(null); }
  };
  const change = (id: number, next: string) => run(id, () => post(`/api/todos/${id}`, { status: next }), t('已标记为{0}', label(next)));
  const rename = async (row: any) => {
    if (!await confirmAction(t('确认 {0} 是 {1} 改名而来？业务信息、字段含义、规则、关联、流程和指标引用会迁移到新对象。', row.key.split('.').pop(), row.reason.split('.').pop()))) return;
    run(row.id, () => post('/api/rename', { from: row.reason, to: row.key, todo: row.id }), t('已迁移人工内容'));
  };
  const conflict = (row: any, keep: string) => run(row.id, async () => { const r = await post<{ sql: string }>('/api/comment-conflict', { key: row.key, keep, todo: row.id }); if (keep === 'tool') setSql(r.sql); }, t('已处理'));
  const objectKey = (key: string) => key.split('.').slice(0, 3).join('.');
  const actions = (row: any) => {
    const disabled = busy === row.id;
    if (status !== 'open') return <button disabled={disabled} onClick={() => change(row.id, 'open')}>{t('重新打开')}</button>;
    const extra = row.kind === 'possible_rename' ? <button disabled={disabled} onClick={() => rename(row)}>{t('确认改名')}</button>
      : row.kind === 'comment_conflict' ? <><button disabled={disabled} onClick={() => conflict(row, 'db')}>{t('以数据库为准')}</button><button disabled={disabled} onClick={() => conflict(row, 'tool')}>{t('以工具为准')}</button></>
      : row.kind === 'check_downstream' ? <button onClick={() => go('lineage?key=' + encodeURIComponent(objectKey(row.key)))}>{t('查看下游')}</button>
      : null;
    return <>{extra}{row.kind !== 'comment_conflict' && <button disabled={disabled} onClick={() => change(row.id, 'done')}>{t('完成')}</button>}<button disabled={disabled} onClick={() => change(row.id, 'ignored')}>{t('忽略')}</button></>;
  };
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('待办')}</h2></header>
    <div className="toolbar-row">
      <div className="segmented" role="tablist">
        {['open', 'done', 'ignored'].map(value => <button key={value} role="tab" aria-selected={status === value} onClick={() => { setStatus(value); setPage(0); }}>{label(value)}</button>)}
      </div>
      <select value={kind} onChange={event => { setKind(event.target.value); setPage(0); }} aria-label={t('待办类型')}>
        <option value="">{t('全部类型')}</option>
        {kinds.map(value => <option key={value} value={value}>{label(value)}</option>)}
      </select>
    </div>
    <table className="reports-table todo-table">
      <thead><tr><th>{t('待办类型')}</th><th>{t('对象 / 字段')}</th><th>{t('说明')}</th><th>{status === 'open' ? t('产生时间') : t('处理时间')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={5} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map(row => <tr key={row.id}>
          <td>{label(row.kind)}</td>
          <td title={row.key}><button className="report-name" onClick={() => openObject(objectKey(row.key), row.key.split('.').length > 3 ? 'columns' : '')}>{row.key.split('.').slice(2).join('.')}</button></td>
          <td title={row.reason || ''}>{row.kind === 'possible_rename' ? t('原对象：{0}', row.reason?.split('.').pop()) : row.reason ? label(row.reason) : ''}</td>
          <td>{utc(status === 'open' ? row.created_at : row.handled_at)}</td>
          <td><div className="report-actions">{actions(row)}</div></td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={list.data?.total || 0} onChange={setPage} disabled={list.loading} />
    {sql !== null && <SqlModal title={t('COMMENT ON 语句')} sql={sql} onClose={() => setSql(null)} />}
  </main>;
}

export function ChangesPage({ run }: { run: string | null }) {
  const [page, setPage] = useState(0);
  const list = useApi<Page<any>>(`/api/changes?page=${page}${run ? '&run=' + run : ''}`);
  return <main className="reports-page">
    <header className="reports-heading">
      <h2>{run ? t('同步 #{0} 的差异报告', run) : t('变更历史')}</h2>
      {run && <button onClick={() => go('changes')}>{t('查看全部变更')}</button>}
    </header>
    <ChangeTable data={list.data} loading={list.loading} error={list.error} showKey />
    <Pagination page={page} total={list.data?.total || 0} onChange={setPage} disabled={list.loading} />
  </main>;
}
