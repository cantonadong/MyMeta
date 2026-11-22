import { ReactNode, useEffect, useRef, useState } from 'react';
import { post, toast } from './api';
import { TableState, useApi } from './Catalog';
import { confirmAction } from './confirmAction';
import { label } from './format';
import { Language, setLanguage, t, useLanguage } from './i18n';
import { Pagination } from './Pagination';
import { usePaged } from './ui';

type Scope = { profile: string; schema: string };
type Folder = { path: string; database: string; schema: string };

const SECTIONS: [string, string][] = [['sync', '同步'], ['naming', '命名规则'], ['scripts', '脚本与 MyBI'], ['jobs', '定时检查'], ['cleanup', '临时对象清理'], ['data', '导入导出'], ['ui', '界面']];
const toLines = (map: Record<string, string>) => Object.entries(map || {}).map(([k, v]) => `${k}=${v}`).join('\n');
const fromLines = (text: string) => Object.fromEntries(text.split('\n').map(l => l.split('=').map(s => s.trim())).filter(p => p.length === 2 && p[0] && p[1]));

function Card({ id, title, children, onSave, saving }: { id: string; title: string; children: ReactNode; onSave?: () => void; saving?: boolean }) {
  return <section id={'settings-' + id} className="settings-card">
    <h3>{title}</h3>
    {children}
    {onSave && <div className="settings-actions"><button className="primary" disabled={saving} onClick={onSave}>{saving ? <><span className="busy-spinner" />{t('保存中…')}</> : t('保存')}</button></div>}
  </section>;
}

export function SettingsPage() {
  const { language } = useLanguage();
  const [version, setVersion] = useState(0);
  const connections = useApi<{ name: string; redshift: boolean; database: string }[]>('/api/connections');
  const settings = useApi<any>('/api/settings', version);
  const [form, setForm] = useState<any>(null);
  const [saving, setSaving] = useState('');
  const [active, setActive] = useState('sync');
  useEffect(() => { if (settings.data) setForm({ ...settings.data, intervalText: String(settings.data.sync_interval_minutes), products: toLines(settings.data.naming.products), maintainers: toLines(settings.data.naming.maintainers), layer_synonyms: toLines(settings.data.naming.layer_synonyms), grain_synonyms: toLines(settings.data.naming.grain_synonyms), grains: settings.data.naming.grains.join(', ') }); }, [settings.data]);
  const profiles = connections.data || [];
  const save = async (section: string, body: Record<string, unknown>) => {
    setSaving(section);
    try { await post('/api/settings', body, 'PUT'); toast(t('设置已保存')); setVersion(v => v + 1); return true; }
    catch (e) { toast((e as Error).message); return false; } finally { setSaving(''); }
  };
  if (!form) return <main className="reports-page"><p className="source-hint">{settings.error || t('加载中…')}</p></main>;
  const set = (k: string, v: unknown) => setForm({ ...form, [k]: v });
  const editScope = (i: number, change: Partial<Scope>) => set('scopes', form.scopes.map((s: Scope, j: number) => j === i ? { ...s, ...change } : s));
  const editFolder = (i: number, change: Partial<Folder>) => set('script_folders', form.script_folders.map((f: Folder, j: number) => j === i ? { ...f, ...change } : f));
  const databases = [...new Set(profiles.map(p => p.database))];
  return <main className="reports-page settings-page">
    <nav className="settings-nav">{SECTIONS.map(([id, text]) => <a key={id} href={'#/settings'} aria-current={active === id ? 'location' : undefined} onClick={e => { e.preventDefault(); setActive(id); document.getElementById('settings-' + id)?.scrollIntoView({ behavior: 'smooth' }); }}>{t(text)}</a>)}</nav>
    <div className="settings-content">
      <Card id="sync" title={t('同步范围与频率')} saving={saving === 'sync'} onSave={() => {
        const minutes = Number(form.intervalText);
        if (!Number.isInteger(minutes) || minutes < 1 || minutes > 1440) { toast(t('同步间隔需在 1–1440 分钟之间')); return; }
        save('sync', { scopes: form.scopes.map((s: Scope) => ({ ...s, schema: s.schema.trim() })), sync_interval_minutes: minutes, rename_threshold: Number(form.rename_threshold) });
      }}>
        <p className="source-hint">{t('连接来自 MyQuery（%APPDATA%\\MyDataTools），在 MyQuery 中新增或修改连接后刷新本页即可。只采集所选模式下的表、视图和物化视图。')}</p>
        {connections.error && <p role="alert" className="source-error">{connections.error}</p>}
        <div className="scope-list">
          {form.scopes.map((scope: Scope, index: number) => <div key={index} className="scope-row">
            <select value={scope.profile} onChange={e => editScope(index, { profile: e.target.value })} aria-label={t('连接')}>
              <option value="">{t('选择连接')}</option>
              {profiles.map(p => <option key={p.name} value={p.name}>{p.name}（{p.database} · {p.redshift ? 'Redshift' : 'PostgreSQL'}）</option>)}
              {scope.profile && !profiles.some(p => p.name === scope.profile) && <option value={scope.profile}>{scope.profile}（{t('连接不存在')}）</option>}
            </select>
            <input value={scope.schema} onChange={e => editScope(index, { schema: e.target.value })} placeholder={t('模式，如 public')} aria-label={t('模式')} />
            <button className="report-delete" onClick={() => set('scopes', form.scopes.filter((_: Scope, i: number) => i !== index))}>{t('移除')}</button>
          </div>)}
        </div>
        <button onClick={() => set('scopes', [...form.scopes, { profile: '', schema: 'public' }])}>{t('添加范围')}</button>
        <label className="setting-row">{t('自动同步间隔（分钟）')}<input type="number" min={1} max={1440} value={form.intervalText} onChange={e => set('intervalText', e.target.value)} /></label>
        <label className="setting-row">{t('表重命名识别：字段名重合比例 ≥')}<input type="number" min={0.5} max={1} step={0.05} value={form.rename_threshold} onChange={e => set('rename_threshold', e.target.value)} /></label>
      </Card>

      <Card id="naming" title={t('命名规则')} saving={saving === 'naming'} onSave={() => save('naming', { naming: { products: fromLines(form.products), maintainers: fromLines(form.maintainers), layer_synonyms: fromLines(form.layer_synonyms), grain_synonyms: fromLines(form.grain_synonyms), grains: form.grains.split(',').map((g: string) => g.trim()).filter(Boolean) } })}>
        <p className="source-hint">{t('规则：{产品前缀}[维护人缩写_]{层级}_{主题}[_{粒度}][_v 或 _mv]。层级为 dim / dtl / fct / tpc / app / tmp；同义写法可解析，但会在命名检查中标记。每行一项，格式“写法=值”。')}</p>
        <div className="form-grid">
          <label className="form-field"><span>{t('产品前缀')}</span><textarea rows={4} value={form.products} onChange={e => set('products', e.target.value)} /></label>
          <label className="form-field"><span>{t('维护人缩写')}</span><textarea rows={4} value={form.maintainers} onChange={e => set('maintainers', e.target.value)} /></label>
          <label className="form-field"><span>{t('层级同义词')}</span><textarea rows={4} value={form.layer_synonyms} onChange={e => set('layer_synonyms', e.target.value)} /></label>
          <label className="form-field"><span>{t('粒度缩写')}</span><textarea rows={4} value={form.grain_synonyms} onChange={e => set('grain_synonyms', e.target.value)} /></label>
          <label className="form-field form-wide"><span>{t('粒度（逗号分隔）')}</span><input value={form.grains} onChange={e => set('grains', e.target.value)} /></label>
        </div>
      </Card>

      <Card id="scripts" title={t('刷新脚本与 MyBI')} saving={saving === 'scripts'} onSave={() => save('scripts', { script_folders: form.script_folders.map((f: Folder) => ({ ...f, path: f.path.trim() })), mybi_path: form.mybi_path.trim() })}>
        <p className="source-hint">{t('递归读取目录中的 .sql 文件（可以是已 clone 的 Git 仓库），解析 INSERT / CREATE TABLE AS / CREATE VIEW 等写入关系，作为血缘来源。未写库名的表名按这里的库和模式解析。')}</p>
        <div className="scope-list">
          {form.script_folders.map((f: Folder, i: number) => <div key={i} className="scope-row">
            <input className="path-input" value={f.path} placeholder={t('目录路径，如 C:\\etl\\sql')} onChange={e => editFolder(i, { path: e.target.value })} />
            <select value={f.database} onChange={e => editFolder(i, { database: e.target.value })} aria-label={t('库')}>{databases.map(d => <option key={d}>{d}</option>)}</select>
            <input value={f.schema} onChange={e => editFolder(i, { schema: e.target.value })} aria-label={t('模式')} />
            <button className="report-delete" onClick={() => set('script_folders', form.script_folders.filter((_: Folder, j: number) => j !== i))}>{t('移除')}</button>
          </div>)}
        </div>
        <button onClick={() => set('script_folders', [...form.script_folders, { path: '', database: databases[0] || 'proddb', schema: 'public' }])}>{t('添加目录')}</button>
        <label className="setting-row">{t('MyBI 元数据库路径')}<input className="path-input" value={form.mybi_path} onChange={e => set('mybi_path', e.target.value)} /></label>
        <p className="source-hint">{t('只读打开 MyBI 的 data-workbench.db，读取连接数据集、定时任务和看板，血缘末端延伸到看板。保存后点“血缘”页的“重新解析血缘”或等待下次同步。')}</p>
      </Card>

      <Card id="jobs" title={t('定时检查')} saving={saving === 'jobs'} onSave={() => save('jobs', { freshness_time: form.freshness_time })}>
        <label className="setting-row">{t('每日数据日期检查时间（UTC）')}<input value={form.freshness_time} placeholder="03:00" onChange={e => set('freshness_time', e.target.value)} /></label>
        <p className="source-hint">{t('每天一次，对有日期字段的表和 MV 查询 MAX(日期字段)，与 T-1（UTC 当天 - 1）比较。日期字段自动按名称识别，可在对象页修改或填 - 关闭。程序需在运行中。')}</p>
        <div className="report-actions"><JobButton path="/api/freshness/all" text={t('立即检查全部')} done={t('检查完成')} /><JobButton path="/api/backup" text={t('立即备份')} done={t('已备份')} /></div>
        <p className="source-hint">{t('元数据库每天自动备份到 data\\backups，保留最近 14 份。')}</p>
      </Card>

      <Cleanup config={form.cleanup} onSaved={() => setVersion(v => v + 1)} />

      <DataCard />

      <section id="settings-ui" className="settings-card">
        <h3>{t('界面')}</h3>
        <label className="setting-row">{t('语言')}<select value={language} onChange={event => setLanguage(event.target.value as Language)}>
          <option value="auto">{t('跟随浏览器')}</option><option value="zh-CN">简体中文</option><option value="en">English</option>
        </select></label>
      </section>
    </div>
  </main>;
}

function JobButton({ path, text, done }: { path: string; text: string; done: string }) {
  const [busy, setBusy] = useState(false);
  return <button disabled={busy} onClick={async () => { setBusy(true); try { await post(path); toast(done); } catch (e) { toast((e as Error).message); } finally { setBusy(false); } }}>{busy ? <><span className="busy-spinner" />{t('执行中…')}</> : text}</button>;
}

function Cleanup({ config, onSaved }: { config: any; onSaved: () => void }) {
  const [form, setForm] = useState({ enabled: !!config.enabled, pattern: config.pattern, days: String(config.days), time: config.time });
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState(0);
  const preview = useApi<any>('/api/cleanup', version);
  const { rows, page, setPage, total } = usePaged<any>(preview.data?.candidates || []);
  const confirmedRule = config.confirmed && config.confirmed.pattern === form.pattern && String(config.confirmed.days) === form.days;
  const save = async (confirm: boolean) => {
    if (confirm && !await confirmAction(t('确认清理规则？确认后每天 {0}（UTC）自动 DROP 名称日期早于 {1} 天前的临时表 / MV（不使用 CASCADE），无法撤销。', form.time, form.days))) return;
    setBusy(true);
    try { await post('/api/settings', { cleanup: { ...form, days: Number(form.days), enabled: confirm ? true : form.enabled, confirm } }, 'PUT'); toast(confirm ? t('规则已确认并启用') : t('设置已保存')); onSaved(); setVersion(v => v + 1); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  const runNow = async () => {
    const drops = (preview.data?.candidates || []).filter((c: any) => c.action === 'drop');
    if (!await confirmAction(t('立即删除 {0} 个超期临时对象？此操作无法撤销。', drops.length))) return;
    setBusy(true);
    try { const r = await post<any>('/api/cleanup'); toast(t('已删除 {0} 个，失败 {1} 个', r.dropped, r.failed)); setVersion(v => v + 1); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  return <section id="settings-cleanup" className="settings-card">
    <h3>{t('超期临时对象自动清理')}</h3>
    <p className="source-hint">{t('识别规则用第一个括号捕获 yyyymmdd。只处理同步范围内名称含 _cd_tmp_ 的表和 MV；名称不含日期、日期无效、视图或删除失败的对象只进入待办。规则修改后需重新确认，确认前暂停清理。')}</p>
    <p>{config.effective ? <span className="tag tag-ok">{t('已生效')}</span> : config.enabled && !confirmedRule ? <span className="tag tag-warn">{t('规则已修改，等待确认')}</span> : <span className="tag">{t('未启用')}</span>}</p>
    <div className="form-grid">
      <label className="form-field form-wide"><span>{t('识别规则（正则）')}</span><input value={form.pattern} onChange={e => setForm({ ...form, pattern: e.target.value })} /></label>
      <label className="form-field"><span>{t('超期天数 N')}</span><input type="number" min={1} max={30} value={form.days} onChange={e => setForm({ ...form, days: e.target.value })} /></label>
      <label className="form-field"><span>{t('每日执行时间（UTC）')}</span><input value={form.time} onChange={e => setForm({ ...form, time: e.target.value })} /></label>
      <label className="check"><input type="checkbox" checked={form.enabled} onChange={e => setForm({ ...form, enabled: e.target.checked })} />{t('启用自动清理')}</label>
    </div>
    <div className="settings-actions">
      <button disabled={busy || !confirmedRule || !(preview.data?.candidates || []).some((c: any) => c.action === 'drop')} onClick={runNow}>{t('立即执行')}</button>
      <button disabled={busy} onClick={() => save(false)}>{t('保存')}</button>
      <button className="primary" disabled={busy} onClick={() => save(true)}>{t('确认规则并启用')}</button>
    </div>
    <h3 className="section-heading">{t('按当前已保存的规则预览')}</h3>
    <table className="reports-table">
      <thead><tr><th>{t('对象')}</th><th>{t('类型')}</th><th>{t('名称中的日期')}</th><th>{t('处理')}</th></tr></thead>
      <tbody>
        <TableState columns={4} loading={preview.loading} error={preview.error} empty={!rows.length} />
        {rows.map((c: any) => <tr key={c.key}><td title={c.key}>{c.name}</td><td>{label(c.kind)}</td><td>{c.date || '—'}</td>
          <td><span className={'tag' + (c.action === 'drop' ? ' tag-danger' : c.action === 'todo' ? ' tag-warn' : '')}>{c.action === 'drop' ? t('将删除') : c.action === 'todo' ? t('进入待办') : t('保留')}</span> {label(c.reason)}</td></tr>)}
      </tbody>
    </table>
    <Pagination page={page} total={total} onChange={setPage} />
  </section>;
}

function DataCard() {
  const fileRef = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const importFile = async (file: File) => {
    const excel = /\.xlsx$/i.test(file.name);
    if (!await confirmAction(excel ? t('从 Excel 更新业务信息？按业务键匹配，空单元格会清空对应内容；导入前会自动备份。') : t('用 JSON 文件替换全部人工维护内容（业务信息、规则、关联、流程、指标）？导入前会自动备份。'))) return;
    setBusy(true);
    try {
      const body = excel ? await file.arrayBuffer() : await file.text();
      const response = await fetch(excel ? '/api/import.xlsx' : '/api/import.json', { method: 'POST', headers: { 'Content-Type': excel ? 'application/octet-stream' : 'application/json' }, body });
      const text = await response.text();
      if (!response.ok) throw new Error(t(text));
      const result = JSON.parse(text);
      toast(t('导入完成：{0}', Object.entries(result.counts).map(([k, v]) => `${label(k)} ${v}`).join('，')));
    } catch (e) { toast((e as Error).message); } finally { setBusy(false); if (fileRef.current) fileRef.current.value = ''; }
  };
  return <section id="settings-data" className="settings-card">
    <h3>{t('导入导出')}</h3>
    <p className="source-hint">{t('JSON 包含全部人工维护内容和设置（不含数据库密码），用于备份和 V1 → V2 迁移，导入时整体替换。Excel 包含对象、字段、规则三张表，可批量填写后导入更新。')}</p>
    <div className="report-actions">
      <a className="button-link" href="/api/export.json" download>{t('导出 JSON')}</a>
      <a className="button-link" href="/api/export.xlsx" download>{t('导出 Excel')}</a>
      <button disabled={busy} onClick={() => fileRef.current?.click()}>{busy ? <><span className="busy-spinner" />{t('导入中…')}</> : t('导入 JSON / Excel')}</button>
      <input ref={fileRef} type="file" accept=".json,.xlsx" hidden onChange={e => e.target.files?.[0] && importFile(e.target.files[0])} />
    </div>
  </section>;
}
