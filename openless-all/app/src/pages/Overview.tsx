// Overview.tsx — 真实指标，从 listHistory + getCredentials 派生。

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from '../components/Icon';
import { Modal } from '../components/ui/Modal';
import { formatComboLabel } from '../lib/hotkey';
import { getCredentials, listHistory } from '../lib/ipc';
import { useMobileLayout } from '../lib/useMobileLayout';
import type { CredentialsStatus, DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

function useModeLabels(): Record<PolishMode, string> {
  const { t } = useTranslation();
  return {
    raw: t('style.modes.raw.name'),
    light: t('style.modes.light.name'),
    structured: t('style.modes.structured.name'),
    formal: t('style.modes.formal.name'),
  };
}

interface OverviewProps {
  onOpenHistory?: () => void;
}

const ASR_NAME_KEY_BY_ID: Record<string, string> = {
  volcengine: 'asrVolcengine',
  bailian: 'asrBailian',
  siliconflow: 'asrSiliconflow',
  zhipu: 'asrZhipu',
  groq: 'asrGroq',
  whisper: 'asrWhisper',
  openrouter: 'asrOpenrouter',
  'xiaomi-mimo-asr': 'asrXiaomiMimo',
  'foundry-local-whisper': 'asrFoundryLocalWhisper',
  'sherpa-onnx-local': 'asrSherpaOnnxLocal',
  'local-qwen3': 'asrLocalQwen3',
};

const LLM_NAME_KEY_BY_ID: Record<string, string> = {
  ark: 'ark',
  deepseek: 'deepseek',
  siliconflow: 'siliconflow',
  openai: 'openai',
  codex_oauth: 'codexOAuth',
  mimo: 'mimo',
  cometapi: 'cometapi',
  openrouterFree: 'openrouterFree',
  alibabaCoding: 'alibabaCoding',
  codingPlanX: 'codingPlanX',
  custom: 'custom',
};

// ponytail: single estimate for a feel-good stat; make it a setting only if users ask to calibrate it.
const TYPING_CHARS_PER_MINUTE = 100;

export function Overview({ onOpenHistory }: OverviewProps) {
  const { t } = useTranslation();
  const mobile = useMobileLayout();
  const modeLabel = useModeLabels();
  const [history, setHistory] = useState<DictationSession[]>([]);
  const [historyError, setHistoryError] = useState(false);
  const [credsError, setCredsError] = useState(false);
  const [creds, setCreds] = useState<CredentialsStatus>({
    activeAsrProvider: 'volcengine',
    activeLlmProvider: 'ark',
    asrConfigured: false,
    llmConfigured: false,
    volcengineConfigured: false,
    arkConfigured: false,
  });
  const [shareOpen, setShareOpen] = useState(false);
  const { prefs } = useHotkeySettings();
  const credentialsRequestSeq = useRef(0);

  const refreshHistory = useCallback(() => {
    setHistoryError(false);
    listHistory()
      .then(setHistory)
      .catch(error => {
        console.error('[overview] failed to load history', error);
        setHistoryError(true);
      });
  }, []);

  const refreshCredentials = useCallback(() => {
    const requestSeq = credentialsRequestSeq.current + 1;
    credentialsRequestSeq.current = requestSeq;
    setCredsError(false);
    getCredentials()
      .then(status => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        setCreds(status);
        setCredsError(false);
      })
      .catch(error => {
        if (requestSeq !== credentialsRequestSeq.current) return;
        console.error('[overview] failed to load credentials status', error);
        setCredsError(true);
      });
  }, []);

  useEffect(() => {
    refreshHistory();
  }, [refreshHistory]);

  useEffect(() => {
    refreshCredentials();
  }, [refreshCredentials, prefs?.activeAsrProvider, prefs?.activeLlmProvider]);

  // 凭据被保存后重新拉取状态（issue #532 / #573：在 Settings 中填写/更新凭据
  // 但不切换提供商时，上面的 useEffect 不会重跑，导致概览页的状态仍停留在「未配置」）。
  // 复用 refreshCredentials() 以带上 credentialsRequestSeq 防竞态。
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      try {
        const { listen } = await import('@tauri-apps/api/event');
        const handle = await listen('credentials:changed', () => {
          if (cancelled) return;
          refreshCredentials();
        });
        if (cancelled) {
          handle();
        } else {
          unlisten = handle;
        }
      } catch {
        // browser dev mock — 没有 Tauri event bridge
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshCredentials]);

  const metrics = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todays = history.filter(s => new Date(s.createdAt) >= today);
    const charsToday = todays.reduce((acc, s) => acc + s.finalText.length, 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((acc, s) => acc + (s.durationMs ?? 0), 0);
    const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
    const typingMs = (charsToday / TYPING_CHARS_PER_MINUTE) * 60000;
    const savedMs = totalDurationMs > 0 ? Math.max(0, typingMs - totalDurationMs) : 0;
    const speedRatio = totalDurationMs > 0 ? typingMs / totalDurationMs : 0;
    return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs, savedMs, speedRatio };
  }, [history]);

  // 周历:过去 7 天每天的条数
  const weekly = useMemo(() => {
    const buckets = Array(7).fill(0);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    history.forEach(s => {
      const d = new Date(s.createdAt);
      const diff = Math.floor((today.getTime() - d.setHours(0, 0, 0, 0)) / 86400000);
      if (diff >= 0 && diff < 7) {
        buckets[6 - diff] += 1;
      }
    });
    return buckets;
  }, [history]);

  const asrProviderId = creds.activeAsrProvider || 'volcengine';
  const llmProviderId = creds.activeLlmProvider || 'ark';
  const asrNameKey = ASR_NAME_KEY_BY_ID[asrProviderId];
  const llmNameKey = LLM_NAME_KEY_BY_ID[llmProviderId];
  const asrProviderName = asrNameKey
    ? t(`settings.providers.presets.${asrNameKey}`)
    : asrProviderId;
  const llmProviderName = llmNameKey
    ? t(`settings.providers.presets.${llmNameKey}`)
    : llmProviderId;
  const shareDisabled = historyError || metrics.charsToday <= 0;

  return (
    <>
      <PageHeader
        title={t('overview.title')}
        right={
          <Btn
            icon="download"
            size="sm"
            variant="ghost"
            disabled={shareDisabled}
            onClick={() => setShareOpen(true)}
          >
            分享
          </Btn>
        }
      />

      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1fr', gap: 12, marginBottom: 18 }}>
        <ProviderCard
          kind={t('overview.asrKind')}
          name={asrProviderName}
          subname={asrProviderId}
          status={credsError ? 'error' : creds.asrConfigured ? 'configured' : 'notConfigured'}
        />
        <ProviderCard
          kind={t('overview.llmKind')}
          name={llmProviderName}
          subname={llmProviderId}
          status={credsError ? 'error' : creds.llmConfigured ? 'configured' : 'notConfigured'}
        />
      </div>

      <div className="ol-overview-hero" style={{ display: 'grid', gridTemplateColumns: mobile ? 'repeat(2, 1fr)' : 'repeat(4, 1fr)', gap: 12, marginBottom: 18 }}>
        <Metric icon="hash" label={t('overview.metricChars')} value={historyError ? '—' : metrics.charsToday.toLocaleString()} trend={historyError ? t('overview.historyLoadError') : t('overview.metricSegments', { count: metrics.segmentsToday })} />
        <Metric icon="mic" label={t('overview.metricDuration')} value={historyError ? '—' : formatDuration(metrics.totalDurationMs, t)} trend={historyError ? t('overview.historyLoadError') : efficiencyTinyText(metrics)} />
        <Metric icon="clock" label={t('overview.metricAvg')} value={historyError ? '—' : formatDuration(metrics.avgLatencyMs, t)} trend={historyError ? t('overview.historyLoadError') : metrics.segmentsToday > 0 ? t('overview.metricAvgTrend') : t('overview.metricNoData')} />
        <Metric icon="bolt" label={t('overview.metricTotal')} value={historyError ? '—' : String(history.length)} trend={historyError ? t('overview.historyLoadError') : t('overview.metricTotalTrend')} accent />
      </div>

      {/* 底部一行 = flex:1 撑满剩余高度（父 wrapper 是 display:flex/column）。
          只有「最近识别」内部允许滚动；其他卡片按内容自然高度，不破裂底部圆角。
          issue #243 follow-up：去掉外层 overflow 后底部圆角被裁的视觉问题。 */}
      <div style={{ display: 'grid', gridTemplateColumns: mobile ? '1fr' : '1fr 1.4fr', gap: 12, flex: mobile ? undefined : 1, minHeight: mobile ? undefined : 0 }}>
        <Card padding={18} style={{ display: 'flex', flexDirection: 'column', minHeight: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>{t('overview.weekTitle')}</span>
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>{t('overview.weekUnit')}</span>
          </div>
          {historyError ? (
            <div style={{ height: 100, display: 'flex', alignItems: 'center', justifyContent: 'center', textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
              {t('overview.historyLoadError')}
            </div>
          ) : (
            <WeekChart data={weekly} />
          )}
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--ol-ink-4)', marginTop: 8 }}>
            {weekDayLabels(t('overview.weekDays', { returnObjects: true }) as string[]).map((d, i) => <span key={i}>{d}</span>)}
          </div>
        </Card>

        <Card padding={0} style={{ display: 'flex', flexDirection: 'column', minHeight: 0, overflow: 'hidden' }}>
          <div style={{ padding: '14px 18px', borderBottom: '0.5px solid var(--ol-line)', display: 'flex', alignItems: 'center', justifyContent: 'space-between', flexShrink: 0 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>{t('overview.recentTitle')}</span>
            <Btn size="sm" variant="ghost" onClick={onOpenHistory}>{t('overview.recentAll')}</Btn>
          </div>
          <div className="ol-thinscroll" style={{ flex: 1, minHeight: 0, overflow: 'auto' }}>
            {historyError ? (
              <div style={{ padding: 24, textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 10 }}>
                <span>{t('overview.recentLoadFailed')}</span>
                <Btn size="sm" variant="ghost" onClick={refreshHistory}>{t('overview.historyRetry')}</Btn>
              </div>
            ) : (
              <>
                {history.length === 0 && (
                  <div style={{ padding: 24, textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
                    {t('overview.recentEmpty', { trigger: prefs ? formatComboLabel(prefs.dictationHotkey) : '' })}
                  </div>
                )}
                {history.slice(0, 5).map(s => (
                  <RecentRow key={s.id} session={s} modeLabel={modeLabel} />
                ))}
              </>
            )}
          </div>
        </Card>
      </div>

      {shareOpen && (
        <OverviewShareModal
          metrics={metrics}
          history={history}
          onClose={() => setShareOpen(false)}
        />
      )}
    </>
  );
}

interface ProviderCardProps {
  kind: string;
  name: string;
  subname: string;
  status: 'configured' | 'notConfigured' | 'error';
}

function ProviderCard({ kind, name, subname, status }: ProviderCardProps) {
  const { t } = useTranslation();
  // ASR 卡用 mic 图标，其他用 sparkle —— 通过比较译文判断会随语言改变，故改用本地化无关的字面量比较。
  const isAsr = kind === t('overview.asrKind');
  return (
    <Card padding={16} style={{ display: 'flex', alignItems: 'center', gap: 14 }}>
      <div
        style={{
          width: 38, height: 38, borderRadius: 10,
          background: 'var(--ol-blue-soft)',
          color: 'var(--ol-blue)',
          display: 'flex', alignItems: 'center', justifyContent: 'center',
        }}
      >
        <Icon name={isAsr ? 'mic' : 'sparkle'} size={18} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontWeight: 600, letterSpacing: '.06em', textTransform: 'uppercase' }}>{kind}</span>
          {status === 'configured' && (
            <Pill tone="ok" size="sm">
              <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />
              {t('overview.statusConfigured')}
            </Pill>
          )}
          {status === 'notConfigured' && (
            <Pill tone="outline" size="sm">{t('overview.statusNotConfigured')}</Pill>
          )}
          {status === 'error' && (
            <Pill tone="outline" size="sm" style={{ color: 'var(--ol-red, #ef4444)', borderColor: 'rgba(239,68,68,0.24)' }}>{t('overview.statusUnknown')}</Pill>
          )}
        </div>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{name}</div>
        <div style={{ fontSize: 11.5, color: status === 'error' ? 'var(--ol-red, #ef4444)' : 'var(--ol-ink-3)', marginTop: 1, fontFamily: status === 'error' ? undefined : 'var(--ol-font-mono)' }}>
          {status === 'error' ? t('overview.credentialsLoadError') : subname}
        </div>
      </div>
    </Card>
  );
}

interface MetricProps {
  icon: string;
  label: string;
  value: string;
  trend: string;
  accent?: boolean;
}

function Metric({ icon, label, value, trend, accent }: MetricProps) {
  return (
    <Card padding={16}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 8, color: 'var(--ol-ink-3)' }}>
        <Icon name={icon} size={13} />
        <span style={{ fontSize: 11.5 }}>{label}</span>
      </div>
      <div style={{ fontSize: 26, fontWeight: 600, letterSpacing: '-0.02em', color: accent ? 'var(--ol-blue)' : 'var(--ol-ink)', lineHeight: 1.1 }}>{value}</div>
      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', marginTop: 6 }}>{trend || ' '}</div>
    </Card>
  );
}

interface OverviewMetrics {
  charsToday: number;
  segmentsToday: number;
  totalDurationMs: number;
  avgLatencyMs: number;
  savedMs: number;
  speedRatio: number;
}

function calculateStreak(history: DictationSession[]): number {
  if (history.length === 0) return 0;
  const dates = Array.from(new Set(
    history.map(s => {
      const d = new Date(s.createdAt);
      return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
    })
  )).sort((a, b) => b.localeCompare(a));

  if (dates.length === 0) return 0;

  const todayStr = localDateKey(new Date());
  const yesterdayStr = localDateKey(addDays(new Date(), -1));

  let currentIdx = dates.indexOf(todayStr);
  if (currentIdx === -1) {
    currentIdx = dates.indexOf(yesterdayStr);
  }
  if (currentIdx === -1) {
    return 0;
  }

  let streak = 1;
  let checkDate = new Date(dates[currentIdx]);

  while (true) {
    checkDate = addDays(checkDate, -1);
    const prevDateStr = localDateKey(checkDate);
    if (dates.includes(prevDateStr)) {
      streak++;
    } else {
      break;
    }
  }

  return streak;
}

function generatePortrait(sessions: DictationSession[]): { role: string; desc: string } {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const todays = sessions.filter(s => new Date(s.createdAt) >= today);
  const combinedText = todays.map(s => s.finalText).join(' ');

  const matches = {
    dev: {
      keywords: ['代码', '接口', '编译', '报错', '调试', '构建', '测试', 'bug', '运行', '部署', '数据库', '函数', '组件', '前端', '后端', '开发'],
      role: '在调试系统逻辑的研发探索者',
      desc: '今天你像一个在调试系统逻辑的研发探索者。常说“代码、调试、接口”，主要在做模块开发和排查，专注于逻辑的稳定流转。'
    },
    pm: {
      keywords: ['产品', '需求', '设计', '原型', '规划', '文档', '流程', '排期', '上线', '改动', '方案', '业务', '版本', '用户'],
      role: '在拆解任务的产品技术协作者',
      desc: '今天你像一个在拆解任务的产品技术协作者。常说“整理、结构、删除、同步”，主要在处理项目梳理和版本推进。'
    },
    comm: {
      keywords: ['沟通', '对接', '反馈', '确认', '同步', '跟进', '汇报', '对齐', '会议', '群里', '微信', '电话', '通知', '沟通'],
      role: '追求高效对齐进度的团队联络员',
      desc: '今天你像一个追求高效对齐进度的团队联络员。常说“反馈、确认、同步、跟进”，主要在处理跨团队沟通，消除信息不对称。'
    },
    organizer: {
      keywords: ['整理', '结构', '删除', '提取', '梳理', '要点', '记录', '文档', '大纲', '笔记', '写完', '分析', '研究', '内容'],
      role: '在精炼核心观点的思考整理者',
      desc: '今天你像一个在精炼核心观点的思考整理者。常说“整理、要点、梳理”，主要在做碎片想法的整理提炼，让思维更有结构。'
    }
  };

  let bestMatch: keyof typeof matches = 'organizer';
  let maxCount = 0;

  for (const key of Object.keys(matches) as Array<keyof typeof matches>) {
    let count = 0;
    for (const kw of matches[key].keywords) {
      const regex = new RegExp(kw, 'gi');
      count += (combinedText.match(regex) || []).length;
    }
    if (count > maxCount) {
      maxCount = count;
      bestMatch = key;
    }
  }

  if (maxCount === 0) {
    const keys = Object.keys(matches) as Array<keyof typeof matches>;
    const index = Math.abs(combinedText.length) % keys.length;
    bestMatch = keys[index] || 'organizer';
  }

  return {
    role: matches[bestMatch].role,
    desc: matches[bestMatch].desc
  };
}

function formatHeaderDate(): string {
  const now = new Date();
  const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const m = months[now.getMonth()];
  const d = String(now.getDate()).padStart(2, '0');
  const y = now.getFullYear();
  return `${m}/${d} ${y}`;
}

function OverviewShareModal({ metrics, history, onClose }: { metrics: OverviewMetrics; history: DictationSession[]; onClose: () => void }) {
  const [status, setStatus] = useState<string | null>(null);
  const [styleTheme, setStyleTheme] = useState<'dark' | 'light'>('dark');
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  const communitySvg = shareCommunitySvg(metrics, history, styleTheme);
  const dateKey = localDateKey(new Date());

  const chars = metrics.charsToday.toLocaleString();
  const timeParts = getSavedTimeParts(metrics.savedMs);
  const speedRatioVal = metrics.speedRatio > 0 ? metrics.speedRatio.toFixed(1) : '1.0';

  const savePng = async () => {
    try {
      setStatus('正在生成图片...');
      const blob = await svgToPngBlob(communitySvg, 1080, 1350);
      downloadBlob(blob, `openless-${dateKey}-share.png`);
      setStatus('图片下载已启动，请在浏览器下载记录中查看。');
    } catch (error) {
      console.error('[overview-share] png export failed', error);
      setStatus('PNG 生成失败，请重试。');
    }
  };

  const shareText = `今天用 OpenLess 少敲了 ${chars} 字，预计节省 ${timeParts.value} ${timeParts.unit}，嘴比手快 ${speedRatioVal} 倍。#OpenLess`;
  const shareTargets = [
    { label: '微信', icon: 'wechat', bg: '#07C160', shadow: 'rgba(7, 193, 96, 0.32)' },
    { label: 'X', icon: 'x', bg: '#0f1419', shadow: 'rgba(15, 20, 25, 0.35)' },
    { label: 'QQ', icon: 'qq', bg: '#12B7F5', shadow: 'rgba(18, 183, 245, 0.32)' },
    { label: '微博', icon: 'weibo', bg: '#E6162D', shadow: 'rgba(230, 22, 45, 0.32)' },
  ] as const;

  const copySharePack = async (platform: string) => {
    try {
      setStatus(`正在准备${platform}图片和文案...`);
      const blob = await svgToPngBlob(communitySvg, 1080, 1350);
      // ponytail: browser clipboard is enough for this demo; native share sheet comes later if users ask.
      if (navigator.clipboard?.write && 'ClipboardItem' in window) {
        await navigator.clipboard.write([
          new ClipboardItem({
            'image/png': blob,
            'text/plain': new Blob([shareText], { type: 'text/plain' }),
          }),
        ]);
        setStatus(`${platform}图片和文案已复制，打开${platform}后直接粘贴。`);
        return;
      }
      await navigator.clipboard?.writeText(shareText);
      setStatus(`${platform}文案已复制；浏览器暂不支持复制图片，请先保存图片再粘贴。`);
    } catch (error) {
      console.error('[overview-share] clipboard share failed', error);
      try {
        await navigator.clipboard?.writeText(shareText);
        setStatus(`${platform}文案已复制；图片请用“保存本地”后手动上传。`);
      } catch {
        setStatus('剪贴板不可用，请先保存图片，再复制下方文案。');
      }
    }
  };

  return (
    <Modal onClose={onClose} width="min(460px, 100%)">
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 16, marginBottom: 16 }}>
        <div>
          <div style={{ fontSize: 18, fontWeight: 700, color: 'var(--ol-ink)' }}>分享今日效率</div>
          <div style={{ marginTop: 5, fontSize: 12, color: 'var(--ol-ink-4)' }}>支持两种风格，可直接下载或分享。</div>
        </div>
        <Btn icon="close" size="sm" variant="ghost" onClick={onClose}>关闭</Btn>
      </div>

      {/* 风格切换 */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 16, background: 'var(--ol-control-muted)', padding: 3, borderRadius: 9 }}>
        <button
          type="button"
          onClick={() => setStyleTheme('dark')}
          style={{
            flex: 1,
            height: 32,
            borderRadius: 7,
            border: 'none',
            background: styleTheme === 'dark' ? 'var(--ol-surface)' : 'transparent',
            color: styleTheme === 'dark' ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
            fontSize: 12,
            fontWeight: 600,
            cursor: 'pointer',
            boxShadow: styleTheme === 'dark' ? 'var(--ol-shadow-sm)' : 'none',
            transition: 'all 0.16s var(--ol-motion-quick)'
          }}
        >
          科技暗黑
        </button>
        <button
          type="button"
          onClick={() => setStyleTheme('light')}
          style={{
            flex: 1,
            height: 32,
            borderRadius: 7,
            border: 'none',
            background: styleTheme === 'light' ? 'var(--ol-surface)' : 'transparent',
            color: styleTheme === 'light' ? 'var(--ol-blue)' : 'var(--ol-ink-3)',
            fontSize: 12,
            fontWeight: 600,
            cursor: 'pointer',
            boxShadow: styleTheme === 'light' ? 'var(--ol-shadow-sm)' : 'none',
            transition: 'all 0.16s var(--ol-motion-quick)'
          }}
        >
          温暖纸本
        </button>
      </div>

      {/* 预览卡片 */}
      <div style={{ display: 'flex', justifyContent: 'center', marginBottom: 16 }}>
        <div style={{ width: '100%' }}>
          <SharePreview
            title="今日少敲字数卡片"
            src={svgDataUrl(communitySvg)}
            tall
            dark={styleTheme === 'dark'}
          />
        </div>
      </div>

      <textarea
        readOnly
        value={shareText}
        onFocus={(event) => event.currentTarget.select()}
        style={{
          width: '100%',
          minHeight: 58,
          marginBottom: 12,
          padding: '9px 10px',
          border: '0.5px solid var(--ol-line)',
          borderRadius: 8,
          resize: 'none',
          background: 'var(--ol-control-muted)',
          color: 'var(--ol-ink-2)',
          font: '12px/1.45 inherit',
        }}
      />

      {/* 状态提示 */}
      {status && (
        <div style={{
          marginBottom: 14,
          padding: '8px 12px',
          borderRadius: 6,
          background: 'var(--ol-blue-soft)',
          color: 'var(--ol-blue)',
          fontSize: 12,
          textAlign: 'center',
          animation: 'ol-fade-in 0.2s ease-out'
        }}>
          {status}
        </div>
      )}

      {/* 平台按钮 */}
      <div style={{
        display: 'flex',
        justifyContent: 'center',
        flexWrap: 'wrap',
        gap: 16,
        marginTop: 18,
        marginBottom: 8
      }}>
        {/* 保存到本地 */}
        <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6 }}>
          <button
            aria-label="保存本地"
            onClick={() => void savePng()}
            onMouseEnter={() => setHoveredIdx(0)}
            onMouseLeave={() => setHoveredIdx(null)}
            style={{
              width: 50,
              height: 50,
              borderRadius: '50%',
              background: 'var(--ol-accent-solid-bg)',
              color: 'var(--ol-accent-solid-ink)',
              border: 'none',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              cursor: 'pointer',
              boxShadow: hoveredIdx === 0 ? '0 4px 12px rgba(37, 99, 235, 0.35)' : '0 2px 6px rgba(37, 99, 235, 0.15)',
              transform: hoveredIdx === 0 ? 'scale(1.08) translateY(-2px)' : 'scale(1)',
              transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)'
            }}
          >
            <Icon name="download" size={20} />
          </button>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-3)', fontWeight: 600 }}>保存本地</span>
        </div>

        {shareTargets.map((target, idx) => {
          const hoverIdx = idx + 1;
          return (
            <div key={target.label} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 6 }}>
              <button
                aria-label={`复制到${target.label}`}
                onClick={() => void copySharePack(target.label)}
                onMouseEnter={() => setHoveredIdx(hoverIdx)}
                onMouseLeave={() => setHoveredIdx(null)}
                style={{
                  width: 50,
                  height: 50,
                  borderRadius: '50%',
                  background: target.bg,
                  color: '#ffffff',
                  border: 'none',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  cursor: 'pointer',
                  boxShadow: hoveredIdx === hoverIdx ? `0 4px 12px ${target.shadow}` : '0 2px 6px rgba(15, 23, 42, 0.12)',
                  transform: hoveredIdx === hoverIdx ? 'scale(1.08) translateY(-2px)' : 'scale(1)',
                  transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)'
                }}
              >
                <ShareLogo name={target.icon} />
              </button>
              <span style={{ fontSize: 11, color: 'var(--ol-ink-3)', fontWeight: 600 }}>{target.label}</span>
            </div>
          );
        })}
      </div>

      <div style={{ fontSize: 11, color: 'var(--ol-ink-4)', textAlign: 'center', marginTop: 12 }}>
        生成内容仅在本地处理；平台按钮只复制图片和文案，不拼分享 URL。
      </div>
    </Modal>
  );
}

function ShareLogo({ name }: { name: 'wechat' | 'x' | 'qq' | 'weibo' }) {
  if (name === 'wechat') {
    return (
      <svg width="25" height="25" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <path fill="currentColor" d="M9.2 4.2c-4.1 0-7.4 2.6-7.4 5.9 0 1.8 1 3.5 2.7 4.6l-.6 2.1 2.5-1.2c.9.2 1.8.4 2.8.4.2 0 .5 0 .7-.1a6.2 6.2 0 0 1-.3-1.8c0-3.2 3-5.8 6.6-5.8h.4c-.9-2.4-3.8-4.1-7.4-4.1Zm-2.5 4.9a.9.9 0 1 1 0-1.8.9.9 0 0 1 0 1.8Zm5 0a.9.9 0 1 1 0-1.8.9.9 0 0 1 0 1.8Z"/>
        <path fill="currentColor" d="M22.2 14.1c0-2.7-2.7-4.9-6-4.9s-6 2.2-6 4.9 2.7 4.9 6 4.9c.7 0 1.4-.1 2-.3l2 1-.5-1.7c1.5-.9 2.5-2.3 2.5-3.9Zm-8.1-.8a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5Zm4.1 0a.75.75 0 1 1 0-1.5.75.75 0 0 1 0 1.5Z"/>
      </svg>
    );
  }
  if (name === 'x') {
    return (
      <svg width="19" height="19" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z"/>
      </svg>
    );
  }
  if (name === 'qq') {
    return (
      <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
        <path d="M21.4 15a40 40 0 0 0-.8-2.3l-1.1-2.7v-.8C19.5 4.6 17.4 0 12 0S4.5 4.6 4.5 9.2v.8l-1.1 2.7c-.3.7-.6 1.5-.8 2.3-1 3.3-.7 4.6-.4 4.7.5.1 2.1-2.5 2.1-2.5 0 1.5.8 3.4 2.4 4.8-.6.2-1.4.5-1.9.8-.4.3-.4.6-.3.8.3.6 5.9.4 7.5.2 1.6.2 7.1.4 7.5-.2.1-.1.1-.5-.3-.8-.5-.4-1.2-.6-1.9-.8 1.6-1.4 2.4-3.3 2.4-4.8 0 0 1.6 2.5 2.1 2.5.3-.1.6-1.4-.4-4.7ZM7.4 8.3c.2-.4 2.2-.9 4.6-.9s4.4.5 4.6.9c0 .4-2.3 1.6-4.6 1.6S7.4 8.7 7.4 8.3Zm10.4 8.6c-.2 3.7-2.4 6-5.8 6s-5.6-2.3-5.8-6c-.1-1.4 0-2.5.1-3.4 2 .4 4 .6 5.7.6s3.8-.2 5.7-.6c.2.9.3 2 .1 3.4Z"/>
      </svg>
    );
  }
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M10.1 20.3c-4 .4-7.4-1.4-7.7-4-.3-2.6 2.8-5.1 6.7-5.4 4-.4 7.4 1.4 7.7 4 .3 2.6-2.7 5-6.7 5.4Zm-.6-7.4c-1.9-.5-4 .5-4.9 2.1-.8 1.7 0 3.6 1.9 4.2 2 .6 4.3-.3 5.1-2.2.8-1.8-.2-3.6-2.1-4.1Zm8.6-1.3c-.4-.1-.6-.2-.4-.6.4-1 .4-1.8 0-2.4-.8-1.1-2.9-1.1-5.4 0 0 0-.8.3-.6-.3.4-1.2.3-2.2-.3-2.8-1.3-1.3-4.9 0-7.9 3C1.3 10.9 0 13.3 0 15.3c0 4 5.1 6.4 10.1 6.4 6.5 0 10.9-3.8 10.9-6.8 0-1.8-1.6-2.9-2.9-3.3Zm1.9-5.1c-.8-.9-1.9-1.2-3-.9-.4.1-.7.5-.6.9.1.4.5.7.9.6.5-.1 1.1 0 1.4.5.4.4.5 1 .3 1.5-.1.4.1.9.5 1 .4.1.9-.1 1-.5.3-1 .1-2.2-.5-3.1Zm2.4-2.2c-1.6-1.8-3.9-2.4-6.1-2-.5.1-.8.6-.7 1.1.1.5.6.8 1.1.7 1.5-.3 3.2.1 4.3 1.4s1.4 2.9.9 4.4c-.2.5.1 1 .6 1.2.5.2 1-.1 1.2-.6.7-2.1.2-4.5-1.3-6.2Z"/>
    </svg>
  );
}

function SharePreview({ title, src, tall = false, dark = false }: { title: string; src: string; tall?: boolean; dark?: boolean }) {
  return (
    <div>
      <div style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-3)', marginBottom: 8 }}>{title}</div>
      <div style={{ border: '0.5px solid var(--ol-line)', borderRadius: 10, background: dark ? '#090d16' : '#ffffff', padding: 10 }}>
        <img
          src={src}
          alt={title}
          style={{
            display: 'block',
            width: '100%',
            height: tall ? 380 : 'auto',
            objectFit: 'contain',
            borderRadius: 8,
            background: dark ? '#0f172a' : '#f8fafc',
          }}
        />
      </div>
    </div>
  );
}

function WeekChart({ data }: { data: number[] }) {
  const max = Math.max(...data, 1);
  return (
    <div style={{ display: 'flex', alignItems: 'flex-end', gap: 8, height: 100 }}>
      {data.map((v, i) => {
        const isToday = i === 6;
        return (
          <div key={i} style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 4 }}>
            <div style={{ fontSize: 9.5, color: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)', fontWeight: isToday ? 600 : 400 }}>{v}</div>
            <div
              style={{
                width: '100%',
                height: `${(v / max) * 80}px`,
                minHeight: 2,
                borderRadius: 4,
                background: isToday ? 'var(--ol-blue)' : 'var(--ol-ink-4)',
                opacity: v === 0 ? 0.15 : isToday ? 1 : 0.85,
                transition: 'height 0.18s var(--ol-motion-soft), opacity 0.18s var(--ol-motion-soft)',
              }}
            />
          </div>
        );
      })}
    </div>
  );
}

function RecentRow({ session, modeLabel }: { session: DictationSession; modeLabel: Record<PolishMode, string> }) {
  const { t } = useTranslation();
  return (
    <div style={{ padding: '12px 18px', borderBottom: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 12, alignItems: 'flex-start' }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 4, minWidth: 60 }}>
        <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
          {formatTime(session.createdAt)}
        </span>
        <Pill size="sm" tone="default">{modeLabel[session.mode]}</Pill>
      </div>
      <div style={{ flex: 1, fontSize: 12.5, color: 'var(--ol-ink-2)', whiteSpace: 'pre-line', lineHeight: 1.55, overflow: 'hidden', textOverflow: 'ellipsis', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}>
        {session.finalText.split('\n')[0]}
      </div>
      <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
        {formatDuration(session.durationMs ?? 0, t)}
      </span>
    </div>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  const pad = (n: number) => String(n).padStart(2, '0');
  if (sameDay) return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function formatDuration(ms: number, t: ReturnType<typeof useTranslation>['t']): string {
  if (ms <= 0) return t('common.durationSeconds', { value: '0.0' });
  const sec = ms / 1000;
  if (sec < 60) return t('common.durationSeconds', { value: sec.toFixed(1) });
  if (sec < 3600) return t('common.durationMinutes', { value: (sec / 60).toFixed(1) });
  return t('common.durationHours', { value: (sec / 3600).toFixed(1) });
}

function efficiencyTinyText(metrics: OverviewMetrics): string {
  if (metrics.charsToday <= 0) return '暂无效率估算';
  switch (new Date().getDate() % 3) {
    case 0:
      return `预计节省 ${formatSavedHours(metrics.savedMs)}`;
    case 1:
      return metrics.speedRatio > 0 ? `嘴比手快 ${metrics.speedRatio.toFixed(1)} 倍` : `少敲 ${metrics.charsToday.toLocaleString()} 字`;
    default:
      return `少敲 ${metrics.charsToday.toLocaleString()} 字`;
  }
}

function formatSavedHours(ms: number): string {
  return `${Math.max(0, ms / 3600000).toFixed(1)} 小时`;
}

function formatSavedMinutes(ms: number): string {
  return `${Math.max(0, Math.round(ms / 60000)).toLocaleString()} 分钟`;
}

function getSavedTimeParts(ms: number): { value: string; unit: string } {
  const mins = Math.max(0, Math.round(ms / 60000));
  if (mins < 1) {
    return { value: '< 1', unit: '分钟' };
  }
  if (mins >= 60) {
    const hrs = (mins / 60).toFixed(1);
    const formattedHrs = hrs.endsWith('.0') ? hrs.slice(0, -2) : hrs;
    return { value: formattedHrs, unit: '小时' };
  }
  return { value: String(mins), unit: '分钟' };
}

const SVG_FONT_FAMILY = "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Monaco, Consolas, 'Liberation Mono', 'Courier New', 'PingFang SC', 'Source Han Sans CN', 'Microsoft YaHei', monospace, sans-serif";

function getLocaleDateString(): string {
  const now = new Date();
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, '0');
  const day = String(now.getDate()).padStart(2, '0');
  const dayOfWeek = ['星期日', '星期一', '星期二', '星期三', '星期四', '星期五', '星期六'][now.getDay()];
  return `${year}.${month}.${day} ${dayOfWeek}`;
}

function startOfLocalDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

function localDateKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function activityLevel(count: number, maxCount: number): number {
  if (count <= 0 || maxCount <= 0) return 0;
  const ratio = count / maxCount;
  if (ratio <= 0.25) return 1;
  if (ratio <= 0.5) return 2;
  if (ratio <= 0.75) return 3;
  return 4;
}

function generateHeatmapSvgCells(history: DictationSession[], isDark: boolean): string {
  const today = startOfLocalDay(new Date());
  const gridStart = addDays(today, -364);
  const rangeStart = gridStart;
  const byDay = new Map<string, number>();

  history.forEach(session => {
    const date = startOfLocalDay(new Date(session.createdAt));
    if (isNaN(date.getTime()) || date < rangeStart || date > today) return;
    const key = localDateKey(date);
    byDay.set(key, (byDay.get(key) || 0) + 1);
  });

  const maxCount = Math.max(...Array.from(byDay.values()), 0);

  let rects: string[] = [];
  const cellSize = 10;
  const cellGap = 3;

  const getCellColor = (level: number) => {
    if (isDark) {
      const colors = ['rgba(255,255,255,0.06)', 'rgba(96, 165, 250, 0.25)', 'rgba(96, 165, 250, 0.5)', 'rgba(96, 165, 250, 0.75)', '#60a5fa'];
      return colors[level];
    } else {
      const colors = ['#F0E6DC', '#D9C1AE', '#BC8873', '#9A5647', '#7E3E34'];
      return colors[level];
    }
  };

  for (let week = 0; week < 53; week++) {
    for (let day = 0; day < 7; day++) {
      const offset = week * 7 + day;
      const date = addDays(gridStart, offset);
      if (date > today) continue;

      const key = localDateKey(date);
      const count = byDay.get(key) || 0;
      const level = activityLevel(count, maxCount);

      const x = week * (cellSize + cellGap);
      const y = day * (cellSize + cellGap);
      const fill = getCellColor(level);

      rects.push(`<rect x="${x}" y="${y}" width="${cellSize}" height="${cellSize}" rx="2" fill="${fill}" />`);
    }
  }

  return rects.join('\n');
}

function shareCommunitySvg(metrics: OverviewMetrics, history: DictationSession[], theme: 'dark' | 'light' = 'dark'): string {
  const chars = metrics.charsToday.toLocaleString();
  const timeParts = getSavedTimeParts(metrics.savedMs);
  const speedRatioVal = metrics.speedRatio > 0 ? metrics.speedRatio.toFixed(1) : '1.0';

  const headerDate = formatHeaderDate();
  const streak = calculateStreak(history);
  const streakText = streak > 1 ? `DAY ${streak}` : '今日记录';
  const totalSessions = history.length;
  const userLevel = Math.floor(totalSessions / 10) + 1;
  const portrait = generatePortrait(history);

  // Split portrait description into two sentences
  const descParts = portrait.desc.split('。');
  const descLine1 = descParts[0] ? descParts[0] + '。' : '';
  const descLine2 = descParts[1] ? descParts[1] + '。' : '';

  const isDark = theme === 'dark';

  // Background & layout
  const bgGradStart1 = isDark ? '#1e1b4b' : '#F3E8EE';
  const bgGradEnd1 = isDark ? '#0f172a' : '#F7F1E9';
  const bgGradStart2 = isDark ? '#0f172a' : '#F7F1E9';
  const bgGradEnd2 = isDark ? '#090d16' : '#FFF9F2';

  const glowTopColor = isDark ? '#3b82f6' : '#D97757';
  const glowTopOpacity = isDark ? '0.22' : '0.06';
  const glowBottomColor = isDark ? '#8b5cf6' : '#E6B673';
  const glowBottomOpacity = isDark ? '0.25' : '0.06';

  const gridColor = isDark ? '#38bdf8' : '#2B1D13';
  const gridOpacity = isDark ? '0.04' : '0.015';
  const innerBorderColor = isDark ? 'rgba(255, 255, 255, 0.05)' : 'rgba(43, 29, 19, 0.04)';

  // Text Accent Gradient
  const accentGradStart = isDark ? '#60a5fa' : '#D97757';
  const accentGradEnd = isDark ? '#c084fc' : '#9C5A3C';

  // Header text colors
  const logoSymbolColor = '#ffffff'; // Always white inside the filled badge
  const headerTitleColor = isDark ? '#ffffff' : '#2B1D13';
  const headerDateColor = isDark ? '#ffffff' : '#2B1D13';

  // Metrics row colors
  const metricLabelColor = isDark ? '#94a3b8' : '#6B5142';
  const metricValueColor = isDark ? '#ffffff' : '#2B1D13';

  // Central card colors
  const centerCardBg = isDark ? 'rgba(255, 255, 255, 0.04)' : '#ffffff';
  const centerCardBorder = isDark ? 'rgba(255, 255, 255, 0.08)' : '#e8d7c6';
  const centerCardTitleColor = isDark ? '#ffffff' : '#2B1D13';
  const centerCardTextColor = isDark ? '#94a3b8' : '#6B5142';
  const centerCardDivider = isDark ? 'rgba(255, 255, 255, 0.08)' : 'rgba(43, 29, 19, 0.06)';

  // Bottom Evidence Card colors
  const evidenceCardBg = isDark ? 'rgba(255, 255, 255, 0.02)' : '#FFF9F2';
  const evidenceCardBorder = isDark ? 'rgba(255, 255, 255, 0.05)' : '#e8d7c6';
  const evidenceTitleColor = isDark ? '#ffffff' : '#2B1D13';
  const evidenceLabelColor = isDark ? '#475569' : '#8A7264';

  // Pills
  const pillLeftBg = isDark ? 'rgba(96, 165, 250, 0.1)' : 'rgba(217, 119, 87, 0.06)';
  const pillLeftBorder = isDark ? 'rgba(96, 165, 250, 0.25)' : 'rgba(217, 119, 87, 0.18)';
  const pillLeftText = isDark ? '#60a5fa' : '#D97757';

  const pillRightBg = isDark ? 'rgba(192, 132, 252, 0.1)' : 'rgba(230, 182, 115, 0.06)';
  const pillRightBorder = isDark ? 'rgba(192, 132, 252, 0.2)' : 'rgba(230, 182, 115, 0.15)';
  const pillRightText = isDark ? '#c084fc' : '#9C5A3C';

  // Footer
  const footerDividerColor = isDark ? 'rgba(255, 255, 255, 0.08)' : 'rgba(43, 29, 19, 0.08)';
  const footerNoteColor = isDark ? '#94a3b8' : '#6B5142';
  const footerBrandColor = isDark ? '#475569' : '#8A7264';

  const footerLogoRing = isDark ? 'rgba(96, 165, 250, 0.15)' : 'rgba(217, 119, 87, 0.12)';
  const footerLogoInnerBg = isDark ? 'rgba(96, 165, 250, 0.05)' : 'rgba(217, 119, 87, 0.03)';
  const footerMicColor = isDark ? '#60a5fa' : '#D97757';

  return `
<svg xmlns="http://www.w3.org/2000/svg" width="1080" height="1350" viewBox="0 0 1080 1350">
  <defs>
    <linearGradient id="bg-grad-top" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${bgGradStart1}"/>
      <stop offset="100%" stop-color="${bgGradEnd1}"/>
    </linearGradient>
    <linearGradient id="bg-grad-bottom" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="${bgGradStart2}"/>
      <stop offset="100%" stop-color="${bgGradEnd2}"/>
    </linearGradient>
    <radialGradient id="glow-top" cx="90%" cy="10%" r="60%">
      <stop offset="0%" stop-color="${glowTopColor}" stop-opacity="${glowTopOpacity}"/>
      <stop offset="100%" stop-color="${glowTopColor}" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="glow-bottom" cx="10%" cy="90%" r="65%">
      <stop offset="0%" stop-color="${glowBottomColor}" stop-opacity="${glowBottomOpacity}"/>
      <stop offset="100%" stop-color="${glowBottomColor}" stop-opacity="0"/>
    </radialGradient>
    <pattern id="grid" width="60" height="60" patternUnits="userSpaceOnUse">
      <path d="M 60 0 L 0 0 0 60" fill="none" stroke="${gridColor}" stroke-width="1" opacity="${gridOpacity}"/>
    </pattern>
    <linearGradient id="text-accent-grad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="${accentGradStart}"/>
      <stop offset="100%" stop-color="${accentGradEnd}"/>
    </linearGradient>
  </defs>

  <!-- Diagonal Split Background -->
  <polygon points="0,0 1080,0 1080,480 0,650" fill="url(#bg-grad-top)" />
  <polygon points="0,650 1080,480 1080,1350 0,1350" fill="url(#bg-grad-bottom)" />

  <rect width="1080" height="1350" fill="url(#glow-top)"/>
  <rect width="1080" height="1350" fill="url(#glow-bottom)"/>
  <rect width="1080" height="1350" fill="url(#grid)"/>

  <rect x="40" y="40" width="1000" height="1270" rx="40" fill="none" stroke="${innerBorderColor}" stroke-width="2"/>

  <!-- Header -->
  <!-- Left Side: User Info -->
  <circle cx="104" cy="130" r="24" fill="${accentGradStart}" opacity="0.15" />
  <circle cx="104" cy="130" r="20" fill="url(#text-accent-grad)" />
  <text x="104" y="137" text-anchor="middle" fill="#ffffff" font-family="${SVG_FONT_FAMILY}" font-size="20" font-weight="800">O</text>

  <text x="144" y="125" fill="${headerTitleColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="700">OpenLess User</text>
  <rect x="144" y="135" width="56" height="20" rx="6" fill="${pillLeftBg}" stroke="${pillLeftBorder}" stroke-width="1" />
  <text x="172" y="149" text-anchor="middle" fill="${pillLeftText}" font-family="${SVG_FONT_FAMILY}" font-size="11" font-weight="700">LVL ${userLevel}</text>

  <!-- Right Side: Date & Streak -->
  <text x="1000" y="125" text-anchor="end" fill="${headerDateColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="700" letter-spacing="1px">${escapeSvg(headerDate)}</text>
  <rect x="912" y="135" width="88" height="20" rx="6" fill="${pillRightBg}" stroke="${pillRightBorder}" stroke-width="1" />
  <text x="956" y="149" text-anchor="middle" fill="${pillRightText}" font-family="${SVG_FONT_FAMILY}" font-size="11" font-weight="700">${escapeSvg(streakText)}</text>

  <!-- Top Metrics Row -->
  <text x="220" y="220" text-anchor="middle" fill="${metricLabelColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="600">今日字数</text>
  <text x="220" y="285" text-anchor="middle" fill="${metricValueColor}" font-family="${SVG_FONT_FAMILY}" font-size="52" font-weight="800">${escapeSvg(chars)}</text>

  <text x="540" y="220" text-anchor="middle" fill="${metricLabelColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="600">今日段数</text>
  <text x="540" y="285" text-anchor="middle" fill="${metricValueColor}" font-family="${SVG_FONT_FAMILY}" font-size="52" font-weight="800">${metrics.segmentsToday}</text>

  <text x="860" y="220" text-anchor="middle" fill="${metricLabelColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="600">预计节省时间</text>
  <text x="860" y="285" text-anchor="middle" fill="${metricValueColor}" font-family="${SVG_FONT_FAMILY}" font-size="52" font-weight="800">${(metrics.savedMs / 3600000).toFixed(1)}<tspan font-size="24" font-weight="700">h</tspan></text>

  <!-- Central White Card: Today's Oral Portrait -->
  <rect x="80" y="330" width="920" height="410" rx="24" fill="${centerCardBg}" stroke="${centerCardBorder}" stroke-width="1.5"/>

  <text x="130" y="390" fill="${centerCardTitleColor}" font-family="${SVG_FONT_FAMILY}" font-size="20" font-weight="700" letter-spacing="1px">今日口述画像</text>
  <line x1="130" y1="420" x2="950" y2="420" stroke="${centerCardDivider}" stroke-width="1"/>

  <g transform="translate(880, 365)" fill="${pillLeftText}" opacity="0.8">
    <path d="M12 3L14.5 9.5 21 12 14.5 14.5 12 21 9.5 14.5 3 12 9.5 9.5 12 3Z" />
  </g>

  <text x="130" y="485" fill="url(#text-accent-grad)" font-family="${SVG_FONT_FAMILY}" font-size="30" font-weight="800">${escapeSvg(portrait.role)}</text>
  <text x="130" y="555" fill="${centerCardTextColor}" font-family="${SVG_FONT_FAMILY}" font-size="20" font-weight="600" letter-spacing="0.5px">${escapeSvg(descLine1)}</text>
  <text x="130" y="605" fill="${centerCardTextColor}" font-family="${SVG_FONT_FAMILY}" font-size="20" font-weight="600" letter-spacing="0.5px">${escapeSvg(descLine2)}</text>

  <!-- Bottom Heatmap Evidence Card -->
  <text x="80" y="810" fill="${evidenceTitleColor}" font-family="${SVG_FONT_FAMILY}" font-size="22" font-weight="700" letter-spacing="1px">年度口述活跃度</text>
  <rect x="80" y="840" width="920" height="200" rx="24" fill="${evidenceCardBg}" stroke="${evidenceCardBorder}" stroke-width="1.5" />

  <g transform="translate(197, 885)">
    ${generateHeatmapSvgCells(history, isDark)}
  </g>
  <text x="540" y="1005" text-anchor="middle" fill="${evidenceLabelColor}" font-family="${SVG_FONT_FAMILY}" font-size="14" font-weight="600">过去 12 个月的口述效率记录格</text>

  <!-- Footer -->
  <line x1="80" y1="1120" x2="1000" y2="1120" stroke="${footerDividerColor}" stroke-width="2"/>
  <text x="80" y="1185" fill="${footerNoteColor}" font-family="${SVG_FONT_FAMILY}" font-size="22" font-weight="600">按 ${TYPING_CHARS_PER_MINUTE} 字/分钟手打速度估算</text>
  <text x="80" y="1225" fill="${footerBrandColor}" font-family="${SVG_FONT_FAMILY}" font-size="18" font-weight="500">OpenLess · 极简本地语音录入助手</text>

  <circle cx="940" cy="1205" r="48" fill="none" stroke="${footerLogoRing}" stroke-width="2" stroke-dasharray="6 4"/>
  <circle cx="940" cy="1205" r="38" fill="${footerLogoInnerBg}" stroke="url(#text-accent-grad)" stroke-width="2.5"/>
  <g transform="translate(928, 1191)">
    <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" fill="${footerMicColor}"/>
    <path d="M19 10v1a7 7 0 0 1-14 0v-1" fill="none" stroke="${footerMicColor}" stroke-width="2.5" stroke-linecap="round"/>
    <line x1="12" y1="18" x2="12" y2="22" stroke="${footerMicColor}" stroke-width="2.5" stroke-linecap="round"/>
  </g>
</svg>`.trim();
}

function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function escapeSvg(value: string): string {
  return value.replace(/[&<>"']/g, char => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&apos;' })[char] ?? char);
}

function downloadBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  window.setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

async function svgToPngBlob(svg: string, width: number, height: number): Promise<Blob> {
  const url = URL.createObjectURL(new Blob([svg], { type: 'image/svg+xml;charset=utf-8' }));
  try {
    const image = new Image();
    await new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new Error('SVG 预览加载失败'));
      image.src = url;
    });
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('Canvas 不可用');
    ctx.drawImage(image, 0, 0, width, height);
    return await new Promise<Blob>((resolve, reject) => {
      canvas.toBlob(blob => blob ? resolve(blob) : reject(new Error('PNG 生成失败')), 'image/png');
    });
  } finally {
    URL.revokeObjectURL(url);
  }
}

function weekDayLabels(names: string[]): string[] {
  const today = new Date().getDay();
  const out: string[] = [];
  for (let i = 6; i >= 0; i--) {
    out.push(names[(today - i + 7) % 7]);
  }
  return out;
}
