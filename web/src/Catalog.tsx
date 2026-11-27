import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { api, confirmBeforeLeaving, Page, post, toast } from './api';
import { go } from './App';
import { bytes, columnType, isTemporary, label, number, utc } from './format';
import { t } from './i18n';
import { RuleEditor } from './ObjectPage';
import { Field, Modal } from './ui';
import { Icon } from './Icon';
import { CatalogFilters, defaultFilters, matchingObjects, moveObject, QuickFilters, readQuickFilters, searchPattern, tableStructureTsv, writeQuickFilters } from './catalogPreferences';

type OrderGroup = 'order' | 'bottom';

/** Loads `path` whenever it changes; keeps the previous rows visible while the next page loads. */
export function useApi<T>(path: string | null, version = 0) {
  const [state, setState] = useState<{ data?: T; loading: boolean; error?: string }>({ loading: true });
  useEffect(() => {
    if (!path) { setState({ loading: false }); return; }
    let live = true;
    setState(previous => ({ data: previous.data, loading: true }));
    api<T>(path, { cache: 'no-store' }).then(data => live && setState({ data, loading: false }), (error: Error) => live && setState({ loading: false, error: error.message }));
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

type ObjectRow = { key: string; name: string; kind: string; comment: string | null; cn_name: string | null; description: string | null; props: Record<string, unknown>; is_deleted: number };
type ColumnRow = { key: string; name: string; data_type: string; props?: Parameters<typeof columnType>[1]; comment: string | null; meaning: string | null; sample: string | null; notes: string | null; is_deleted: number; is_sensitive?: number | null; object_key: string; object_name: string };

function loadCatalogPage(path: string, page: number, order: string[], hidden: string[], bottom: string[]) {
  const endpoint = `${path}&page=${page}`;
  const tail = new Set(bottom);
  const ranked = order.filter(key => !tail.has(key));
  return ranked.length || hidden.length || bottom.length
    ? post<Page<ObjectRow | ColumnRow>>(endpoint, { order: ranked, hidden, bottom })
    : api<Page<ObjectRow | ColumnRow>>(endpoint, { cache: 'no-store' });
}

// Filters survive navigating to an object and back.
let saved = defaultFilters;
let savedPreset: string | undefined;
let savedSearchTab: CatalogFilters['tab'] | undefined;

export function CatalogPage() {
  const [filters, setFilters] = useState(saved);
  const [q, setQ] = useState(saved.q);
  const [quick, setQuick] = useState(readQuickFilters);
  const [name, setName] = useState<string>();
  const [selectedPreset, setSelectedPreset] = useState(() => savedPreset ?? quick.presets[0]?.name ?? '');
  const [presetDraft, setPresetDraft] = useState<(QuickFilters['presets'][number] & { originalName: string; filterText: string })[]>();
  const [searchTab, setSearchTab] = useState(savedSearchTab);
  const [sampleProgress, setSampleProgress] = useState<string>();
  const [sampleVersion, setSampleVersion] = useState(0);
  const [sampleVersions, setSampleVersions] = useState<Record<string, number>>({});
  const [version, setVersion] = useState(0);
  const [syncing, setSyncing] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const [displayVersion, setDisplayVersion] = useState(0);
  const selectPreset = (value: string) => { savedPreset = value; setSelectedPreset(value); };
  const update = (change: Partial<CatalogFilters>) => setFilters(current => (saved = { ...current, ...change }));
  const preset = quick.presets.find(item => item.name === selectedPreset);
  const current = { ...(preset?.filters ?? filters), tab: searchTab ?? preset?.filters.tab ?? filters.tab, q: filters.q };
  const changeFilter = (change: Partial<CatalogFilters>) => {
    selectPreset('');
    update({ ...current, ...change });
  };
  const status = useApi<{ scopes: { label: string; display_name?: string | null }[] }>('/api/sync/status', version);
  const scopes = new Map((status.data?.scopes ?? []).filter(scope => scope.label).map(scope => [scope.label, scope.display_name || scope.label]));
  const scope = scopes.has(current.scope) ? current.scope : scopes.keys().next().value ?? '';
  const saveQuick = (next: QuickFilters) => {
    try { writeQuickFilters(next); setQuick(next); return true; }
    catch { toast(t('无法保存目录设置')); return false; }
  };
  const refresh = async () => {
    setSyncing(true);
    try {
      const runs = await post<{ status: string }[]>('/api/sync');
      const failed = runs.filter(run => run.status !== 'ok').length;
      toast(failed ? t('同步完成，{0} 个范围失败', failed) : t('同步完成'));
    } catch (error) { toast((error as Error).message); }
    finally { setSyncing(false); setVersion(value => value + 1); }
  };
  const moveToTop = (key: string) => {
    const order = [key, ...quick.order.filter(item => item !== key)];
    if (saveQuick({ ...quick, order, bottom: quick.bottom.filter(item => item !== key) })) setDisplayVersion(value => value + 1);
  };
  const toggleBottom = (key: string) => {
    const bottom = quick.bottom.includes(key) ? quick.bottom.filter(item => item !== key) : [...quick.bottom, key];
    if (saveQuick({ ...quick, bottom })) setDisplayVersion(value => value + 1);
  };
  const toggleHidden = (key: string) => {
    const hiding = !quick.hidden.includes(key);
    const hidden = hiding ? [...quick.hidden, key] : quick.hidden.filter(item => item !== key);
    if (saveQuick({ ...quick, hidden })) {
      if (hiding) setShowHidden(false);
      setDisplayVersion(value => value + 1);
    }
  };
  const savePreset = () => {
    const title = name?.trim();
    if (!title || !scope) return;
    if (quick.presets.some(preset => preset.name === title)) { toast(t('已存在同名快筛')); return; }
    const terms = [...new Set([...(preset?.terms ?? []), q.trim()].filter(Boolean))];
    if (terms.length > 32) { toast(t('快筛最多包含 32 项搜索条件')); return; }
    if (saveQuick({ ...quick, presets: [...quick.presets, { name: title, filters: { ...current, q: '', scope }, terms }] })) {
      selectPreset(title);
      setName(undefined);
    }
  };
  useEffect(() => {
    const timer = setTimeout(() => setFilters(previous => previous.q === q ? previous : (saved = { ...previous, q })), 250);
    return () => clearTimeout(timer);
  }, [q]);
  const search = new URLSearchParams();
  if (filters.q.trim()) search.set('q', filters.q.trim());
  if (preset?.terms.length) search.set('terms', JSON.stringify(preset.terms));
  const query = new URLSearchParams(search);
  for (const [key, value] of Object.entries({ scope, kind: current.kind, layer: current.layer })) query.set(key, value);
  const objects = current.tab === 'objects';
  const pending = q.trim() !== filters.q.trim();
  const path = `/api/${current.tab}?${query}`;
  const highlight = searchPattern(filters.q);
  const readSamples = () => confirmBeforeLeaving(async () => {
    setSampleProgress(t('读取中…'));
    let failed = 0;
    let updated = 0;
    let skipped = 0;
    let refreshed = false;
    try {
      const keys = await matchingObjects(page =>
        loadCatalogPage(path, page, objects ? quick.order : [], showHidden ? [] : quick.hidden, objects ? quick.bottom : []));
      let index = 0;
      // ponytail: one connection per object through the existing endpoint. Move to a server-side job if large batches need connection reuse or background execution.
      for (const key of keys) {
        setSampleProgress(t('读取示例 {0}/{1}', ++index, keys.size));
        try {
          const result = await post<{ updated: number; skipped?: boolean }>('/api/table-samples', { key, skip_recent: true });
          if (result.skipped) skipped++;
          refreshed = true;
          setSampleVersions(previous => ({ ...previous, [key]: (previous[key] ?? 0) + 1 }));
          updated += result.updated;
        } catch { failed++; }
      }
      toast(keys.size ? t('读取完成，更新 {0} 个字段，跳过 {1} 个对象，{2} 个对象失败', updated, skipped, failed) : t('没有匹配的对象'));
    } catch (error) { toast((error as Error).message); }
    finally {
      if (refreshed) setSampleVersion(value => value + 1);
      setSampleProgress(undefined);
    }
  });
  const savePresetDraft = () => {
    if (!presetDraft) return;
    const names = presetDraft.map(item => item.name.trim());
    if (names.some(value => !value)) return;
    if (new Set(names).size !== names.length) { toast(t('已存在同名快筛')); return; }
    const presets = presetDraft.map(({ filters, filterText }, index) => ({
      name: names[index], filters, terms: [...new Set(filterText.split('\n').map(term => term.trim()).filter(Boolean))],
    }));
    if (presets.some(item => item.terms.length > 32)) { toast(t('快筛最多包含 32 项搜索条件')); return; }
    if (presets.some(item => new Set(item.terms.flatMap(term => term.split(/\s+/))).size > 64)) {
      toast(t('搜索关键词不能超过 64 个'));
      return;
    }
    if (saveQuick({ ...quick, presets })) {
      selectPreset(names[presetDraft.findIndex(item => item.originalName === selectedPreset)] ?? '');
      setPresetDraft(undefined);
    }
  };
  const searchInput = useRef<HTMLInputElement>(null);
  const scroll = useRef<HTMLDivElement>(null);
  useEffect(() => { scroll.current?.scrollTo(0, 0); }, [path, version]);
  return <main className="reports-page catalog-page">
    <header className="reports-heading"><h2>{t('数据目录')}</h2></header>
    <form className="catalog-search" role="search" onSubmit={event => { event.preventDefault(); update({ q }); }}>
      <div className="toolbar-row catalog-search-row">
        {scopes.size > 0 && <div className="segmented catalog-scopes" role="group" aria-label={t('范围')}>
          {[...scopes].map(([value, text]) => <button key={value} type="button" title={value} aria-pressed={scope === value}
            onClick={() => changeFilter({ scope: value })}>{text}</button>)}
        </div>}
        <div className="segmented catalog-kinds" role="group" aria-label={t('对象类型')}>
          {['', 'table', 'view', 'mv'].map(kind => <button key={kind} type="button" aria-pressed={current.kind === kind}
            onClick={() => changeFilter({ kind })}>{kind ? label(kind) : t('全部对象')}</button>)}
        </div>
        <select value={current.layer} onChange={event => changeFilter({ layer: event.target.value })} aria-label={t('表类型')}>
          <option value="">{t('表类型')}</option>
          {['dim', 'dtl', 'fct', 'tpc', 'app', 'tmp'].map(l => <option key={l} value={l}>{label(l)}</option>)}
        </select>
        <label className="check"><input type="checkbox" checked={showHidden} onChange={event => {
          scroll.current?.scrollTo(0, 0);
          setShowHidden(event.target.checked);
        }} />{t('显示隐藏项')}</label>
        <label className="check"><input type="checkbox" checked={!objects} onChange={event => setSearchTab(savedSearchTab = event.target.checked ? 'columns' : 'objects')} />{t('搜索字段')}</label>
        {objects && <label className="check"><input type="checkbox" checked={current.expanded} onChange={event => changeFilter({ expanded: event.target.checked })} />{t('展开字段')}</label>}
        <button type="button" className="catalog-update" disabled={syncing || sampleProgress !== undefined || !scopes.size} onClick={refresh}>
          {syncing ? <><span className="busy-spinner" />{t('同步中…')}</> : t('更新表')}
        </button>
        <button type="button" disabled={!scope || pending || syncing || sampleProgress !== undefined} onClick={readSamples}>
          {sampleProgress ?? t('读取示例')}
        </button>
      </div>
      <div className="toolbar-row catalog-search-row catalog-presets">
        <div className="catalog-search-actions">
          <div className="catalog-search-field">
            <input ref={searchInput} className="search-input" type="search" value={q} onChange={event => setQ(event.target.value)} placeholder={t(objects ? '搜索表名' : '搜索字段名')} aria-label={t('搜索')} autoFocus />
            {q && <button type="button" className="catalog-search-clear icon-button" title={t('清除搜索')} aria-label={t('清除搜索')} onClick={() => {
              setQ('');
              update({ q: '' });
              searchInput.current?.focus();
            }}><Icon name="close" /></button>}
          </div>
          <button type="button" className="icon-button" disabled={!scope} title={t('保存为快筛')} aria-label={t('保存为快筛')} onClick={() => setName('')}><Icon name="add" /></button>
        </div>
        {quick.presets.length > 0 && <>
          <div className="segmented" role="group" aria-label={t('快速筛选')}>
            {quick.presets.map(preset => <button key={preset.name} type="button" aria-pressed={selectedPreset === preset.name}
              onClick={() => selectPreset(selectedPreset === preset.name ? '' : preset.name)}>{preset.name}</button>)}
          </div>
          <button type="button" className="icon-button" title={t('管理快筛')} aria-label={t('管理快筛')}
            onClick={() => setPresetDraft(quick.presets.map(item => ({ ...item, originalName: item.name, filterText: item.terms.join('\n') })))}><Icon name="edit" /></button>
        </>}
      </div>
    </form>
    <div className="catalog-scroll" ref={scroll}>
      {scope && !pending ? <CatalogResults key={`${path}:${version}:${objects ? 0 : sampleVersion}`} path={path} objects={objects} expanded={objects && current.expanded} displayVersion={displayVersion}
        root={scroll} order={quick.order} moveToTop={moveToTop} sampleVersions={sampleVersions}
        hidden={quick.hidden} showHidden={showHidden} toggleHidden={toggleHidden}
        bottom={quick.bottom} toggleBottom={toggleBottom} highlight={highlight}
        saveOrder={(keys, group) => saveQuick({ ...quick, [group]: keys })} />
        : <table className={'reports-table ' + (objects ? 'catalog-table' : 'catalog-fields')}><CatalogHead objects={objects} /><tbody>
          <TableState columns={objects ? 5 : 7} loading={pending || status.loading} error={scope ? undefined : status.error} empty />
        </tbody></table>}
    </div>
    {name !== undefined && <Modal title={t('保存为快筛')} onClose={() => setName(undefined)}
      actions={<button type="submit" form="catalog-preset-name" className="primary" disabled={!name.trim()}>{t('保存')}</button>}>
      <form id="catalog-preset-name" onSubmit={event => { event.preventDefault(); savePreset(); }}>
        <Field label={t('快筛名称')}><input autoFocus autoComplete="off" maxLength={60} value={name} onChange={event => setName(event.target.value)} /></Field>
      </form>
    </Modal>}
    {presetDraft && <Modal title={t('管理快筛')} onClose={() => setPresetDraft(undefined)}
      actions={<button type="submit" form="catalog-preset-manage" className="primary">{t('保存')}</button>}>
      <form id="catalog-preset-manage" onSubmit={event => { event.preventDefault(); savePresetDraft(); }}>
        {presetDraft.map((item, index) => <div key={item.originalName}>
          <div className="catalog-preset-edit">
          <input aria-label={t('快筛名称') + ' · ' + item.originalName} required maxLength={60} value={item.name} onChange={event => {
            setPresetDraft(presetDraft.map((entry, position) => position === index ? { ...entry, name: event.target.value } : entry));
          }} />
          {[-1, 1].map(step => <button key={step} type="button" disabled={!presetDraft[index + step]}
            title={t(step < 0 ? '上移' : '下移')} aria-label={t(step < 0 ? '上移' : '下移') + ' · ' + item.originalName} onClick={() => {
              const reordered = [...presetDraft];
              reordered.splice(index, 1);
              reordered.splice(index + step, 0, item);
              setPresetDraft(reordered);
            }}>{step < 0 ? '↑' : '↓'}</button>)}
          <button type="button" className="report-delete" onClick={() => setPresetDraft(presetDraft.filter(entry => entry !== item))}>{t('删除')}</button>
          </div>
          <Field label={t('过滤值（每行一项）')}>
            <textarea rows={2} value={item.filterText} onChange={event => {
              setPresetDraft(presetDraft.map((entry, position) => position === index ? { ...entry, filterText: event.target.value } : entry));
            }} />
          </Field>
        </div>)}
      </form>
    </Modal>}
  </main>;
}

function CatalogHead({ objects, nested, sampleAction }: { objects?: boolean; nested?: boolean; sampleAction?: React.ReactNode }) {
  return <thead>{objects
    ? <tr><th>{t('名称')}</th><th>{t('类型')}</th><th>{t('注释')}</th><th>{t('行数')}</th><th>{t('大小')}</th></tr>
    : <tr>{!nested && <th>{t('所在对象')}</th>}<th>{t('字段')}</th><th>{t('类型')}</th><th>{t('注释')}</th><th><div className="catalog-sample-head">{t('示例值')}{sampleAction}</div></th><th>{t('注意事项')}</th><th>{t('规则')}</th></tr>}
  </thead>;
}

function CatalogResults({ path, objects, expanded, root, order, moveToTop, hidden, showHidden, toggleHidden, bottom, toggleBottom, saveOrder, highlight, sampleVersions, displayVersion }: {
  path: string; objects: boolean; expanded: boolean; root: React.RefObject<HTMLDivElement>; displayVersion: number;
  order: string[]; moveToTop: (key: string) => void; saveOrder: (order: string[], group: OrderGroup) => boolean;
  hidden: string[]; showHidden: boolean; toggleHidden: (key: string) => void;
  bottom: string[]; toggleBottom: (key: string) => void;
  highlight?: RegExp;
  sampleVersions: Record<string, number>;
}) {
  const [page, setPage] = useState(0);
  const [rows, setRows] = useState<(ObjectRow | ColumnRow)[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [expandedRows, setExpandedRows] = useState<Record<string, boolean>>({});
  const [ruleTarget, setRuleTarget] = useState<RuleTarget>();
  const [ruleVersions, setRuleVersions] = useState<Record<string, number>>({});
  const displayKey = `${displayVersion}:${showHidden}`;
  const loadedDisplay = useRef(displayKey);
  const scrollTop = useRef<number>();
  const end = useRef<HTMLDivElement>(null);
  const dragging = useRef<string>();
  const [drop, setDrop] = useState<{ key: string; after: boolean }>();
  const hiddenKeys = new Set(hidden);
  const bottomKeys = new Set(bottom);
  const groupFor = (key: string): OrderGroup => bottomKeys.has(key) ? 'bottom' : 'order';
  const finishDrag = () => { dragging.current = undefined; setDrop(undefined); };
  const move = (source: number, target: number) => {
    if (!rows[source] || !rows[target]) return;
    const group = groupFor(rows[source].key);
    if (group !== groupFor(rows[target].key)) return;
    const groupRows = rows.filter(row => groupFor(row.key) === group);
    const moved = moveObject(group === 'bottom' ? bottom : order, groupRows, groupRows.indexOf(rows[source]), groupRows.indexOf(rows[target]));
    if (moved.rows !== groupRows && saveOrder(moved.order, group)) {
      let index = 0;
      setRows(rows.map(row => groupFor(row.key) === group ? moved.rows[index++] : row));
    }
  };
  const dropTarget = (event: React.DragEvent<HTMLTableSectionElement>) => {
    const row = (event.target as Element).closest<HTMLTableRowElement>('tr[data-object-key]');
    const key = row?.dataset.objectKey;
    if (!dragging.current || !key || key === dragging.current || groupFor(key) !== groupFor(dragging.current)) return;
    const bounds = row.getBoundingClientRect();
    return { key, after: event.clientY >= bounds.top + bounds.height / 2 };
  };
  useEffect(() => { setExpandedRows({}); }, [expanded]);
  useEffect(() => {
    let live = true;
    const replace = loadedDisplay.current !== displayKey;
    const load = (index: number) => loadCatalogPage(path, index, objects ? order : [], showHidden ? [] : hidden, objects ? bottom : []);
    if (replace) {
      setLoading(true);
      setError('');
    }
    Promise.all(replace ? Array.from({ length: page + 1 }, (_, index) => load(index)) : [load(page)]).then(pages => {
      if (!live) return;
      if (replace) {
        loadedDisplay.current = displayKey;
        scrollTop.current = root.current?.scrollTop;
      }
      const incoming = pages.flatMap(data => data.rows);
      setRows(previous => {
        const merged = new Map((replace ? [] : previous).map(row => [row.key, row]));
        for (const row of incoming) if (!merged.has(row.key)) merged.set(row.key, row);
        return [...merged.values()];
      });
      setHasMore((page + 1) * 10 < pages[0].total);
      setLoading(false);
    }, (error: Error) => {
      if (!live) return;
      setError(error.message);
      setLoading(false);
    });
    return () => { live = false; };
    // Moves only affect loaded rows. The next page reads the latest order.
  }, [path, page, retry, displayKey]);
  useLayoutEffect(() => {
    const viewport = root.current;
    const table = viewport?.querySelector('.catalog-table');
    if (table instanceof HTMLElement) {
      let longest = 0;
      const range = document.createRange();
      for (const name of table.querySelectorAll('.catalog-name-label>.report-name')) {
        range.selectNodeContents(name);
        longest = Math.max(longest, range.getBoundingClientRect().width);
      }
      table.style.setProperty('--catalog-name-width', `${Math.ceil(longest)}px`);
    }
    if (viewport && scrollTop.current !== undefined) viewport.scrollTop = scrollTop.current;
    scrollTop.current = undefined;
  }, [rows, root, highlight?.source]);
  useEffect(() => {
    const viewport = root.current;
    const table = viewport?.querySelector<HTMLTableElement>('.catalog-table');
    if (!viewport || !table?.tHead) return;
    const header = table.tHead;
    const groups = [...table.querySelectorAll<HTMLTableRowElement>('.catalog-expanded-row')].flatMap(fields => {
      const nested = fields.querySelector<HTMLTableElement>('.catalog-nested');
      const row = fields.previousElementSibling as HTMLTableRowElement;
      return nested?.tHead ? [{ fields, row, nested, head: nested.tHead }] : [];
    });
    if (!groups.length) return;
    let sticky: (typeof groups)[number] | undefined;
    const clear = () => {
      sticky?.row.style.removeProperty('--catalog-sticky-offset');
      sticky?.head.style.removeProperty('--catalog-sticky-offset');
    };
    const position = () => {
      const top = viewport.getBoundingClientRect().top + header.offsetHeight;
      for (const group of groups) {
        const { fields, row, nested, head } = group;
        const bounds = fields.getBoundingClientRect();
        if (bounds.bottom <= top || bounds.top - row.offsetHeight >= top) continue;
        // The bottom of the field header cannot move past the end of this field group.
        const offset = Math.max(0, Math.min(bounds.bottom - nested.getBoundingClientRect().top - head.offsetHeight, top - bounds.top + row.offsetHeight));
        if (sticky !== group) clear();
        row.style.setProperty('--catalog-sticky-offset', `${offset}px`);
        head.style.setProperty('--catalog-sticky-offset', `${offset}px`);
        sticky = group;
        return;
      }
      clear();
      sticky = undefined;
    };
    const observer = new ResizeObserver(position);
    observer.observe(viewport);
    observer.observe(header);
    for (const { fields, head, row } of groups) {
      observer.observe(fields);
      observer.observe(head);
      observer.observe(row);
    }
    viewport.addEventListener('scroll', position, { passive: true });
    position();
    return () => {
      observer.disconnect();
      viewport.removeEventListener('scroll', position);
      clear();
    };
  }, [root, rows, expanded, expandedRows]);
  useEffect(() => {
    if (loading || error || !hasMore || !end.current) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (!entry.isIntersecting) return;
      observer.disconnect();
      setLoading(true);
      setPage(p => p + 1);
    }, { root: root.current, rootMargin: '0px 0px 80px 0px' });
    observer.observe(end.current);
    return () => observer.disconnect();
  }, [loading, error, hasMore, root]);
  return <>
    <table className={'reports-table ' + (objects ? 'catalog-table' : 'catalog-fields')}>
      <CatalogHead objects={objects} />
      <tbody onDragOver={event => {
        const target = dropTarget(event);
        if (target) {
          event.preventDefault();
          event.dataTransfer.dropEffect = 'move';
        }
        setDrop(current => current?.key === target?.key && current?.after === target?.after ? current : target);
      }} onDragLeave={event => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDrop(undefined);
      }} onDrop={event => {
        const target = dropTarget(event);
        if (target) {
          event.preventDefault();
          const source = rows.findIndex(row => row.key === dragging.current);
          const insertion = rows.findIndex(row => row.key === target.key) + Number(target.after);
          move(source, insertion - Number(source < insertion));
        }
        finishDrag();
      }}>
        <TableState columns={objects ? 5 : 7} loading={loading} error={error || undefined} empty={!rows.length} />
        {rows.map((row, index) => 'object_key' in row
          ? <CatalogColumn key={row.key} row={row} parent={row.object_key} openRules={setRuleTarget} highlight={highlight} />
          : <CatalogObject key={row.key} row={row} expanded={expandedRows[row.key] ?? expanded} openRules={setRuleTarget} version={ruleVersions[row.key] ?? 0} batchSampleVersion={sampleVersions[row.key] ?? 0}
            onToggle={() => setExpandedRows(current => ({ ...current, [row.key]: !(current[row.key] ?? expanded) }))}
            highlight={highlight}
            onTop={() => moveToTop(row.key)}
            hidden={hiddenKeys.has(row.key)} onHide={() => toggleHidden(row.key)}
            bottom={bottomKeys.has(row.key)} onBottom={() => toggleBottom(row.key)}
            dropSide={drop?.key === row.key ? drop.after ? 'after' : 'before' : undefined}
            orderHandle={<button type="button" className="catalog-grip" draggable
              title={t('拖拽调整顺序，也可用方向键移动')} aria-label={t('调整对象顺序') + ' · ' + row.name} aria-keyshortcuts="ArrowUp ArrowDown"
              onDragStart={event => {
                dragging.current = row.key;
                event.dataTransfer.effectAllowed = 'move';
                event.dataTransfer.setData('text/plain', row.key);
              }} onDragEnd={finishDrag} onKeyDown={event => {
                if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
                event.preventDefault();
                move(index, index + (event.key === 'ArrowUp' ? -1 : 1));
              }}>⠿</button>} />)}
      </tbody>
    </table>
    <div className="table-pagination" ref={end} aria-live="polite">
      {loading && rows.length > 0 && <span>{t('加载中…')}</span>}
      {error && <button onClick={() => { setError(''); setLoading(true); setRetry(n => n + 1); }}>{t('重试')}</button>}
    </div>
    {ruleTarget && <CatalogRules target={ruleTarget} onClose={() => setRuleTarget(undefined)} onChanged={() => setRuleVersions(versions => ({ ...versions, [ruleTarget.objectKey]: (versions[ruleTarget.objectKey] ?? 0) + 1 }))} />}
  </>;
}

type RuleTarget = { objectKey: string; key: string; name: string };
type Rule = { id: number; target_key: string; kind: string; value?: string; condition?: string; note?: string };
type CatalogDetail = { columns: ColumnRow[]; rules: Rule[] };

function marked(text: string, pattern?: RegExp) {
  return pattern ? text.split(pattern).map((part, index) => index % 2 ? <mark className="catalog-match" key={index}>{part}</mark> : part) : text;
}

function CatalogObject({ row, expanded, openRules, version, batchSampleVersion, onToggle, orderHandle, dropSide, onTop, hidden, onHide, bottom, onBottom, highlight }: {
  row: ObjectRow; expanded: boolean;
  openRules: (target: RuleTarget) => void; version: number; batchSampleVersion: number; onToggle: () => void;
  orderHandle: React.ReactNode; dropSide?: 'before' | 'after';
  onTop: () => void;
  hidden: boolean; onHide: () => void;
  bottom: boolean; onBottom: () => void;
  highlight?: RegExp;
}) {
  const [reading, setReading] = useState(false);
  const [sampleVersion, setSampleVersion] = useState(0);
  const detailPath = '/api/object?key=' + encodeURIComponent(row.key);
  const detail = useApi<CatalogDetail>(expanded ? detailPath : null, version + sampleVersion + batchSampleVersion);
  const columns = detail.data?.columns.filter(column => !column.is_deleted) || [];
  const count = row.props?.rows ?? row.props?.rows_estimate;
  const shortCount = typeof count === 'number' && count >= 999950 ? (count / 1000000).toFixed(1) + 'M'
    : typeof count === 'number' && count >= 1000 ? (count / 1000).toFixed(1) + 'K' : number(count);
  const topLabel = t('移到顶部');
  const hideLabel = t(hidden ? '取消隐藏' : '隐藏');
  const bottomLabel = t(bottom ? '取消置底' : '移到底部');
  const copyStructure = async () => {
    try {
      const text = api<CatalogDetail>(detailPath, { cache: 'no-store' }).then(data =>
        tableStructureTsv(row.name, [t('表名'), t('字段名'), t('字段类型'), t('示例值')], data.columns));
      if (typeof ClipboardItem !== 'undefined' && navigator.clipboard.write) {
        const blob = text.then(value => new Blob([value], { type: 'text/plain' }));
        await Promise.all([blob, navigator.clipboard.write([new ClipboardItem({ 'text/plain': blob })])]);
      } else {
        await navigator.clipboard.writeText(await text);
      }
      toast(t('已复制'));
    } catch { toast(t('复制失败')); }
  };
  const readSamples = async () => {
    setReading(true);
    try {
      const { updated } = await post<{ updated: number }>('/api/table-samples', { key: row.key });
      setSampleVersion(value => value + 1);
      toast(updated ? t('已读取 {0} 个字段的示例值', updated) : t('没有可更新的示例值'));
    } catch (error) { toast((error as Error).message); }
    finally { setReading(false); }
  };
  return <>
    <tr data-object-key={row.key} data-drop-side={dropSide} className={[row.is_deleted ? 'row-deleted' : '', hidden ? 'catalog-hidden' : ''].join(' ')} onClick={event => {
      if (event.target instanceof Element && !event.target.closest('button,a,input,textarea,select')) onToggle();
    }}>
      <td title={row.cn_name ? row.name + ' · ' + row.cn_name : row.name}>
        <div className="catalog-object-name">
          {orderHandle}
          <button className="catalog-disclosure" aria-expanded={expanded} aria-label={t(expanded ? '收起字段' : '展开字段') + ' · ' + row.name} onClick={onToggle}>{expanded ? '▾' : '▸'}</button>
          <div className="catalog-name-label">
            <button className="report-name" onClick={() => openObject(row.key)}>{marked(row.name, highlight)}</button>
            {isTemporary(row.name) && <span className="tag">{t('临时')}</span>}{row.is_deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}
          </div>
          <button type="button" className="catalog-row-action catalog-top icon-button" title={topLabel} aria-label={topLabel + ' · ' + row.name} onClick={onTop}>
            <Icon name="top" />
          </button>
          <button type="button" className="catalog-row-action catalog-hide icon-button" title={hideLabel} aria-label={hideLabel + ' · ' + row.name} onClick={onHide}>
            <Icon name={hidden ? 'view' : 'view-off'} />
          </button>
          <button type="button" className="catalog-row-action catalog-bottom icon-button" title={bottomLabel} aria-label={bottomLabel + ' · ' + row.name} onClick={onBottom}>
            <Icon name={bottom ? 'bottom-off' : 'bottom'} />
          </button>
          <button type="button" className="catalog-row-action icon-button" title={t('复制表名')} aria-label={t('复制表名') + ' · ' + row.name} onClick={async () => {
            try { await navigator.clipboard.writeText(row.name); toast(t('已复制')); }
            catch { toast(t('复制失败')); }
          }}><Icon name="copy" /></button>
          <button type="button" className="catalog-row-action icon-button" title={t('复制表结构')} aria-label={t('复制表结构') + ' · ' + row.name} onClick={copyStructure}>
            <Icon name="table" />
          </button>
        </div>
        {row.cn_name && <small className="catalog-cn muted">{row.cn_name}</small>}
      </td>
      <td>{label(row.kind)}</td>
      <EditableCell rowKey={row.key} field="description" value={row.description} fallback={row.comment} labelText={t('注释')} />
      <td title={number(count)}>{shortCount}</td><td>{bytes(row.props?.size_bytes)}</td>
    </tr>
    {expanded && <tr className="catalog-expanded-row"><td className="catalog-expanded" colSpan={5}>
      <table className="reports-table catalog-fields catalog-nested" aria-label={t('字段') + ' · ' + row.name}>
        <CatalogHead nested sampleAction={<button type="button" disabled={reading || detail.loading || !columns.length || !!row.is_deleted} onClick={readSamples}>
          {reading ? t('读取中…') : t('读取')}
        </button>} />
        <tbody>
          <TableState columns={6} loading={detail.loading} error={detail.error} empty={!columns.length} />
          {columns.map(column => <CatalogColumn key={column.key} row={column} parent={row.key} nested sampleVersion={sampleVersion + batchSampleVersion} openRules={openRules} rules={detail.data?.rules.filter(rule => rule.target_key === column.key)} />)}
        </tbody>
        <tfoot><tr><td colSpan={5}>{t('对象规则')}</td><RulesCell rules={detail.data?.rules.filter(rule => rule.target_key === row.key)} onClick={() => openRules({ objectKey: row.key, key: row.key, name: row.name })} /></tr></tfoot>
      </table>
    </td></tr>}
  </>;
}

function CatalogColumn({ row, parent, nested, openRules, rules, highlight, sampleVersion }: { row: ColumnRow; parent: string; nested?: boolean; openRules: (target: RuleTarget) => void; rules?: Rule[]; highlight?: RegExp; sampleVersion?: number }) {
  return <tr className={row.is_deleted ? 'row-deleted' : ''}>
    {!nested && <td><button className="report-name" title={row.object_name} onClick={() => openObject(parent, 'columns')}>{row.object_name}</button></td>}
    <td title={row.name}>{marked(row.name, highlight)}{row.is_deleted ? <span className="tag tag-danger">{t('已删除')}</span> : null}</td><td title={row.data_type}>{columnType(row.data_type, row.props)}</td>
    <EditableCell rowKey={row.key} field="meaning" value={row.meaning} fallback={row.comment} labelText={t('注释')} />
    <EditableCell key={sampleVersion} rowKey={row.key} field="sample" value={row.sample} labelText={t('示例值')} />
    <EditableCell rowKey={row.key} field="notes" value={row.notes} labelText={t('注意事项')} />
    <RulesCell rules={rules} onClick={() => openRules({ objectKey: parent, key: row.key, name: row.name })} />
  </tr>;
}

function RulesCell({ rules, onClick }: { rules?: Rule[]; onClick: () => void }) {
  const summary = rules?.map(rule => [label(rule.kind), rule.value || rule.condition || rule.note].filter(Boolean).join(' ')).join(' · ');
  return <td title={summary}><button className="report-name" onClick={onClick} aria-label={summary ? t('编辑规则') + ' · ' + summary : t('编辑规则')}>{summary || t('编辑规则')}</button></td>;
}

function CatalogRules({ target, onClose, onChanged }: { target: RuleTarget; onClose: () => void; onChanged: () => void }) {
  const detail = useApi<CatalogDetail>('/api/object?key=' + encodeURIComponent(target.objectKey));
  return <Modal wide title={target.name + ' · ' + t('规则')} onClose={onClose} actions={<button onClick={onClose}>{t('关闭')}</button>}>
    {detail.error && <p role="alert" className="source-error">{detail.error}</p>}
    {detail.loading && <p className="source-hint">{t('加载中…')}</p>}
    {detail.data && <RuleEditor target={target.key} rules={detail.data.rules.filter(rule => rule.target_key === target.key)} kinds={target.objectKey === target.key ? ['filter', 'note'] : ['valid', 'invalid', 'filter', 'note']} onChanged={onChanged} />}
  </Modal>;
}

function EditableCell({ rowKey, field, value, fallback, labelText }: { rowKey: string; field: 'description' | 'meaning' | 'sample' | 'notes'; value?: string | null; fallback?: string | null; labelText: string }) {
  const objectComment = field === 'description';
  const [savedValue, setSavedValue] = useState<string | null>();
  const [draft, setDraft] = useState<string>();
  const [busy, setBusy] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);
  const manual = savedValue === undefined ? value : savedValue;
  const display = manual ?? fallback ?? '';
  useEffect(() => {
    if (draft === undefined && restoreFocus.current) {
      restoreFocus.current = false;
      button.current?.focus();
    }
  }, [draft]);
  const close = () => { restoreFocus.current = true; setDraft(undefined); };
  const save = async (text: string) => {
    if (text === display) { close(); return; }
    setBusy(true);
    try {
      const stored = text.trim() ? text : objectComment ? '' : null;
      await post('/api/m/' + (objectComment ? 'object_biz' : 'column_biz'), { key: rowKey, [field]: stored }, 'PUT');
      setSavedValue(stored);
      close();
      toast(t('已保存'));
    } catch (error) { toast((error as Error).message); }
    finally { setBusy(false); }
  };
  const beginEdit = () => { if (draft === undefined) setDraft(display); };
  return <td className={objectComment ? 'catalog-edit catalog-object-comment' : 'catalog-edit'} title={display}
    onDoubleClick={objectComment ? undefined : beginEdit}>
    {draft === undefined ? <button ref={button} className={manual == null && fallback ? 'inherited' : ''} aria-label={t('编辑{0}', labelText) + (display ? ' · ' + display : '')} onDoubleClick={objectComment ? beginEdit : undefined} onClick={event => { if (event.detail === 0) setDraft(display); }}>{display || '—'}</button>
      : <textarea autoFocus rows={2} aria-label={labelText} value={draft} readOnly={busy} onChange={event => setDraft(event.target.value)} onKeyDown={event => {
        if (event.nativeEvent.isComposing || event.keyCode === 229) return;
        if (event.key === 'Escape' && !busy) { event.preventDefault(); setDraft(undefined); }
        if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); if (!busy) void save(draft); }
      }} />}
  </td>;
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
