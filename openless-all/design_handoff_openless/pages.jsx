// pages.jsx — content blocks for each tab. The 3 variants reuse these so the
// difference between A/B/C is purely about navigation + framing, not content.

const { useState, useMemo } = React;

// ─── shared atoms ──────────────────────────────────────────────────────
const PageHeader = ({ kicker, title, desc, right }) => (
  <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 24, marginBottom: 24 }}>
    <div style={{ minWidth: 0 }}>
      {kicker && (
        <div style={{ fontSize: 11, fontWeight: 600, letterSpacing: '.08em', textTransform: 'uppercase', color: 'var(--ol-ink-4)', marginBottom: 8 }}>{kicker}</div>
      )}
      <h1 style={{ margin: 0, fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: 'var(--ol-ink)' }}>{title}</h1>
      {desc && <p style={{ margin: '8px 0 0', fontSize: 13, color: 'var(--ol-ink-3)', maxWidth: 640, lineHeight: 1.55 }}>{desc}</p>}
    </div>
    {right}
  </div>
);

const Card = ({ children, style, padding = 18, glassy = false }) => (
  <div
    style={{
      background: glassy ? 'rgba(255,255,255,0.55)' : 'var(--ol-surface)',
      backdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
      WebkitBackdropFilter: glassy ? 'blur(20px) saturate(160%)' : undefined,
      border: '0.5px solid var(--ol-line)',
      borderRadius: 'var(--ol-r-lg)',
      padding,
      boxShadow: 'var(--ol-shadow-sm)',
      ...style,
    }}
  >
    {children}
  </div>
);

const Pill = ({ children, tone = 'default', size = 'md', style }) => {
  const tones = {
    default: { bg: 'rgba(0,0,0,0.05)',   color: 'var(--ol-ink-2)',  bd: 'transparent' },
    blue:    { bg: 'var(--ol-blue-soft)',color: 'var(--ol-blue)',   bd: 'transparent' },
    ok:      { bg: 'var(--ol-ok-soft)',  color: 'var(--ol-ok)',     bd: 'transparent' },
    outline: { bg: 'transparent',        color: 'var(--ol-ink-3)',  bd: 'var(--ol-line-strong)' },
    dark:    { bg: 'var(--ol-ink)',      color: '#fff',             bd: 'transparent' },
  };
  const t = tones[tone];
  const sz = size === 'sm'
    ? { padding: '2px 8px', fontSize: 10.5 }
    : { padding: '4px 10px', fontSize: 11.5 };
  return (
    <span
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        borderRadius: 999,
        background: t.bg,
        color: t.color,
        border: t.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${t.bd}`,
        fontWeight: 500,
        ...sz,
        ...style,
      }}
    >
      {children}
    </span>
  );
};

const Btn = ({ children, variant = 'ghost', size = 'md', icon, style, onClick }) => {
  const variants = {
    primary: { bg: 'var(--ol-ink)',     color: '#fff',                bd: 'transparent', sh: '0 1px 2px rgba(0,0,0,.08)' },
    blue:    { bg: 'var(--ol-blue)',    color: '#fff',                bd: 'transparent', sh: '0 1px 2px rgba(37,99,235,.18)' },
    ghost:   { bg: 'transparent',       color: 'var(--ol-ink-2)',     bd: 'var(--ol-line-strong)', sh: 'none' },
    soft:    { bg: 'rgba(0,0,0,0.04)',  color: 'var(--ol-ink-2)',     bd: 'transparent', sh: 'none' },
  };
  const v = variants[variant];
  const sizes = { sm: { padding: '5px 10px', fontSize: 12 }, md: { padding: '7px 14px', fontSize: 12.5 } };
  return (
    <button
      onClick={onClick}
      style={{
        display: 'inline-flex', alignItems: 'center', gap: 6,
        background: v.bg, color: v.color,
        border: v.bd === 'transparent' ? '0.5px solid transparent' : `0.5px solid ${v.bd}`,
        borderRadius: 8,
        boxShadow: v.sh,
        fontFamily: 'inherit', fontWeight: 500,
        cursor: 'default',
        ...sizes[size],
        ...style,
      }}
    >
      {icon && <Icon name={icon} size={13} />}
      {children}
    </button>
  );
};

// ─── Overview ──────────────────────────────────────────────────────────
const Overview = () => {
  const m = OL_DATA.metrics;
  return (
    <>
      <PageHeader
        kicker="DASHBOARD"
        title="今日概览"
        desc="本地说出，本地落字。下面是你今日的口述节奏与系统状态。"
        right={
          <div
            style={{
              display: 'inline-flex', alignItems: 'center', gap: 8,
              padding: '6px 12px',
              borderRadius: 999,
              border: '0.5px solid var(--ol-line-strong)',
              background: 'var(--ol-surface-2)',
              color: 'var(--ol-ink-3)',
              fontSize: 12,
            }}
          >
            <Icon name="cmd" size={12} />
            按
            <kbd style={{
              padding: '2px 7px', fontSize: 11, fontFamily: 'var(--ol-font-mono)',
              background: '#fff', borderRadius: 5,
              border: '0.5px solid var(--ol-line-strong)',
              color: 'var(--ol-ink)',
            }}>右 Option</kbd>
            开始录音
          </div>
        }
      />

      {/* Provider status — first thing the user sees */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 18 }}>
        <ProviderCard kind="ASR 语音" {...OL_DATA.providers.asr} />
        <ProviderCard kind="LLM 模型" {...OL_DATA.providers.llm} />
      </div>

      {/* Metric grid — 4 up */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 18 }}>
        <Metric icon="mic"   label="口述时长"    value={m.duration}  trend="+12% 较昨日" />
        <Metric icon="hash"  label="总字数"      value={m.words}     trend="+1,820" />
        <Metric icon="clock" label="平均每分钟"  value={m.perMin}    trend="稳定" />
        <Metric icon="bolt"  label="估算节省"    value={m.saved}     trend={`${m.speedup} 速度提升`} accent />
      </div>

      {/* Activity + recent */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1.4fr', gap: 12 }}>
        <Card padding={18}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>本周活跃</span>
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>条数 / 天</span>
          </div>
          <WeekChart />
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--ol-ink-4)', marginTop: 8 }}>
            {['一','二','三','四','五','六','日'].map(d => <span key={d}>{d}</span>)}
          </div>
        </Card>

        <Card padding={0}>
          <div style={{ padding: '14px 18px', borderBottom: '0.5px solid var(--ol-line)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>最近识别</span>
            <Btn size="sm" variant="ghost">全部记录 →</Btn>
          </div>
          <div>
            {OL_DATA.history.slice(0, 4).map((h, i) => (
              <RecentRow key={i} {...h} />
            ))}
          </div>
        </Card>
      </div>
    </>
  );
};

const ProviderCard = ({ kind, name, subname, status }) => (
  <Card padding={16} style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
    <div
      style={{
        width: 38, height: 38, borderRadius: 10,
        background: 'var(--ol-blue-soft)',
        color: 'var(--ol-blue)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
      }}
    >
      <Icon name={kind.startsWith('ASR') ? 'mic' : 'sparkle'} size={18} />
    </div>
    <div style={{ flex: 1, minWidth: 0 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
        <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontWeight: 600, letterSpacing: '.06em', textTransform: 'uppercase' }}>{kind}</span>
        <Pill tone="ok" size="sm">
          <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />
          已配置
        </Pill>
      </div>
      <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{name}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginTop: 1, fontFamily: 'var(--ol-font-mono)' }}>{subname}</div>
    </div>
    <Btn size="sm" variant="ghost">切换</Btn>
  </Card>
);

const Metric = ({ icon, label, value, trend, accent }) => (
  <Card padding={16}>
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, color: 'var(--ol-ink-3)' }}>
      <Icon name={icon} size={13} />
      <span style={{ fontSize: 11.5 }}>{label}</span>
    </div>
    <div style={{ fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: accent ? 'var(--ol-blue)' : 'var(--ol-ink)', lineHeight: 1.1 }}>{value}</div>
    <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 6 }}>{trend}</div>
  </Card>
);

const WeekChart = () => {
  const max = Math.max(...OL_DATA.weekly);
  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 8, height: 100 }}>
      {OL_DATA.weekly.map((v, i) => {
        const isToday = i === 5;
        return (
          <div key={i} style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4 }}>
            <div style={{ fontSize: 9.5, color: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)', fontWeight: isToday ? 600 : 400 }}>{v}</div>
            <div
              style={{
                width: '100%',
                height: `${(v / max) * 80}px`,
                borderRadius: 4,
                background: isToday ? 'var(--ol-blue)' : 'var(--ol-ink)',
                opacity: isToday ? 1 : 0.85,
              }}
            />
          </div>
        );
      })}
    </div>
  );
};

const RecentRow = ({ time, style, dur, preview }) => (
  <div style={{ padding: '12px 18px', borderBottom: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 12, alignItems: 'flex-start' }}>
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 4, minWidth: 60 }}>
      <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{time}</span>
      <Pill size="sm" tone="default">{style}</Pill>
    </div>
    <div style={{ flex: 1, fontSize: 12.5, color: 'var(--ol-ink-2)', whiteSpace: 'pre-line', lineHeight: 1.55 }}>{preview.split('\n')[0]}</div>
    <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>{dur}</span>
  </div>
);

// ─── History ───────────────────────────────────────────────────────────
// History — built-in two-column workspace (list + detail)
const History = () => {
  const [filter, setFilter] = useState('全部');
  const [selected, setSelected] = useState(0);
  const list = OL_DATA.history.filter(h => filter === '全部' || h.style === filter);
  const item = list[selected] || list[0];
  return (
    <>
      <PageHeader
        kicker="HISTORY"
        title="历史记录"
        desc="最近的识别结果只保存在本机。左侧为时间线，右侧为原文与润色对比。"
        right={
          <div style={{ display: 'flex', gap: 8 }}>
            <Btn icon="refresh" variant="ghost" size="sm">刷新</Btn>
            <Btn icon="trash" variant="ghost" size="sm">清空</Btn>
          </div>
        }
      />
      <div style={{ display: 'grid', gridTemplateColumns: '300px 1fr', gap: 14, height: 'calc(100% - 110px)', minHeight: 480 }}>
        {/* List pane */}
        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ padding: '12px 14px', borderBottom: '0.5px solid var(--ol-line)' }}>
            <div style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '6px 10px', fontSize: 12,
              border: '0.5px solid var(--ol-line-strong)', borderRadius: 8,
              background: 'var(--ol-surface-2)', color: 'var(--ol-ink-3)',
            }}>
              <Icon name="search" size={12} />
              <span style={{ flex: 1 }}>搜索 {OL_DATA.history.length} 条</span>
              <kbd style={{ fontSize: 10, fontFamily: 'var(--ol-font-mono)' }}>⌘K</kbd>
            </div>
            <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap', marginTop: 10 }}>
              {['全部', '原文', '轻度润色', '清晰结构', '正式表达'].map(f => (
                <button
                  key={f}
                  onClick={() => { setFilter(f); setSelected(0); }}
                  style={{
                    padding: '3px 9px', fontSize: 11, borderRadius: 999,
                    border: '0.5px solid ' + (filter === f ? 'var(--ol-ink)' : 'var(--ol-line-strong)'),
                    background: filter === f ? 'var(--ol-ink)' : 'transparent',
                    color: filter === f ? '#fff' : 'var(--ol-ink-3)',
                    cursor: 'default', fontFamily: 'inherit', fontWeight: 500,
                  }}
                >{f}</button>
              ))}
            </div>
          </div>
          <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
            {list.map((h, i) => (
              <button
                key={i}
                onClick={() => setSelected(i)}
                style={{
                  width: '100%', padding: '10px 12px', textAlign: 'left',
                  display: 'flex', flexDirection: 'column', gap: 4,
                  border: 0, borderRadius: 8,
                  background: selected === i ? 'rgba(37,99,235,0.06)' : 'transparent',
                  boxShadow: selected === i ? 'inset 2px 0 0 var(--ol-blue)' : 'none',
                  cursor: 'default', fontFamily: 'inherit', marginBottom: 1,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', gap: 8 }}>
                  <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{h.time}</span>
                  <span style={{ fontSize: 10, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>{h.dur}</span>
                </div>
                <div style={{ fontSize: 12, color: 'var(--ol-ink-2)', lineHeight: 1.45, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>
                  {h.preview.split('\n')[0]}
                </div>
                <div><Pill size="sm" tone={h.style === '原文' ? 'outline' : 'default'}>{h.style}</Pill></div>
              </button>
            ))}
          </div>
        </Card>

        {/* Detail pane */}
        <Card padding={20} style={{ overflow: 'auto' }}>
          {item && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <span style={{ fontSize: 13, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{item.time}</span>
                  <Pill size="sm" tone="default">{item.style}</Pill>
                  <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{item.dur}</span>
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <Btn icon="copy" variant="ghost" size="sm">复制</Btn>
                  <Btn icon="sparkle" variant="primary" size="sm">重新润色</Btn>
                </div>
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-line)', borderRadius: 10, background: 'var(--ol-surface-2)' }}>
                  <Pill size="sm" tone="outline" style={{ marginBottom: 10 }}>原文</Pill>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink-2)' }}>
                    嗯那个我刚刚看了下新出的电影预告片，挺有意思的你有空也看看，呃就是那个画面感特别好。
                  </p>
                </div>
                <div style={{ padding: 14, border: '0.5px solid var(--ol-blue)', borderRadius: 10, background: 'var(--ol-blue-soft)' }}>
                  <Pill size="sm" tone="blue" style={{ marginBottom: 10 }}>{item.style}</Pill>
                  <p style={{ margin: 0, fontSize: 13, lineHeight: 1.7, color: 'var(--ol-ink)', whiteSpace: 'pre-line' }}>{item.preview}</p>
                </div>
              </div>
              {item.tag && (
                <div style={{ marginTop: 14, padding: '10px 12px', borderRadius: 8, background: 'rgba(37,99,235,0.06)', fontSize: 11.5, color: 'var(--ol-blue)', display: 'flex', alignItems: 'center', gap: 6 }}>
                  <Icon name="sparkle" size={11} />
                  {item.tag}
                </div>
              )}
              <div style={{ marginTop: 18, paddingTop: 14, borderTop: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 18, fontSize: 11, color: 'var(--ol-ink-4)' }}>
                <span>插入到 <b style={{ color: 'var(--ol-ink-2)' }}>VS Code</b></span>
                <span>56 字 · 0.6s</span>
                <span>火山引擎 + DeepSeek</span>
              </div>
            </>
          )}
        </Card>
      </div>
    </>
  );
};

const HistoryRow = ({ time, style, dur, preview, tag, last }) => (
  <div style={{ padding: '14px 18px', borderBottom: last ? 'none' : '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 16 }}>
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6, minWidth: 72 }}>
      <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>{time}</span>
      <Pill size="sm" tone={style === '原文' ? 'outline' : 'default'}>{style}</Pill>
    </div>
    <div style={{ flex: 1, minWidth: 0 }}>
      <div style={{ fontSize: 12.5, color: 'var(--ol-ink-2)', whiteSpace: 'pre-line', lineHeight: 1.6 }}>{preview}</div>
      {tag && (
        <div style={{ marginTop: 8, fontSize: 11, color: 'var(--ol-blue)', display: 'inline-flex', alignItems: 'center', gap: 5 }}>
          <Icon name="sparkle" size={11} />
          {tag}
        </div>
      )}
    </div>
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 6 }}>
      <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>{dur}</span>
      <Pill size="sm" tone="blue">
        <Icon name="check" size={10} />
        已插入
      </Pill>
    </div>
  </div>
);

// ─── Vocab ─────────────────────────────────────────────────────────────
const Vocab = () => (
  <>
    <PageHeader
      kicker="VOCABULARY"
      title="词汇表"
      desc="告诉模型识别前可能出现的词——生词、新词或专业词汇。同时进入 ASR 热词与后期模型上下文。"
      right={
        <div style={{ display: 'flex', gap: 8 }}>
          <Btn icon="refresh" variant="ghost" size="sm">重置统计</Btn>
          <Btn icon="trash" variant="ghost" size="sm">清除全部</Btn>
        </div>
      }
    />
    <Card padding={0}>
      <div style={{ padding: 18, borderBottom: '0.5px solid var(--ol-line)' }}>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            placeholder="输入词语，每行一个…"
            style={{
              flex: 1, height: 36, padding: '0 12px',
              border: '0.5px solid var(--ol-line-strong)',
              borderRadius: 8, fontSize: 13,
              fontFamily: 'inherit', outline: 'none',
              background: 'var(--ol-surface-2)',
            }}
          />
          <Btn variant="primary" icon="plus">添加</Btn>
        </div>
        <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 10 }}>
          支持中英混合 · 数字开头按字面识别 · 命中次数自动计数
        </div>
      </div>
      <div style={{ padding: 18, display: 'flex', flexWrap: 'wrap', gap: 8 }}>
        {OL_DATA.vocab.map((v, i) => (
          <VocabChip key={i} {...v} />
        ))}
      </div>
    </Card>
  </>
);

const VocabChip = ({ word, count }) => (
  <span
    style={{
      display: 'inline-flex', alignItems: 'center', gap: 6,
      padding: '5px 10px 5px 12px',
      borderRadius: 999,
      border: '0.5px solid var(--ol-line-strong)',
      background: count > 0 ? 'var(--ol-blue-soft)' : 'var(--ol-surface)',
      fontSize: 12, color: 'var(--ol-ink)',
      fontFamily: 'var(--ol-font-mono)',
    }}
  >
    {word}
    <span
      style={{
        minWidth: 18, height: 18, padding: '0 5px',
        borderRadius: 999, fontSize: 10, fontWeight: 600,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        background: count > 0 ? 'var(--ol-blue)' : 'rgba(0,0,0,0.06)',
        color: count > 0 ? '#fff' : 'var(--ol-ink-4)',
        fontFamily: 'var(--ol-font-sans)',
      }}
    >{count}</span>
    <button
      style={{
        width: 14, height: 14, padding: 0, border: 0, borderRadius: 999,
        background: 'transparent', color: 'var(--ol-ink-4)',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center', cursor: 'default',
      }}
    >
      <svg width="8" height="8" viewBox="0 0 8 8"><path d="M1 1l6 6M7 1l-6 6" stroke="currentColor" strokeWidth="1.4" /></svg>
    </button>
  </span>
);

// ─── Style ─────────────────────────────────────────────────────────────
const Style = () => {
  const [active, setActive] = useState('clear');
  const [enabled, setEnabled] = useState(true);
  return (
    <>
      <PageHeader
        kicker="STYLE"
        title="输出风格"
        desc="为不同场景配置输出风格。每个风格包含 AI 润色规则与文本优化设置。点击卡片切换当前生效风格。"
        right={
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <span style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>启用</span>
            <button
              onClick={() => setEnabled(!enabled)}
              style={{
                position: 'relative', width: 36, height: 20, borderRadius: 999, border: 0,
                background: enabled ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
                cursor: 'default', transition: 'background .15s',
              }}
            >
              <span
                style={{
                  position: 'absolute', top: 2, left: enabled ? 18 : 2,
                  width: 16, height: 16, borderRadius: 999, background: '#fff',
                  boxShadow: '0 1px 2px rgba(0,0,0,.2)', transition: 'left .15s',
                }}
              />
            </button>
          </div>
        }
      />
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        {OL_DATA.styles.map(s => {
          const isActive = active === s.id;
          return (
            <button
              key={s.id}
              onClick={() => setActive(s.id)}
              style={{
                padding: 18, textAlign: 'left',
                background: 'var(--ol-surface)',
                border: '0.5px solid ' + (isActive ? 'var(--ol-blue)' : 'var(--ol-line)'),
                borderRadius: 'var(--ol-r-lg)',
                boxShadow: isActive ? '0 0 0 3px var(--ol-blue-ring), var(--ol-shadow-sm)' : 'var(--ol-shadow-sm)',
                cursor: 'default',
                fontFamily: 'inherit',
                position: 'relative',
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 4 }}>
                {isActive
                  ? <span style={{ width: 16, height: 16, borderRadius: 999, background: 'var(--ol-blue)', display: 'inline-flex', alignItems: 'center', justifyContent: 'center', color: '#fff' }}>
                      <svg width="9" height="9" viewBox="0 0 9 9"><path d="M1.5 4.5l2.5 2.5 4-5" stroke="currentColor" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round"/></svg>
                    </span>
                  : <span style={{ width: 16, height: 16, borderRadius: 999, border: '1.5px solid var(--ol-line-strong)' }} />}
                <span style={{ fontSize: 14, fontWeight: 600 }}>{s.name}</span>
                {isActive && <Pill tone="blue" size="sm" style={{ marginLeft: 'auto' }}>当前生效</Pill>}
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginBottom: 12 }}>{s.desc}</div>
              <div
                style={{
                  fontSize: 12.5, color: 'var(--ol-ink-2)', lineHeight: 1.6,
                  padding: 12, borderRadius: 8,
                  background: 'var(--ol-surface-2)',
                  border: '0.5px dashed var(--ol-line)',
                }}
              >
                {s.sample}
              </div>
            </button>
          );
        })}
      </div>
    </>
  );
};

// ─── Settings (merged: 配置 + 设置 + 帮助) ──────────────────────────────
const Settings = ({ embedded = false }) => {
  const [section, setSection] = useState('录音');
  const sections = ['录音', '提供商', '快捷键', '权限', '关于'];
  return (
    <>
      {!embedded && (
        <PageHeader
          kicker="SETTINGS"
          title="设置"
          desc="录音方式、模型与语音提供商、快捷键、权限与关于信息——全部在这里。"
        />
      )}
      <div style={{ display: 'grid', gridTemplateColumns: embedded ? '120px 1fr' : '160px 1fr', gap: 18 }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {sections.map(s => (
            <button
              key={s}
              onClick={() => setSection(s)}
              style={{
                padding: '8px 12px', textAlign: 'left',
                fontSize: 13, color: section === s ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                background: section === s ? 'rgba(0,0,0,0.04)' : 'transparent',
                border: 0, borderRadius: 8, fontFamily: 'inherit', fontWeight: section === s ? 600 : 500,
                cursor: 'default',
              }}
            >
              {s}
            </button>
          ))}
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {section === '录音' && <RecordingSection />}
          {section === '提供商' && <ProvidersSection />}
          {section === '快捷键' && <ShortcutsSection />}
          {section === '权限' && <PermissionsSection />}
          {section === '关于' && <AboutSection />}
        </div>
      </div>
    </>
  );
};

const SettingRow = ({ label, desc, children }) => (
  <div style={{ display: 'grid', gridTemplateColumns: '180px 1fr', gap: 16, padding: '14px 0', borderTop: '0.5px solid var(--ol-line-soft)' }}>
    <div>
      <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--ol-ink)' }}>{label}</div>
      {desc && <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 4, lineHeight: 1.5 }}>{desc}</div>}
    </div>
    <div style={{ display: 'flex', alignItems: 'flex-start' }}>{children}</div>
  </div>
);

const RecordingSection = () => {
  const [mode, setMode] = useState('toggle');
  return (
    <Card>
      <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>录音</div>
      <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>定义全局录音的快捷键与触发方式。</div>
      <SettingRow label="录音快捷键" desc="按下即开始捕获语音，全局生效。">
        <div style={{ display: 'inline-flex', alignItems: 'center', gap: 6, padding: '5px 10px', borderRadius: 8, border: '0.5px solid var(--ol-line-strong)', background: 'var(--ol-surface-2)', fontSize: 12, fontFamily: 'var(--ol-font-mono)' }}>
          <Icon name="option" size={12} />
          右 Option
        </div>
      </SettingRow>
      <SettingRow label="录音方式">
        <div style={{ display: 'inline-flex', padding: 2, borderRadius: 8, background: 'rgba(0,0,0,0.05)' }}>
          {[['toggle', '切换式'], ['hold', '按住说话']].map(([v, l]) => (
            <button
              key={v}
              onClick={() => setMode(v)}
              style={{
                padding: '5px 14px', fontSize: 12, fontWeight: 500,
                border: 0, borderRadius: 6, fontFamily: 'inherit',
                background: mode === v ? '#fff' : 'transparent',
                color: mode === v ? 'var(--ol-ink)' : 'var(--ol-ink-3)',
                boxShadow: mode === v ? '0 1px 2px rgba(0,0,0,.08)' : 'none',
                cursor: 'default',
              }}
            >
              {l}
            </button>
          ))}
        </div>
      </SettingRow>
      <SettingRow label="自动插入光标位置" desc="识别完成后将文本自动写入当前活动应用的光标位置。">
        <Toggle on />
      </SettingRow>
    </Card>
  );
};

const Toggle = ({ on: initial = false }) => {
  const [on, setOn] = useState(initial);
  return (
    <button
      onClick={() => setOn(!on)}
      style={{
        position: 'relative', width: 32, height: 18, borderRadius: 999, border: 0,
        background: on ? 'var(--ol-blue)' : 'rgba(0,0,0,0.15)',
        cursor: 'default',
      }}
    >
      <span
        style={{
          position: 'absolute', top: 2, left: on ? 16 : 2,
          width: 14, height: 14, borderRadius: 999, background: '#fff',
          boxShadow: '0 1px 2px rgba(0,0,0,.25)', transition: 'left .15s',
        }}
      />
    </button>
  );
};

const ProvidersSection = () => {
  const [llm, setLlm] = useState('deepseek');
  const llms = [
    { id: 'doubao',   name: '豆包',     sub: 'Ark' },
    { id: 'openai',   name: 'OpenAI',   sub: 'GPT' },
    { id: 'dashscope',name: '阿里通义', sub: 'DashScope' },
    { id: 'deepseek', name: 'DeepSeek', sub: 'v4-flash', current: true },
    { id: 'moonshot', name: 'Moonshot', sub: 'Kimi' },
  ];
  return (
    <>
      <Card>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <div>
            <div style={{ fontSize: 13, fontWeight: 600 }}>LLM 模型</div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>用于风格化润色与结构化整理。</div>
          </div>
          <Pill tone="ok" size="sm"><span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />已配置</Pill>
        </div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', margin: '6px 0 14px' }}>
          {llms.map(l => (
            <button
              key={l.id}
              onClick={() => setLlm(l.id)}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                padding: '6px 12px', fontSize: 12,
                borderRadius: 8, fontFamily: 'inherit', fontWeight: 500,
                background: llm === l.id ? 'var(--ol-ink)' : 'var(--ol-surface)',
                color: llm === l.id ? '#fff' : 'var(--ol-ink-2)',
                border: '0.5px solid ' + (llm === l.id ? 'var(--ol-ink)' : 'var(--ol-line-strong)'),
                cursor: 'default',
              }}
            >
              {l.name}
              {l.current && llm !== l.id && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 999, background: 'var(--ol-blue-soft)', color: 'var(--ol-blue)' }}>当前</span>}
            </button>
          ))}
        </div>
        <SettingRow label="API Key">
          <KeyField value="••••••••••••••••••••••••••••" />
        </SettingRow>
        <SettingRow label="Base URL">
          <input defaultValue="https://api.deepseek.com" style={inputStyle} />
        </SettingRow>
        <SettingRow label="Model">
          <input defaultValue="deepseek-v4-flash" style={{ ...inputStyle, fontFamily: 'var(--ol-font-mono)' }} />
        </SettingRow>
        <SettingRow label="Temperature">
          <div style={{ display: 'flex', alignItems: 'center', gap: 12, width: '100%', maxWidth: 360 }}>
            <input type="range" min="0" max="1" step="0.05" defaultValue="0.4" style={{ flex: 1, accentColor: 'var(--ol-blue)' }} />
            <span style={{ fontSize: 12, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)', minWidth: 32 }}>0.40</span>
          </div>
        </SettingRow>
      </Card>

      <Card>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 10 }}>
          <div>
            <div style={{ fontSize: 13, fontWeight: 600 }}>ASR 语音</div>
            <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginTop: 2 }}>用于将口述实时转写为文本。</div>
          </div>
          <Pill tone="ok" size="sm"><span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />已配置</Pill>
        </div>
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginBottom: 14 }}>
          {[
            { id: 'volc',     name: '火山引擎',         current: true, active: true },
            { id: 'apple',    name: 'macOS 本地',       sub: 'Apple Speech' },
            { id: 'paraform', name: '阿里 Paraformer', sub: 'DashScope' },
          ].map(p => (
            <button
              key={p.id}
              style={{
                display: 'inline-flex', alignItems: 'center', gap: 6,
                padding: '6px 12px', fontSize: 12,
                borderRadius: 8, fontFamily: 'inherit', fontWeight: 500,
                background: p.active ? 'var(--ol-ink)' : 'var(--ol-surface)',
                color: p.active ? '#fff' : 'var(--ol-ink-2)',
                border: '0.5px solid ' + (p.active ? 'var(--ol-ink)' : 'var(--ol-line-strong)'),
                cursor: 'default',
              }}
            >
              {p.name}
              {p.sub && <span style={{ fontSize: 10, opacity: 0.6 }}>· {p.sub}</span>}
              {p.current && !p.active && <span style={{ fontSize: 9, padding: '1px 5px', borderRadius: 999, background: 'var(--ol-blue-soft)', color: 'var(--ol-blue)' }}>当前</span>}
            </button>
          ))}
        </div>
        <SettingRow label="APP ID"><input defaultValue="1140349910" style={inputStyle} /></SettingRow>
        <SettingRow label="Access Token"><KeyField value="••••••••••••••••••••" /></SettingRow>
        <SettingRow label="Resource ID"><input defaultValue="volc.bigasr.sauc.duration" style={{ ...inputStyle, fontFamily: 'var(--ol-font-mono)' }} /></SettingRow>
      </Card>
    </>
  );
};

const KeyField = ({ value }) => (
  <div style={{ display: 'flex', gap: 6, alignItems: 'center', width: '100%', maxWidth: 360 }}>
    <input value={value} readOnly style={{ ...inputStyle, fontFamily: 'var(--ol-font-mono)' }} />
    <button style={iconBtnStyle}><Icon name="eye" size={14} /></button>
    <button style={iconBtnStyle}><Icon name="copy" size={14} /></button>
  </div>
);

const inputStyle = {
  flex: 1, height: 32, padding: '0 10px',
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, fontSize: 12.5,
  fontFamily: 'inherit', outline: 'none',
  background: 'var(--ol-surface-2)',
  width: '100%', maxWidth: 360,
};
const iconBtnStyle = {
  width: 32, height: 32,
  border: '0.5px solid var(--ol-line-strong)',
  borderRadius: 8, background: 'var(--ol-surface)',
  display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
  color: 'var(--ol-ink-3)', cursor: 'default', flexShrink: 0,
};

const ShortcutsSection = () => (
  <Card>
    <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>快捷键速查</div>
    <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>所有快捷键全局生效，需要在权限设置中开启辅助功能。</div>
    {[
      ['开始 / 停止录音', '右 Option'],
      ['取消本次录音', 'Esc'],
      ['胶囊确认插入', '点击右侧 ✓'],
      ['切换上一次风格', '⌘ ⇧ S'],
      ['打开 OpenLess', '⌘ ⇧ O'],
    ].map(([k, v]) => (
      <SettingRow key={k} label={k}>
        <kbd style={{
          display: 'inline-flex', alignItems: 'center', gap: 4,
          padding: '4px 10px', fontSize: 12, fontFamily: 'var(--ol-font-mono)',
          borderRadius: 6, background: 'var(--ol-surface-2)',
          border: '0.5px solid var(--ol-line-strong)',
          boxShadow: '0 1px 0 rgba(0,0,0,0.04)',
          color: 'var(--ol-ink-2)',
        }}>{v}</kbd>
      </SettingRow>
    ))}
  </Card>
);

const PermissionsSection = () => (
  <Card>
    <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>权限</div>
    <div style={{ fontSize: 11.5, color: 'var(--ol-ink-4)', marginBottom: 6 }}>OpenLess 需要以下系统权限才能正常工作。</div>
    <SettingRow label="麦克风" desc="用于捕获你的语音输入。">
      <Pill tone="ok"><Icon name="check" size={11} />已授权</Pill>
    </SettingRow>
    <SettingRow label="辅助功能" desc="用于监听全局快捷键并将识别结果写入光标位置。">
      <Pill tone="ok"><Icon name="check" size={11} />已授权</Pill>
    </SettingRow>
    <SettingRow label="网络" desc="云端 ASR / LLM 调用所必需。本地模式可关闭。">
      <Pill tone="ok"><Icon name="check" size={11} />可用</Pill>
    </SettingRow>
  </Card>
);

const AboutSection = () => (
  <Card>
    <div style={{ display: 'flex', alignItems: 'center', gap: 14, marginBottom: 18 }}>
      <div
        style={{
          width: 52, height: 52, borderRadius: 12,
          background: 'linear-gradient(135deg, #0a0a0b 0%, #2563eb 100%)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          color: '#fff', fontSize: 22, fontWeight: 700, letterSpacing: '-0.02em',
        }}
      >OL</div>
      <div>
        <div style={{ fontSize: 16, fontWeight: 600 }}>OpenLess</div>
        <div style={{ fontSize: 12, color: 'var(--ol-ink-3)' }}>自然说话，完美书写 · v0.6.2 (Build 384)</div>
      </div>
    </div>
    <SettingRow label="检查更新"><Btn variant="ghost" size="sm">检查</Btn></SettingRow>
    <SettingRow label="文档"><Btn variant="ghost" size="sm" icon="link">openless.app/docs</Btn></SettingRow>
    <SettingRow label="反馈渠道"><Btn variant="ghost" size="sm" icon="link">GitHub Issues</Btn></SettingRow>
    <SettingRow label="隐私" desc="所有识别结果仅保存在本机。云端 API 仅用于实时转写与润色，不会保留你的录音。">
      <Pill tone="default">本地优先</Pill>
    </SettingRow>
  </Card>
);

window.OLPages = { Overview, History, Vocab, Style, Settings };
window.OLAtoms = { PageHeader, Card, Pill, Btn };
