import { useState } from 'react';
import { del, post, toast } from './api';
import { openObject, TableState, useApi } from './Catalog';
import { confirmAction } from './confirmAction';
import { t } from './i18n';
import { Field, Graph, Modal, ObjectPicker } from './ui';

type Step = { id?: number; product: string; flow: string; step_no: number; name: string; object_key?: string | null; columns?: string | null; note?: string | null };

/** Business flows per product (F5): ordered steps linked to landing tables and key columns, shown as a flow chart. */
export function FlowsPage() {
  const [version, setVersion] = useState(0);
  const steps = useApi<Step[]>('/api/m/flow_step', version);
  const settings = useApi<any>('/api/settings');
  const all = steps.data || [];
  const products = [...new Set([...Object.values(settings.data?.naming?.products || {}) as string[], '公共', ...all.map(s => s.product)])];
  const [product, setProduct] = useState('');
  const current = product || products[0] || '';
  const flows = [...new Set(all.filter(s => s.product === current).map(s => s.flow))];
  const [flow, setFlow] = useState('');
  const currentFlow = flows.includes(flow) ? flow : flows[0] || '';
  const list = all.filter(s => s.product === current && s.flow === currentFlow).sort((a, b) => a.step_no - b.step_no);
  const [editing, setEditing] = useState<Step | null>(null);
  const reload = () => setVersion(v => v + 1);
  const remove = async (s: Step) => {
    if (!await confirmAction(t('删除步骤“{0}”？', s.name))) return;
    try { await del('/api/m/flow_step/' + s.id); reload(); } catch (e) { toast((e as Error).message); }
  };
  const nodes = list.map((s, i) => ({ key: 'step:' + s.id, name: `${s.step_no}. ${s.name}`, depth: i, kind: '', scope: s.object_key?.split('.').pop() || t('未关联表'), external: !s.object_key }));
  const edges = nodes.slice(1).map((n, i) => ({ from: nodes[i].key, to: n.key }));
  return <main className="reports-page">
    <header className="reports-heading"><h2>{t('业务流程')}</h2>
      <button className="primary" disabled={!current} onClick={() => setEditing({ product: current, flow: currentFlow, step_no: (list.at(-1)?.step_no || 0) + 1, name: '' })}>{t('添加步骤')}</button>
    </header>
    <div className="toolbar-row">
      <select value={current} onChange={e => { setProduct(e.target.value); setFlow(''); }} aria-label={t('产品')}>{products.map(p => <option key={p}>{p}</option>)}</select>
      <select value={currentFlow} onChange={e => setFlow(e.target.value)} aria-label={t('流程')}>{flows.length ? flows.map(f => <option key={f}>{f}</option>) : <option value="">{t('暂无流程')}</option>}</select>
      <button onClick={() => setEditing({ product: current, flow: '', step_no: 1, name: '' })}>{t('新建流程')}</button>
    </div>
    <section className="settings-card">
      {steps.error ? <p role="alert" className="source-error">{steps.error}</p> : <Graph nodes={nodes} edges={edges} onOpen={n => { const s = list.find(x => 'step:' + x.id === n.key); if (s?.object_key) openObject(s.object_key); }} />}
      <p className="source-hint">{t('点击步骤跳转到对应的表。')}</p>
    </section>
    <table className="reports-table">
      <thead><tr><th className="narrow">#</th><th>{t('步骤')}</th><th>{t('落地表')}</th><th>{t('关键字段')}</th><th>{t('说明')}</th><th>{t('操作')}</th></tr></thead>
      <tbody>
        <TableState columns={6} loading={steps.loading} empty={!list.length} />
        {list.map(s => <tr key={s.id}><td>{s.step_no}</td><td>{s.name}</td>
          <td title={s.object_key || ''}>{s.object_key ? <button className="report-name" onClick={() => openObject(s.object_key!)}>{s.object_key.split('.').pop()}</button> : '—'}</td>
          <td>{s.columns}</td><td title={s.note || ''}>{s.note}</td>
          <td><div className="report-actions"><button onClick={() => setEditing(s)}>{t('编辑')}</button><button className="report-delete" onClick={() => remove(s)}>{t('删除')}</button></div></td></tr>)}
      </tbody>
    </table>
    {editing && <StepModal step={editing} onClose={() => setEditing(null)} onSaved={(p, f) => { setProduct(p); setFlow(f); reload(); }} />}
  </main>;
}

function StepModal({ step, onClose, onSaved }: { step: Step; onClose: () => void; onSaved: (product: string, flow: string) => void }) {
  const [form, setForm] = useState<Step>(step);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    if (!form.product.trim() || !form.flow.trim() || !form.name.trim()) { toast(t('产品、流程名和步骤名不能为空')); return; }
    setBusy(true);
    try { await post('/api/m/flow_step', { ...form, step_no: Number(form.step_no) || 1 }, 'PUT'); onSaved(form.product, form.flow); onClose(); }
    catch (e) { toast((e as Error).message); } finally { setBusy(false); }
  };
  return <Modal title={step.id ? t('编辑步骤') : t('添加步骤')} onClose={onClose} actions={<><button onClick={onClose}>{t('取消')}</button><button className="primary" disabled={busy} onClick={save}>{t('保存')}</button></>}>
    <div className="form-grid">
      <Field label={t('产品')}><input value={form.product} onChange={e => setForm({ ...form, product: e.target.value })} /></Field>
      <Field label={t('流程名')}><input value={form.flow} placeholder={t('如 新手流程')} onChange={e => setForm({ ...form, flow: e.target.value })} /></Field>
      <Field label={t('步骤序号')}><input type="number" min={1} value={form.step_no} onChange={e => setForm({ ...form, step_no: Number(e.target.value) })} /></Field>
      <Field label={t('步骤名')}><input value={form.name} placeholder={t('如 首次启动')} onChange={e => setForm({ ...form, name: e.target.value })} /></Field>
      <Field label={t('落地表')} wide><ObjectPicker value={form.object_key || ''} onChange={key => setForm({ ...form, object_key: key })} /></Field>
      <Field label={t('关键字段')} wide><input value={form.columns || ''} placeholder={t('多个字段用逗号分隔')} onChange={e => setForm({ ...form, columns: e.target.value })} /></Field>
      <Field label={t('说明')} wide><textarea rows={2} value={form.note || ''} onChange={e => setForm({ ...form, note: e.target.value })} /></Field>
    </div>
  </Modal>;
}
