import { useEffect, useState } from 'react';
import { api, post, toast } from './api';
import { go } from './App';
import { openObject, TableState, useApi } from './Catalog';
import { label } from './format';
import { t } from './i18n';
import { Pagination } from './Pagination';
import { downloadCsv, Graph, ObjectPicker, Tabs, usePaged } from './ui';

const LEVELS = ['1', '2', '3', '5', '10'];

export function LineagePage({ initialKey }: { initialKey: string }) {
  const [tab, setTab] = useState('graph');
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState(0);
  const rebuild = async () => {
    setBusy(true);
    try { const r = await post<any>('/api/lineage/rebuild'); toast(t('已重新解析：{0} 个视图、{1} 个脚本，{2} 处无法解析', r.views, r.scripts, r.parse_errors)); setVersion(v => v + 1); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('血缘与影响分析')}</h2><button disabled={busy} onClick={rebuild}>{busy ? <><span className="busy-spinner" />{t('解析中…')}</> : t('重新解析血缘')}</button></header>
    <p className="source-hint">{t('血缘来自 View / MV 定义、刷新脚本和 MyBI 数据集，每次同步后自动重新解析。')}</p>
    <div className="toolbar-row"><Tabs value={tab} onChange={setTab} tabs={[['graph', t('血缘图与影响')], ['violations', t('分层依赖检查')]]} /></div>
    {tab === 'graph' ? <GraphTab initialKey={initialKey} version={version} /> : <Violations version={version} />}
  </main>;
}

function GraphTab({ initialKey, version }: { initialKey: string; version: number }) {
  const [key, setKey] = useState(initialKey);
  const [up, setUp] = useState('3'), [down, setDown] = useState('3');
  const [tmp, setTmp] = useState(false);
  const [column, setColumn] = useState('');
  const [columns, setColumns] = useState<string[]>([]);
  useEffect(() => { setColumn(''); if (key) api<any>('/api/object?key=' + encodeURIComponent(key)).then(d => setColumns(d.columns.filter((c: any) => !c.is_deleted).map((c: any) => c.name)), () => setColumns([])); }, [key]);
  useEffect(() => { if (key && key !== initialKey) history.replaceState(null, '', '#/lineage?key=' + encodeURIComponent(key)); }, [key]);
  const graph = useApi<any>(key ? `/api/lineage/graph?key=${encodeURIComponent(key)}&up=${up}&down=${down}&tmp=${tmp}` : null, version);
  const impact = useApi<any>(key ? `/api/lineage/impact?key=${encodeURIComponent(key)}&column=${encodeURIComponent(column)}` : null, version);
  const objects = usePaged<any>(impact.data?.objects || []);
  const impactColumns = usePaged<any>(impact.data?.columns || []);
  const exportCsv = () => {
    const name = key.split('.').pop();
    downloadCsv(`impact-${name}${column ? '-' + column : ''}.csv`, [t('层级距离'), t('对象'), t('类型'), t('层级'), t('上一层'), t('来源')],
      (impact.data?.objects || []).map((o: any) => [o.depth, o.key, label(o.kind), o.layer || '', o.via || '', (o.sources || '').split(',').map(label).join(' / ')]).concat(
        (impact.data?.columns || []).map((c: any) => [c.depth, `${c.key}.${c.column}`, t('字段'), '', c.from, c.expression || ''])));
  };
  return <>
    <div className="toolbar-row">
      <ObjectPicker value={key} onChange={setKey} placeholder={t('输入表名，选择对象')} includeTmp />
      <label className="check">{t('向上')}<select value={up} onChange={e => setUp(e.target.value)}>{LEVELS.map(l => <option key={l}>{l}</option>)}</select></label>
      <label className="check">{t('向下')}<select value={down} onChange={e => setDown(e.target.value)}>{LEVELS.map(l => <option key={l}>{l}</option>)}</select></label>
      <label className="check"><input type="checkbox" checked={tmp} onChange={e => setTmp(e.target.checked)} />{t('显示临时对象')}</label>
    </div>
    {!key ? <p className="reports-empty graph-empty">{t('选择一个对象查看上下游')}</p> : <>
      <section className="settings-card">
        {graph.error ? <p role="alert" className="source-error">{graph.error}</p> : graph.loading && !graph.data ? <p className="source-hint">{t('加载中…')}</p>
          : <><Graph nodes={graph.data?.nodes || []} edges={graph.data?.edges || []} center={key} onOpen={n => !n.external && setKey(n.key)} />
            {graph.data?.truncated && <p className="source-hint">{t('节点超过 200 个，已截断；请减少展开层数。')}</p>}
            <p className="source-hint">{t('点击节点以它为中心展开；')}<button className="link-button" onClick={() => openObject(key)}>{t('打开对象详情')}</button></p></>}
      </section>
      <section className="settings-card">
        <div className="card-heading"><h3>{t('影响分析：下游对象（{0}）', impact.data?.objects?.length ?? 0)}</h3>
          <div className="report-actions">
            <select value={column} onChange={e => setColumn(e.target.value)} aria-label={t('字段')}><option value="">{t('整张表')}</option>{columns.map(c => <option key={c}>{c}</option>)}</select>
            <button disabled={!impact.data} onClick={exportCsv}>{t('导出 CSV')}</button>
          </div>
        </div>
        <table className="reports-table">
          <thead><tr><th className="narrow">{t('距离')}</th><th>{t('对象')}</th><th>{t('类型')}</th><th>{t('层级')}</th><th>{t('来源')}</th></tr></thead>
          <tbody>
            <TableState columns={5} loading={impact.loading} error={impact.error} empty={!objects.rows.length} />
            {objects.rows.map((o: any) => <tr key={o.key}><td>{o.depth}</td>
              <td title={o.key}>{o.external ? o.name : <button className="report-name" onClick={() => openObject(o.key)}>{o.name}</button>}{o.deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}</td>
              <td>{label(o.kind)}</td><td>{o.layer || ''}</td><td>{(o.sources || '').split(',').map(label).join(' / ')}</td></tr>)}
          </tbody>
        </table>
        <Pagination page={objects.page} total={objects.total} onChange={objects.setPage} />
        {column && <>
          <h3 className="section-heading">{t('受影响的下游字段（{0}）', impact.data?.columns?.length ?? 0)}</h3>
          <table className="reports-table">
            <thead><tr><th className="narrow">{t('距离')}</th><th>{t('下游字段')}</th><th>{t('来自')}</th><th>{t('表达式')}</th></tr></thead>
            <tbody>
              <TableState columns={4} loading={impact.loading} empty={!impactColumns.rows.length} />
              {impactColumns.rows.map((c: any, i: number) => <tr key={i}><td>{c.depth}</td><td title={c.key}>{c.key.split('.').pop()}.{c.column}</td><td title={c.from}>{c.from.split('.').slice(-2).join('.')}</td><td title={c.expression || ''}><code>{c.expression || '—'}</code></td></tr>)}
            </tbody>
          </table>
          <Pagination page={impactColumns.page} total={impactColumns.total} onChange={impactColumns.setPage} />
        </>}
      </section>
    </>}
  </>;
}

function Violations({ version }: { version: number }) {
  const list = useApi<any[]>('/api/lineage/violations', version);
  const { rows, page, setPage, total } = usePaged(list.data || []);
  return <>
    <p className="source-hint">{t('按 dtl → fct → tpc → app 的顺序，标记反向引用（如 fct 引用 tpc）和 app 之间互相引用；dim 可被任意层引用。层级取人工设置，未设置时按命名规则识别。')}</p>
    <table className="reports-table">
      <thead><tr><th>{t('问题')}</th><th>{t('上游')}</th><th>{t('下游')}</th><th>{t('来源')}</th></tr></thead>
      <tbody>
        <TableState columns={4} loading={list.loading} error={list.error} empty={!rows.length} />
        {rows.map((v: any, i: number) => <tr key={i}>
          <td><span className="tag tag-danger">{v.kind === 'app_to_app' ? t('app 互相引用') : t('反向引用')}</span> {v.upstream_layer} → {v.downstream_layer}</td>
          <td title={v.upstream}><button className="report-name" onClick={() => openObject(v.upstream)}>{v.upstream.split('.').pop()}</button></td>
          <td title={v.downstream}><button className="report-name" onClick={() => go('lineage?key=' + encodeURIComponent(v.downstream))}>{v.downstream.split('.').pop()}</button></td>
          <td>{v.sources.split(',').map(label).join(' / ')}</td>
        </tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
  </>;
}
