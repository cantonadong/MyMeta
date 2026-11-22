import { useState } from 'react';
import { Page } from './api';
import { go } from './App';
import { openObject, TableState, useApi } from './Catalog';
import { duration, label, utc } from './format';
import { t } from './i18n';
import { Pagination } from './Pagination';
import { Tabs, usePaged } from './ui';

const schedule = (s: any) => !s ? '—' : s.enabled === false ? t('未启用') : [s.time, s.timezone].filter(Boolean).join(' ') || '—';
const sourceText = (r: any) => `${label(r.source)}：${r.name || r.origin.split(/[\\/]/).pop()}`;

/** Refresh overview (F10): MyMeta only shows refresh scripts and tasks; execution stays in MyBI or manual runs. */
export function RefreshPage() {
  const [tab, setTab] = useState('objects');
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('刷新任务')}</h2></header>
    <p className="source-hint">{t('这里只展示刷新脚本和定时任务，不执行刷新；执行仍由 MyBI 定时任务或人工完成。脚本目录和 MyBI 路径在设置中配置。')}</p>
    <div className="toolbar-row"><Tabs value={tab} onChange={setTab} tabs={[['objects', t('按对象')], ['scripts', t('刷新脚本')], ['mybi', t('MyBI 任务')]]} /></div>
    {tab === 'objects' ? <ByObject /> : tab === 'scripts' ? <Scripts /> : <MyBITasks />}
  </main>;
}

function ByObject() {
  const [delayed, setDelayed] = useState(false);
  const [q, setQ] = useState('');
  const [page, setPage] = useState(0);
  const list = useApi<Page<any> & { expected: string }>(`/api/refresh/objects?delayed=${delayed}&q=${encodeURIComponent(q)}&page=${page}`);
  const rows = list.data?.rows || [];
  return <>
    <div className="toolbar-row">
      <input type="search" className="search-input" value={q} placeholder={t('筛选表名')} onChange={e => { setQ(e.target.value); setPage(0); }} />
      <label className="check"><input type="checkbox" checked={delayed} onChange={e => { setDelayed(e.target.checked); setPage(0); }} />{t('只看未按 T-1 更新的对象')}</label>
      {list.data?.expected && <span className="source-hint">{t('应到日期（T-1）')}：{list.data.expected}</span>}
    </div>
    <table className="reports-table">
      <thead><tr><th>{t('对象')}</th><th>{t('刷新来源')}</th><th>{t('刷新频率')}</th><th>{t('最近刷新')}</th><th>{t('最新数据日期')}</th></tr></thead>
      <tbody>
        <TableState columns={5} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map(r => {
          const tasks = r.sources.filter((s: any) => s.source === 'mybi_task');
          const lastRun = tasks.map((s: any) => s.info?.last_run_at).filter(Boolean).sort().pop();
          return <tr key={r.key}>
            <td title={r.key}><button className="report-name" onClick={() => openObject(r.key)}>{r.name}</button> <small className="muted">{label(r.kind)}</small></td>
            <td className="wrap">{r.sources.length ? r.sources.map(sourceText).join('；') : r.kind === 'mv' ? (r.auto_refresh ? t('MV 自动刷新') : t('MV 手动刷新')) : '—'}</td>
            <td>{tasks.length ? tasks.map((s: any) => schedule(s.info?.schedule)).join('；') : '—'}</td>
            <td>{lastRun ? utc(lastRun.slice(0, 19).replace(' ', 'T') + 'Z') : r.mv_last_refresh ? r.mv_last_refresh.slice(0, 16) + ' UTC' : '—'}</td>
            <td>{r.max_date || '—'}{r.delayed ? <span className="tag tag-danger">{t('未按 T-1 更新')}</span> : null}{r.freshness_error && <span className="tag tag-warn" title={r.freshness_error}>{t('检查失败')}</span>}</td>
          </tr>;
        })}
      </tbody>
    </table>
    <Pagination page={page} total={list.data?.total || 0} onChange={setPage} disabled={list.loading} />
  </>;
}

function Scripts() {
  const list = useApi<any[]>('/api/scripts');
  const { rows, page, setPage, total } = usePaged(list.data || []);
  const names = (keys: string[]) => keys.map(k => k.split('.').pop()).join(', ');
  return <>
    <table className="reports-table script-table">
      <thead><tr><th>{t('脚本')}</th><th>{t('写入对象')}</th><th>{t('读取上游')}</th><th>{t('修改时间')}</th><th>{t('解析')}</th></tr></thead>
      <tbody>
        <TableState columns={5} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map(s => <tr key={s.path}>
          <td title={s.path}>{s.path.slice(s.folder.length).replace(/^[\\/]/, '') || s.path}</td>
          <td className="wrap">{(s.writes || []).map((k: string) => <button key={k} className="report-name chip-link" onClick={() => openObject(k)}>{k.split('.').pop()}</button>)}</td>
          <td title={(s.reads || []).join('\n')}>{names(s.reads || [])}</td>
          <td>{utc(s.modified)}</td>
          <td>{s.error ? <span className="tag tag-warn" title={s.error}>{t('部分语句无法解析')}</span> : <span className="tag tag-ok">{t('正常')}</span>}</td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
    {!list.loading && !rows.length && <p className="source-hint">{t('尚未配置脚本目录。')} <button className="link-button" onClick={() => go('settings')}>{t('前往设置')}</button></p>}
  </>;
}

function MyBITasks() {
  const list = useApi<any[]>('/api/refresh/mybi');
  const { rows, page, setPage, total } = usePaged(list.data || []);
  return <>
    <table className="reports-table">
      <thead><tr><th>{t('名称')}</th><th>{t('类型')}</th><th>{t('调度')}</th><th>{t('最近运行')}</th><th>{t('耗时')}</th><th>{t('结果')}</th><th>{t('读取 / 写入')}</th></tr></thead>
      <tbody>
        <TableState columns={7} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map(n => <tr key={n.key}>
          <td title={n.name}>{n.name}</td><td>{label(n.kind)}</td><td>{schedule(n.info?.schedule)}</td>
          <td>{n.info?.last_run_at ? utc(n.info.last_run_at.slice(0, 19).replace(' ', 'T') + 'Z') : '—'}</td>
          <td>{n.info?.duration_seconds ? duration('1970-01-01T00:00:00Z', new Date(Number(n.info.duration_seconds) * 1000).toISOString()) : '—'}</td>
          <td>{n.info?.last_error ? <span className="status-error" title={n.info.last_error}>{n.info.last_error}</span> : n.info?.last_run_at ? <span className="status-ok">{label('ok')}</span> : '—'}</td>
          <td>{n.kind === 'mybi_task' ? (n.writes || '—') : t('读取 {0} 个对象', n.reads)} <button className="link-button" onClick={() => go('lineage?key=' + encodeURIComponent(n.key))}>{t('血缘')}</button></td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
  </>;
}
