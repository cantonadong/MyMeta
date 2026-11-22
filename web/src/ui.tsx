import { ReactNode, useEffect, useId, useRef, useState } from 'react';
import { api, Page } from './api';
import { label } from './format';
import { t } from './i18n';

/** Modal dialog in MyBI style: title, body, actions right-aligned (cancel → primary). */
export function Modal({ title, children, actions, onClose, wide }: { title: string; children: ReactNode; actions: ReactNode; onClose: () => void; wide?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { ref.current?.showModal(); }, []);
  return <dialog ref={ref} className={'modal' + (wide ? ' modal-wide' : '')} onCancel={event => { event.preventDefault(); onClose(); }}>
    <h2>{title}</h2>
    <div className="modal-body">{children}</div>
    <div className="modal-actions">{actions}</div>
  </dialog>;
}

export function Tabs({ tabs, value, onChange }: { tabs: [string, string][]; value: string; onChange: (value: string) => void }) {
  return <div className="segmented" role="tablist">{tabs.map(([key, text]) => <button key={key} role="tab" aria-selected={value === key} onClick={() => onChange(key)}>{text}</button>)}</div>;
}

/** Text input with suggestions from the object catalog; the value is the object key. */
export function ObjectPicker({ value, onChange, placeholder, includeTmp }: { value: string; onChange: (key: string) => void; placeholder?: string; includeTmp?: boolean }) {
  const id = useId();
  const [text, setText] = useState(value);
  const [options, setOptions] = useState<{ key: string; name: string; cn_name?: string }[]>([]);
  useEffect(() => setText(value), [value]);
  useEffect(() => {
    if (!text.trim() || text === value) return;
    const timer = setTimeout(() => api<Page<any>>(`/api/objects?q=${encodeURIComponent(text.split('.').pop() || text)}&tmp=${!!includeTmp}`).then(r => setOptions(r.rows), () => setOptions([])), 200);
    return () => clearTimeout(timer);
  }, [text]);
  return <>
    <input className="object-picker" list={id} value={text} placeholder={placeholder || t('搜索对象')} onChange={event => { setText(event.target.value); if (options.some(o => o.key === event.target.value)) onChange(event.target.value); }} onBlur={() => { if (text !== value && options.some(o => o.key === text)) onChange(text); }} />
    <datalist id={id}>{options.map(o => <option key={o.key} value={o.key}>{o.cn_name || o.name}</option>)}</datalist>
  </>;
}

export function Field({ label: text, children, wide }: { label: string; children: ReactNode; wide?: boolean }) {
  return <label className={'form-field' + (wide ? ' form-wide' : '')}><span>{text}</span>{children}</label>;
}

export type GraphNode = { key: string; name: string; depth: number; kind?: string; layer?: string | null; deleted?: boolean; external?: boolean; cn_name?: string | null; scope?: string };
export type GraphEdge = { from: string; to: string; label?: string };

const W = 210, H = 44, GAP_X = 70, GAP_Y = 14;

/** Layered left-to-right graph: columns by depth, curved edges, click a node to open it. */
export function Graph({ nodes, edges, center, onOpen }: { nodes: GraphNode[]; edges: GraphEdge[]; center?: string; onOpen?: (node: GraphNode) => void }) {
  if (!nodes.length) return <p className="reports-empty graph-empty">{t('暂无数据')}</p>;
  const depths = [...new Set(nodes.map(n => n.depth))].sort((a, b) => a - b);
  const columns = depths.map(d => nodes.filter(n => n.depth === d).sort((a, b) => a.name.localeCompare(b.name)));
  const tallest = Math.max(...columns.map(c => c.length));
  const height = tallest * (H + GAP_Y) + GAP_Y;
  const position = new Map<string, { x: number; y: number }>();
  columns.forEach((column, ci) => {
    const offset = (height - column.length * (H + GAP_Y)) / 2;
    column.forEach((node, ri) => position.set(node.key, { x: 10 + ci * (W + GAP_X), y: offset + GAP_Y / 2 + ri * (H + GAP_Y) }));
  });
  const width = columns.length * (W + GAP_X) - GAP_X + 20;
  return <div className="graph-scroll">
    <svg width={width} height={height} role="img" aria-label={t('关系图')}>
      <defs><marker id="arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0 8 4 0 8z" fill="#aebbf2" /></marker></defs>
      {edges.map((edge, i) => {
        const a = position.get(edge.from), b = position.get(edge.to);
        if (!a || !b) return null;
        const forward = b.x > a.x;
        const [x1, y1, x2, y2] = forward ? [a.x + W, a.y + H / 2, b.x, b.y + H / 2] : a.x === b.x ? [a.x + W, a.y + H / 2, b.x + W, b.y + H / 2] : [a.x, a.y + H / 2, b.x + W, b.y + H / 2];
        const bend = a.x === b.x ? 60 : (x2 - x1) / 2;
        const path = `M${x1} ${y1} C${x1 + bend} ${y1} ${x2 - (a.x === b.x ? -60 : bend)} ${y2} ${x2} ${y2}`;
        return <g key={i}><path d={path} className="graph-edge" markerEnd="url(#arrow)" />{edge.label && <text x={(x1 + x2) / 2} y={(y1 + y2) / 2 - 4} className="graph-edge-label">{edge.label}</text>}</g>;
      })}
      {nodes.map(node => {
        const p = position.get(node.key)!;
        const cls = ['graph-node', node.key === center && 'graph-center', node.deleted && 'graph-deleted', node.external && 'graph-external', onOpen && 'graph-clickable'].filter(Boolean).join(' ');
        return <g key={node.key} className={cls} transform={`translate(${p.x},${p.y})`} onClick={() => onOpen?.(node)} tabIndex={onOpen ? 0 : undefined} onKeyDown={event => event.key === 'Enter' && onOpen?.(node)}>
          <title>{node.key}{node.cn_name ? `\n${node.cn_name}` : ''}</title>
          <rect width={W} height={H} rx={7} />
          <text x={10} y={18} className="graph-name">{node.name.length > 30 ? node.name.slice(0, 29) + '…' : node.name}</text>
          <text x={10} y={34} className="graph-meta">{[label(node.kind), node.layer, node.scope].filter(Boolean).join(' · ').slice(0, 38)}</text>
        </g>;
      })}
    </svg>
  </div>;
}

/** Client-side CSV download (UTF-8 with BOM so Excel opens Chinese text correctly). */
export function downloadCsv(name: string, header: string[], rows: unknown[][]) {
  const escape = (v: unknown) => { const s = v === null || v === undefined ? '' : String(v); return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s; };
  const text = '﻿' + [header, ...rows].map(r => r.map(escape).join(',')).join('\n');
  const link = document.createElement('a');
  link.href = URL.createObjectURL(new Blob([text], { type: 'text/csv' }));
  link.download = name;
  link.click();
  URL.revokeObjectURL(link.href);
}

/** 10-row client-side pagination for lists already loaded in full. */
export function usePaged<T>(items: T[]) {
  const [page, setPage] = useState(0);
  useEffect(() => { if (page * 10 >= items.length && page > 0) setPage(0); }, [items.length]);
  return { page, setPage, rows: items.slice(page * 10, page * 10 + 10), total: items.length };
}
