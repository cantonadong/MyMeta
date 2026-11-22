import { useEffect, useState } from 'react';
import { Page, post, toast } from './api';
import { go } from './App';
import { TableState, useApi } from './Catalog';
import { duration, label, number, utc } from './format';
import { t } from './i18n';
import { Pagination } from './Pagination';

type Run = { id: number; scope: string; trigger: string; started_at: string; finished_at: string | null; status: string; error: string | null; baseline: number; summary: Record<string, any> | null };
type Status = { running: boolean; interval_minutes: number; scopes: { profile: string; schema: string; label: string; connection_found: boolean; last_run: Run | null; counts: { objects: number; columns: number } }[] };

function Summary({ run }: { run: Run }) {
  if (run.error) return <span className="source-error" title={run.error}>{run.error === 'interrupted' ? t('程序退出，同步中断') : run.error}</span>;
  const s = run.summary;
  if (!s) return <>—</>;
  const parts = run.baseline ? [t('基线：{0} 个对象，{1} 个字段', number(s.objects), number(s.columns))]
    : [t('新增对象 {0}', s.added_objects), t('删除对象 {0}', s.deleted_objects), t('新增字段 {0}', s.added_columns), t('删除字段 {0}', s.deleted_columns), t('变更 {0} 项', s.changes)];
  return <>{parts.join(' · ')}{s.warnings?.length ? <span className="tag" title={s.warnings.join('\n')}>{t('{0} 项警告', s.warnings.length)}</span> : null}</>;
}

export function SyncPage() {
  const [version, setVersion] = useState(0);
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState<string | null>(null);
  const status = useApi<Status>('/api/sync/status', version);
  const runs = useApi<Page<Run>>(`/api/sync/runs?page=${page}`, version);
  const running = !!busy || !!status.data?.running;
  // Poll while a scheduled or manual sync is in progress.
  useEffect(() => { if (!status.data?.running) return; const timer = setTimeout(() => setVersion(v => v + 1), 3000); return () => clearTimeout(timer); }, [status.data, version]);
  const sync = async (scope?: { profile: string; schema: string }) => {
    setBusy(scope ? scope.profile + scope.schema : 'all');
    try {
      const results = await post<Run[]>('/api/sync', scope ? { profile: scope.profile, schema: scope.schema } : undefined);
      const failed = results.filter(run => run.status !== 'ok').length;
      toast(failed ? t('同步完成，{0} 个范围失败', failed) : t('同步完成'));
    } catch (error) { toast((error as Error).message); }
    finally { setBusy(null); setVersion(v => v + 1); }
  };
  const scopes = status.data?.scopes || [];
  const runRows = runs.data?.rows || [];
  return <main className="reports-page">
    <header className="reports-heading">
      <h2>{t('同步')}</h2>
      <button className="primary" disabled={running} onClick={() => sync()}>{busy === 'all' ? <><span className="busy-spinner" />{t('同步中…')}</> : t('立即同步')}</button>
    </header>
    <p className="source-hint">{t('程序运行期间每 {0} 分钟自动同步一次；同步只读取系统表，不会修改人工维护的内容。', status.data?.interval_minutes ?? 60)}{status.data?.running && !busy && <> {t('后台同步进行中…')}</>}</p>
    <table className="reports-table sync-table">
      <thead><tr><th>{t('范围')}</th><th>{t('连接')}</th><th>{t('对象数')}</th><th>{t('字段数')}</th><th>{t('最近同步')}</th><th>{t('结果')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={7} loading={status.loading} error={status.error} empty={!scopes.length} />
        {scopes.map(scope => <tr key={scope.label}>
          <td>{scope.label}</td>
          <td>{scope.profile}{!scope.connection_found && <span className="tag tag-danger">{t('连接不存在')}</span>}</td>
          <td>{number(scope.counts?.objects)}</td><td>{number(scope.counts?.columns)}</td>
          <td>{utc(scope.last_run?.finished_at || scope.last_run?.started_at)}</td>
          <td className="wrap">{scope.last_run ? <><span className={'status-' + scope.last_run.status}>{label(scope.last_run.status)}</span> <Summary run={scope.last_run} /></> : '—'}</td>
          <td><div className="report-actions"><button disabled={running} onClick={() => sync(scope)}>{busy === scope.profile + scope.schema ? <><span className="busy-spinner" />{t('同步中…')}</> : t('同步此范围')}</button></div></td>
        </tr>)}
      </tbody>
    </table>
    <h3 className="section-heading">{t('同步记录')}</h3>
    <table className="reports-table">
      <thead><tr><th className="narrow">#</th><th>{t('范围')}</th><th>{t('触发方式')}</th><th>{t('开始时间')}</th><th>{t('耗时')}</th><th>{t('结果')}</th><th>{t('差异')}</th></tr></thead>
      <tbody>
        <TableState columns={7} loading={runs.loading} error={runs.error} empty={!runRows.length} />
        {runRows.map(run => <tr key={run.id}>
          <td>{run.id}</td><td>{run.scope}</td><td>{label(run.trigger)}</td><td>{utc(run.started_at)}</td><td>{duration(run.started_at, run.finished_at)}</td>
          <td><span className={'status-' + run.status}>{label(run.status)}</span></td>
          <td className="wrap">{run.summary?.changes ? <button className="report-name" onClick={() => go('changes?run=' + run.id)}><Summary run={run} /></button> : <Summary run={run} />}</td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={runs.data?.total || 0} onChange={setPage} disabled={runs.loading} />
  </main>;
}
