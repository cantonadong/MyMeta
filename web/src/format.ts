import { getLocale, t } from './i18n';

export const utc = (value?: string | null) => value ? value.replace('T', ' ').replace(/:\d\dZ$/, '') + ' UTC' : '—';
export const number = (value: unknown) => typeof value === 'number' && value >= 0 ? value.toLocaleString(getLocale()) : '—';
export function bytes(value: unknown) {
  if (typeof value !== 'number' || value < 0) return '—';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let size = value, unit = 0;
  while (size >= 1024 && unit < units.length - 1) { size /= 1024; unit++; }
  return `${size.toFixed(unit ? 1 : 0)} ${units[unit]}`;
}
export function duration(start?: string, end?: string | null) {
  if (!start || !end) return '—';
  const seconds = Math.round((Date.parse(end) - Date.parse(start)) / 1000);
  return seconds < 60 ? t('{0} 秒', seconds) : t('{0} 分 {1} 秒', Math.floor(seconds / 60), seconds % 60);
}

// Code values from the API → display labels (Chinese source text, translated by t()).
const labels: Record<string, string> = {
  table: '表', view: '视图', mv: '物化视图', '1:1': '一对一', '1:n': '一对多', 'n:n': '多对多',
  new_object: '新对象待补业务元数据', new_column: '新字段待补含义', deleted_object: '删除对象待确认归档', deleted_column: '删除字段待确认归档',
  status: '状态', kind: '对象类型', comment: '注释', definition: '定义', data_type: '字段类型', nullable: '可空',
  created: '新增', deleted: '已删除', present: '存在', restored: '恢复', true: '是', false: '否',
  startup: '启动', schedule: '定时', manual: '手动', ok: '成功', error: '失败', running: '进行中', open: '待处理', done: '已完成', ignored: '已忽略',
  sync: '同步', admin: '人工', object: '对象', column: '字段',
  diststyle: '分布方式', sortkey: '排序键', sortkey_num: '排序键数', size_bytes: '大小', rows: '行数', rows_estimate: '行数估计', encoded: '已压缩编码',
  dim: 'dim 维度', dtl: 'dtl 明细', fct: 'fct 事实', tpc: 'tpc 主题', app: 'app 应用', tmp: 'tmp 临时',
  name_too_long: '名称超过 63 字符', layer_synonym: '层级用了非标准写法（如 topic / detail）', layer_unknown: '无法识别层级', suffix_nonstandard: '后缀不规范（应为 _v / _mv）',
  suffix_kind_mismatch: '后缀与对象类型不符', tmp_no_date: '临时对象名缺少 yyyymmdd 日期', grain_synonym: '粒度用了缩写（如 _d）', subject_missing: '缺少主题',
  comment_conflict: '注释冲突待选择', check_downstream: '字段类型变化，需检查下游', possible_rename: '可能是表重命名', tmp_unrecognized: '临时对象需人工处理', tmp_drop_failed: '临时对象删除失败',
  valid: '有效值', invalid: '无效值', filter: '必加过滤', note: '注意事项',
  script: '脚本', mybi: 'MyBI', mybi_task: 'MyBI 定时任务', cleanup: '自动清理', mybi_dataset: 'MyBI 数据集', mybi_report: 'MyBI 看板', unknown: '未同步对象',
  object_biz: '业务信息', column_biz: '字段业务信息', rule: '规则', relation: '关联', flow_step: '流程步骤', metric: '指标', system: '系统', lineage: '血缘', drop: '删除对象', renamed: '重命名',
  cn_name: '中文名', description: '用途说明', product: '产品', layer: '层级', grain: '粒度', source_tables: '来源表', refresh_method: '刷新方式', maintainer: '维护人',
  is_temporary: '临时对象', date_column: '日期字段', meaning: '业务含义', unit: '单位', currency: '币种', timezone: '时区与切日', is_sensitive: '敏感字段', notes: '注意事项',
  value: '取值', condition: 'SQL 条件', target_key: '目标', key_pairs: '关联键', diff_note: '口径差异', confirmed: '已确认', suggested: '推荐', rejected: '已拒绝', join: 'JOIN 推荐',
  unrecognized: '名称不符合识别规则', invalid_date: '日期无效', expired: '已超期', not_expired: '未超期', keep: '保留', import: '导入', rebuild: '重新解析',
  name_cn: '中文名', name_en: '英文名', formula: '计算公式', numerator: '分子', denominator: '分母', used_by: '使用方', step_no: '步骤序号', name: '名称', flow: '流程',
  primary_key: '主键', indexes: '索引', mv_state: 'MV 状态', auto_refresh: '自动刷新', is_stale: '数据已过期', last_refresh: '最近刷新', encoding: '压缩编码', distkey: 'DIST 键',
};
export const label = (code: unknown) => typeof code === 'string' && labels[code] ? t(labels[code]) : String(code ?? '—');

export const isTemporary = (name: string) => name.includes('_cd_tmp_');
