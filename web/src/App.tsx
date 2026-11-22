import { useEffect, useState } from 'react';
import { Icon } from './Icon';
import { t, useLanguage } from './i18n';
import { CatalogPage } from './Catalog';
import { ObjectPage } from './ObjectPage';
import { ChangesPage, TodosPage } from './Activity';
import { SyncPage } from './SyncPage';
import { SettingsPage } from './SettingsPage';
import { LineagePage } from './LineagePage';
import { RelationsPage } from './Relations';
import { FlowsPage } from './FlowsPage';
import { MetricsPage } from './MetricsPage';
import { RefreshPage } from './RefreshPage';

// Hash routes keep the ?mock preview flag in the query string intact.
function useRoute() {
  const read = () => location.hash.replace(/^#\/?/, '') || 'catalog';
  const [route, setRoute] = useState(read);
  useEffect(() => { const update = () => setRoute(read()); window.addEventListener('hashchange', update); return () => window.removeEventListener('hashchange', update); }, []);
  const [path, query = ''] = route.split('?');
  return { page: path.split('/')[0], params: new URLSearchParams(query), route };
}

export const go = (path: string) => { location.hash = '/' + path; };

const nav = [
  { page: 'catalog', icon: 'datasets', text: '目录' },
  { page: 'lineage', icon: 'tree', text: '血缘' },
  { page: 'relations', icon: 'link', text: '关联' },
  { page: 'flows', icon: 'sankey', text: '流程' },
  { page: 'metrics', icon: 'kpi', text: '指标' },
  { page: 'refresh', icon: 'schedule', text: '刷新' },
  { page: 'todos', icon: 'check', text: '待办' },
  { page: 'changes', icon: 'history', text: '变更' },
  { page: 'sync', icon: 'sync', text: '同步' },
];

export default function App() {
  useLanguage();
  const { page, params, route } = useRoute();
  const [message, setMessage] = useState<{ text: string; id: number } | null>(null);
  useEffect(() => {
    let timer = 0;
    const show = (event: Event) => { clearTimeout(timer); setMessage({ text: (event as CustomEvent).detail, id: Date.now() }); timer = window.setTimeout(() => setMessage(null), 3000); };
    window.addEventListener('mymeta-toast', show);
    return () => window.removeEventListener('mymeta-toast', show);
  }, []);
  const active = page === 'object' ? 'catalog' : page;
  return <div className="app-shell">
    <nav className="rail" aria-label={t('导航')}>
      {nav.map(item => <button key={item.page} className={active === item.page ? 'rail-active' : ''} onClick={() => go(item.page)} title={t(item.text)}><Icon name={item.icon} /><span>{t(item.text)}</span></button>)}
      <button className={'rail-settings' + (active === 'settings' ? ' rail-active' : '')} onClick={() => go('settings')} title={t('设置')}><Icon name="settings" /><span>{t('设置')}</span></button>
    </nav>
    {page === 'object' ? <ObjectPage key={route} objectKey={params.get('key') || ''} tab={params.get('tab')} />
      : page === 'lineage' ? <LineagePage key={params.get('key') || ''} initialKey={params.get('key') || ''} />
      : page === 'relations' ? <RelationsPage />
      : page === 'flows' ? <FlowsPage />
      : page === 'metrics' ? <MetricsPage />
      : page === 'refresh' ? <RefreshPage />
      : page === 'todos' ? <TodosPage />
      : page === 'changes' ? <ChangesPage run={params.get('run')} />
      : page === 'sync' ? <SyncPage />
      : page === 'settings' ? <SettingsPage />
      : <CatalogPage />}
    {message && <div key={message.id} className="workbench-message" role="status">{message.text}</div>}
  </div>;
}
