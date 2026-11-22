// UI preview data: open the page with ?mock (normal), ?mock=empty or ?mock=error. Responses are delayed to show loading states.
const now = '2026-11-23T02:00:00Z';
const run = (id: number, scope: string, extra: object = {}) => ({ id, scope, trigger: 'schedule', started_at: '2026-11-23T01:59:48Z', finished_at: now, status: 'ok', error: null, baseline: 0, summary: { objects: 2203, columns: 16707, added_objects: 1, deleted_objects: 1, added_columns: 2, deleted_columns: 0, changes: 5, warnings: [] }, ...extra });
const objects = [
  { key: 'proddb.public.btd6mobile_cd_fct_iap_daily', scope: 'proddb.public', name: 'btd6mobile_cd_fct_iap_daily', kind: 'table', comment: 'BTD6 每日 IAP 收入（USD）', props: { diststyle: 'KEY(event_date)', sortkey: 'event_date', rows: 1830211, size_bytes: 412090368, encoded: 'Y' }, is_deleted: 0, synced_at: now, column_count: 14 },
  { key: 'proddb.public.btd6mobile_cd_topic_paragon_unlock_daily_mv', scope: 'proddb.public', name: 'btd6mobile_cd_topic_paragon_unlock_daily_mv', kind: 'mv', comment: null, props: { mv_state: '1', auto_refresh: false, last_refresh: '2026-11-22 18:00:03' }, is_deleted: 0, synced_at: now, column_count: 6 },
  { key: 'nkreportr.public.steam_freeapp_downloads_bycountry', scope: 'nkreportr.public', name: 'steam_freeapp_downloads_bycountry', kind: 'table', comment: null, props: { rows_estimate: 64917, size_bytes: 14229504, primary_key: 'date,name,country' }, is_deleted: 0, synced_at: now, column_count: 9 },
  { key: 'proddb.public.btd6mobile_cd_tmp_retention_20261120', scope: 'proddb.public', name: 'btd6mobile_cd_tmp_retention_20261120', kind: 'table', comment: null, props: {}, is_deleted: 1, synced_at: now, column_count: 3 },
];
const columns = [
  { key: 'proddb.public.btd6mobile_cd_fct_iap_daily.country', name: 'country', data_type: 'character varying(8)', nullable: 1, comment: 'ISO 3166 国家代码', is_deleted: 0, ordinal: 2, props: { encoding: 'lzo' }, object_key: objects[0].key, object_name: objects[0].name, scope: 'proddb.public', kind: 'table' },
  { key: 'nkreportr.public.steam_freeapp_downloads_bycountry.country', name: 'country', data_type: 'text', nullable: 0, comment: null, is_deleted: 0, ordinal: 3, props: {}, object_key: objects[2].key, object_name: objects[2].name, scope: 'nkreportr.public', kind: 'table' },
  { key: 'proddb.public.btd6mobile_cd_fct_iap_daily.event_date', name: 'event_date', data_type: 'date', nullable: 0, comment: null, is_deleted: 0, ordinal: 1, props: { encoding: 'az64', distkey: true, sortkey: '1' }, object_key: objects[0].key, object_name: objects[0].name, scope: 'proddb.public', kind: 'table' },
  { key: 'proddb.public.btd6mobile_cd_fct_iap_daily.old_flag', name: 'old_flag', data_type: 'boolean', nullable: 1, comment: null, is_deleted: 1, ordinal: 9, props: {}, object_key: objects[0].key, object_name: objects[0].name, scope: 'proddb.public', kind: 'table' },
];
const changes = [
  { id: 5, entity: 'column', key: 'proddb.public.btd6mobile_cd_fct_iap_daily.country', field: 'data_type', before: 'character varying(2)', after: 'character varying(8)', source: 'sync', actor: 'sync', at: now, run_id: 7 },
  { id: 4, entity: 'column', key: 'proddb.public.btd6mobile_cd_fct_iap_daily.old_flag', field: 'status', before: 'present', after: 'deleted', source: 'sync', actor: 'sync', at: now, run_id: 7 },
  { id: 3, entity: 'object', key: objects[1].key, field: 'kind', before: 'view', after: 'mv', source: 'sync', actor: 'sync', at: now, run_id: 7 },
  { id: 2, entity: 'object', key: objects[0].key, field: 'comment', before: null, after: 'BTD6 每日 IAP 收入（USD）', source: 'sync', actor: 'sync', at: now, run_id: 7 },
];
const todos = [
  { id: 3, kind: 'new_object', key: 'proddb.public.zamobile_cd_dim_game_mode', status: 'open', created_at: now, handled_at: null },
  { id: 2, kind: 'new_column', key: 'proddb.public.btd6mobile_cd_fct_iap_daily.platform', status: 'open', created_at: now, handled_at: null },
  { id: 1, kind: 'deleted_column', key: 'proddb.public.btd6mobile_cd_fct_iap_daily.old_flag', status: 'open', created_at: now, handled_at: null },
];
const page = (rows: unknown[]) => ({ total: rows.length, rows });
const naming = { product: 'BTD6', maintainer: 'Carl Dong', layer: 'tpc', subject: 'paragon_unlock', grain: 'daily', suffix: 'mv', temporary: false, tmp_date: null, issues: ['layer_synonym'] };
const graphNodes = [
  { key: objects[1].key, name: objects[1].name, kind: 'mv', scope: 'proddb.public', layer: 'tpc', depth: 0 },
  { key: 'proddb.public.btd6mobile_cd_dim_paragon_release', name: 'btd6mobile_cd_dim_paragon_release', kind: 'table', scope: 'proddb.public', layer: 'dim', depth: -1 },
  { key: 'proddb.public.btd6mobile_unlocktowertier', name: 'btd6mobile_unlocktowertier', kind: 'table', scope: 'proddb.public', depth: -1 },
  { key: 'mybi:dataset:1', name: 'Paragon 解锁', kind: 'mybi_dataset', depth: 1, external: true },
  { key: 'mybi:report:1', name: 'BTD6 周报', kind: 'mybi_report', depth: 2, external: true },
];
const graphEdges = [[1, 0], [2, 0], [0, 3], [3, 4]].map(([a, b]) => ({ from: graphNodes[a].key, to: graphNodes[b].key, sources: 'view' }));
const relations = [{ id: 1, left_key: objects[0].key, right_key: objects[2].key, key_pairs: [[objects[0].key + '.country', objects[2].key + '.country']], kind: 'n:n', note: '按国家对照收入与下载', diff_note: 'proddb 为 ISO2，nkreportr 为 Steam 国家名', status: 'confirmed', left_name: objects[0].name, right_name: objects[2].name, cross_db: 1 }];
const metrics = [{ id: 1, name_cn: 'D1 留存率', name_en: 'D1 retention', definition: '某日新增用户中，第 1 天（次日）再次活跃的比例', formula: 'D1 活跃新增用户数 / 当日新增用户数', numerator: '第 1 天仍活跃的当日新增用户数', denominator: '当日新增用户数', object_key: objects[1].key, column_name: 'unlock_users', used_by: ['mybi:dataset:1'], builtin: 1 }];
const steps = [['安装', 'btd6mobile_installs'], ['首次启动', 'btd6mobile_sessions'], ['关卡', 'btd6mobile_starttrack'], ['付费', objects[0].name]].map(([name, table], i) => ({ id: i + 1, product: 'BTD6', flow: '新手流程', step_no: i + 1, name, object_key: 'proddb.public.' + table, columns: 'userid, created_at', note: '' }));
const settings = { scopes: [{ profile: 'ProdDB', schema: 'public' }, { profile: 'NKReportR', schema: 'public' }], sync_interval_minutes: 60, rename_threshold: 0.8, freshness_time: '03:00', mybi_path: 'C:\\Program\\MyBI\\data-workbench.db',
  naming: { products: { btd6mobile: 'BTD6', blitzmobile: 'Blitz', zamobile: 'ZAR' }, maintainers: { cd: 'Carl Dong' }, layer_synonyms: { detail: 'dtl', topic: 'tpc' }, grains: ['daily', 'weekly', 'monthly', 'latest'], grain_synonyms: { d: 'daily' } },
  script_folders: [{ path: 'C:\\etl\\sql', database: 'proddb', schema: 'public' }],
  cleanup: { enabled: true, pattern: '^[a-z0-9]+_cd_tmp_[a-z0-9_]+_(\\d{8})(_mv)?$', days: 1, time: '02:00', confirmed: null, effective: false } };

export async function mockApi(path: string, init: RequestInit | undefined, mode: string) {
  await new Promise(resolve => setTimeout(resolve, 400));
  if (mode === 'error') throw new Error('无法连接 MyMeta 服务（预览：错误状态）');
  const empty = mode === 'empty';
  const [route, query = ''] = path.split('?');
  const params = new URLSearchParams(query);
  if (route === '/api/sample') return [{ value: 'US', count: 120345 }, { value: 'GB', count: 40211 }, { value: null, count: 12 }];
  if (route === '/api/lineage/rebuild') return { views: 359, scripts: 1, parse_errors: 0 };
  if (init?.method && init.method !== 'GET') return route === '/api/sync' ? [run(9, 'proddb.public', { trigger: 'manual' }), run(10, 'nkreportr.public', { trigger: 'manual', status: 'error', error: 'timeout expired', summary: null })] : { ok: true };
  switch (route) {
    case '/api/connections': return [{ name: 'ProdDB', redshift: true, database: 'proddb' }, { name: 'NKReportR', redshift: false, database: 'nkreportr' }];
    case '/api/settings': return empty ? { ...settings, scopes: [], script_folders: [] } : settings;
    case '/api/sync/status': return { running: false, interval_minutes: 60, scopes: empty ? [] : [
      { profile: 'ProdDB', schema: 'public', label: 'proddb.public', connection_found: true, last_run: run(7, 'proddb.public'), counts: { objects: 2203, columns: 16707 } },
      { profile: 'NKReportR', schema: 'public', label: 'nkreportr.public', connection_found: true, last_run: run(8, 'nkreportr.public', { status: 'error', error: 'password authentication failed for user "nkreportr"', summary: null }), counts: { objects: 158, columns: 1235 } },
    ] };
    case '/api/sync/runs': return page(empty ? [] : [run(8, 'nkreportr.public', { status: 'error', error: 'password authentication failed', summary: null }), run(7, 'proddb.public'), run(2, 'nkreportr.public', { trigger: 'startup', baseline: 1 }), run(1, 'proddb.public', { trigger: 'startup', baseline: 1 })]);
    case '/api/objects': return page(empty ? [] : objects.filter(o => (params.get('tmp') === 'true' || !o.name.includes('_cd_tmp_')) && (params.get('deleted') === 'true' || !o.is_deleted)));
    case '/api/columns': return page(empty ? [] : columns.filter(c => params.get('deleted') === 'true' || !c.is_deleted));
    case '/api/object': {
      const object = objects.find(o => o.key === params.get('key')) || objects[0];
      return { object: { ...object, naming, first_seen: '2026-11-22T20:44:44Z', definition: object.kind === 'mv' ? 'CREATE MATERIALIZED VIEW btd6mobile_cd_topic_paragon_unlock_daily_mv\nAUTO REFRESH NO AS\nSELECT event_date, paragon, COUNT(*) AS unlocks\nFROM btd6mobile_cd_dtl_paragon_unlock\nGROUP BY 1, 2;' : null },
        biz: empty ? null : { key: object.key, cn_name: 'BTD6 每日 IAP 收入', description: null, layer: null }, columns: empty ? [] : columns.filter(c => c.object_key === object.key).map(c => ({ ...c, meaning: c.name === 'country' ? '用户所在国家' : null, rule_count: c.name === 'country' ? 2 : 0 })),
        rules: empty ? [] : [{ id: 1, target_key: object.key, kind: 'filter', condition: 'is_test = 0', note: '所有报表' }, { id: 2, target_key: object.key + '.country', kind: 'invalid', value: 'XX', note: '未知国家' }, { id: 3, target_key: object.key + '.country', kind: 'valid', value: 'US', note: '美国' }],
        freshness: { date_column: 'event_date', max_date: '2026-11-20', expected: '2026-11-22', delayed: 1, checked_at: now }, date_column: 'event_date',
        refreshed_by: empty ? [] : [{ source: 'script', origin: 'C:\\etl\\sql\\btd6_iap.sql', name: null }], counts: { upstream: 2, downstream: 3, relations: 1 } };
    }
    case '/api/sql-template': return { sql: 'SELECT\n    "event_date",\n    "country"  -- 用户所在国家\nFROM "public"."btd6mobile_cd_fct_iap_daily"\nWHERE is_test = 0  -- 所有报表\n  AND "country" <> \'XX\'\n;' };
    case '/api/comment-sql': return { sql: empty ? '' : 'COMMENT ON TABLE "public"."btd6mobile_cd_fct_iap_daily" IS \'BTD6 每日 IAP 收入\';' };
    case '/api/lineage/graph': return empty ? { nodes: [], edges: [], column_edges: [] } : { nodes: graphNodes, edges: graphEdges, truncated: false, column_edges: [{ upstream: graphNodes[2].key, upstream_column: 'userid', downstream: graphNodes[0].key, downstream_column: 'unlock_users', expression: 'COUNT(DISTINCT userid)' }] };
    case '/api/lineage/impact': return empty ? { objects: [], columns: [] } : { objects: graphNodes.slice(3).map(n => ({ ...n, via: graphNodes[0].key, sources: 'mybi' })), columns: [] };
    case '/api/lineage/violations': return empty ? [] : [{ upstream: 'proddb.public.zamobile_cd_topic_session', downstream: 'proddb.public.zamobile_cd_fct_game', upstream_layer: 'tpc', downstream_layer: 'fct', kind: 'reverse', sources: 'view' }];
    case '/api/relations': return empty ? [] : params.get('status') === 'suggested' ? [{ ...relations[0], id: 2, status: 'suggested', kind: null, note: null, diff_note: null, cross_db: 0 }] : relations;
    case '/api/relations/graph': return empty ? { nodes: [], edges: [] } : { nodes: [{ ...objects[0], depth: 0 }, { ...objects[2], depth: 1 }], edges: relations };
    case '/api/m/metric': return empty ? [] : metrics;
    case '/api/m/flow_step': return empty ? [] : steps;
    case '/api/scripts': return empty ? [] : [{ path: 'C:\\etl\\sql\\btd6_iap.sql', folder: 'C:\\etl\\sql', modified: now, writes: [objects[0].key], reads: ['proddb.public.btd6mobile_iap'], error: null }];
    case '/api/refresh/objects': return { total: empty ? 0 : 2, expected: '2026-11-22', rows: empty ? [] : [{ key: objects[0].key, name: objects[0].name, kind: 'table', max_date: '2026-11-20', delayed: 1, sources: [{ source: 'script', origin: 'C:\\etl\\sql\\btd6_iap.sql' }] }, { key: objects[1].key, name: objects[1].name, kind: 'mv', mv_last_refresh: '2026-11-22 18:00:03', auto_refresh: 0, max_date: '2026-11-22', delayed: 0, sources: [] }] };
    case '/api/refresh/mybi': return empty ? [] : [{ key: 'mybi:dataset:1', name: 'Paragon 解锁', kind: 'mybi_dataset', reads: 2, info: { schedule: { enabled: true, time: '06:00', timezone: 'Pacific/Auckland' }, last_run_at: '2026-11-22 17:00:00', duration_seconds: '3.2', last_error: null } }];
    case '/api/cleanup': return { config: settings.cleanup, candidates: empty ? [] : [{ key: 'proddb.public.btd6mobile_cd_tmp_retention_20261120', name: 'btd6mobile_cd_tmp_retention_20261120', kind: 'table', date: '20261120', action: 'drop', reason: 'expired' }, { key: 'proddb.public.btd6mobile_cd_tmp_cp_params_v', name: 'btd6mobile_cd_tmp_cp_params_v', kind: 'view', date: null, action: 'todo', reason: 'unrecognized' }] };
    case '/api/todos': return page(empty ? [] : [...todos, { id: 4, kind: 'possible_rename', key: 'proddb.public.zamobile_cd_tpc_session', reason: 'proddb.public.zamobile_cd_topic_session', status: 'open', created_at: now, handled_at: null }, { id: 5, kind: 'comment_conflict', key: objects[0].key, reason: null, status: 'open', created_at: now, handled_at: null }].filter(todo => todo.status === (params.get('status') || 'open')));
    case '/api/changes': return page(empty ? [] : changes);
  }
  if (route.startsWith('/api/m/')) return [];
  throw new Error('Unknown mock route ' + route);
}
