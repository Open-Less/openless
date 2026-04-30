// Overview.tsx — 真实指标，从 listHistory + getCredentials 派生。

import { useEffect, useMemo, useState } from 'react';
import { Icon } from '../components/Icon';
import { getHotkeyTriggerLabel } from '../lib/hotkey';
import { getCredentials, listHistory } from '../lib/ipc';
import type { CredentialsStatus, DictationSession, PolishMode } from '../lib/types';
import { useHotkeySettings } from '../state/HotkeySettingsContext';
import { Btn, Card, PageHeader, Pill } from './_atoms';

const MODE_LABEL: Record<PolishMode, string> = {
  raw: '原文',
  light: '轻度润色',
  structured: '清晰结构',
  formal: '正式表达',
};

interface OverviewProps {
  onOpenHistory?: () => void;
}

export function Overview({ onOpenHistory }: OverviewProps) {
  const [history, setHistory] = useState<DictationSession[]>([]);
  const [creds, setCreds] = useState<CredentialsStatus>({
    volcengineConfigured: false,
    arkConfigured: false,
  });
  const { hotkey } = useHotkeySettings();

  useEffect(() => {
    listHistory().then(setHistory);
    getCredentials().then(setCreds);
  }, []);

  const metrics = useMemo(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const todays = history.filter(s => new Date(s.createdAt) >= today);
    const charsToday = todays.reduce((acc, s) => acc + s.finalText.length, 0);
    const segmentsToday = todays.length;
    const totalDurationMs = todays.reduce((acc, s) => acc + (s.durationMs ?? 0), 0);
    const avgLatencyMs = segmentsToday > 0 ? totalDurationMs / segmentsToday : 0;
    return { charsToday, segmentsToday, totalDurationMs, avgLatencyMs };
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
            }}>{getHotkeyTriggerLabel(hotkey?.trigger)}</kbd>
            开始录音
          </div>
        }
      />

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 18 }}>
        <ProviderCard
          kind="ASR 语音"
          name="火山引擎"
          subname="bigmodel"
          configured={creds.volcengineConfigured}
        />
        <ProviderCard
          kind="LLM 模型"
          name="OpenAI 兼容"
          subname={creds.arkConfigured ? '已配置 active LLM' : '未配置'}
          configured={creds.arkConfigured}
        />
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(4, 1fr)', gap: 12, marginBottom: 18 }}>
        <Metric icon="hash" label="今日字数" value={metrics.charsToday.toLocaleString()} trend={`${metrics.segmentsToday} 段`} />
        <Metric icon="mic" label="今日总时长" value={formatDuration(metrics.totalDurationMs)} trend="" />
        <Metric icon="clock" label="平均段落" value={formatDuration(metrics.avgLatencyMs)} trend={metrics.segmentsToday > 0 ? '今日均值' : '暂无数据'} />
        <Metric icon="bolt" label="累计记录" value={String(history.length)} trend="本机存档 (上限 200)" accent />
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1.4fr', gap: 12 }}>
        <Card padding={18}>
          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 14 }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>近 7 天</span>
            <span style={{ fontSize: 11, color: 'var(--ol-ink-4)' }}>条数 / 天</span>
          </div>
          <WeekChart data={weekly} />
          <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--ol-ink-4)', marginTop: 8 }}>
            {weekDayLabels().map((d, i) => <span key={i}>{d}</span>)}
          </div>
        </Card>

        <Card padding={0}>
          <div style={{ padding: '14px 18px', borderBottom: '0.5px solid var(--ol-line)', display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
            <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--ol-ink-2)' }}>最近识别</span>
            <Btn size="sm" variant="ghost" onClick={onOpenHistory}>全部记录 →</Btn>
          </div>
          <div>
            {history.length === 0 && (
              <div style={{ padding: 24, textAlign: 'center', fontSize: 12, color: 'var(--ol-ink-4)' }}>
                还没有记录。按 {getHotkeyTriggerLabel(hotkey?.trigger)} 开始第一次录音。
              </div>
            )}
            {history.slice(0, 5).map(s => (
              <RecentRow key={s.id} session={s} />
            ))}
          </div>
        </Card>
      </div>
    </>
  );
}

interface ProviderCardProps {
  kind: string;
  name: string;
  subname: string;
  configured: boolean;
}

function ProviderCard({ kind, name, subname, configured }: ProviderCardProps) {
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
        <Icon name={kind.startsWith('ASR') ? 'mic' : 'sparkle'} size={18} />
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 2 }}>
          <span style={{ fontSize: 11, color: 'var(--ol-ink-4)', fontWeight: 600, letterSpacing: '.06em', textTransform: 'uppercase' }}>{kind}</span>
          {configured ? (
            <Pill tone="ok" size="sm">
              <span style={{ width: 5, height: 5, borderRadius: 999, background: 'var(--ol-ok)' }} />
              已配置
            </Pill>
          ) : (
            <Pill tone="outline" size="sm">未配置</Pill>
          )}
        </div>
        <div style={{ fontSize: 14, fontWeight: 600, color: 'var(--ol-ink)' }}>{name}</div>
        <div style={{ fontSize: 11.5, color: 'var(--ol-ink-3)', marginTop: 1, fontFamily: 'var(--ol-font-mono)' }}>{subname}</div>
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
                background: isToday ? 'var(--ol-blue)' : 'var(--ol-ink)',
                opacity: v === 0 ? 0.15 : isToday ? 1 : 0.85,
              }}
            />
          </div>
        );
      })}
    </div>
  );
}

function RecentRow({ session }: { session: DictationSession }) {
  return (
    <div style={{ padding: '12px 18px', borderBottom: '0.5px solid var(--ol-line-soft)', display: 'flex', gap: 12, alignItems: 'flex-start' }}>
      <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-start', gap: 4, minWidth: 60 }}>
        <span style={{ fontSize: 11, fontFamily: 'var(--ol-font-mono)', color: 'var(--ol-ink-3)' }}>
          {formatTime(session.createdAt)}
        </span>
        <Pill size="sm" tone="default">{MODE_LABEL[session.mode]}</Pill>
      </div>
      <div style={{ flex: 1, fontSize: 12.5, color: 'var(--ol-ink-2)', whiteSpace: 'pre-line', lineHeight: 1.55, overflow: 'hidden', textOverflow: 'ellipsis', display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical' }}>
        {session.finalText.split('\n')[0]}
      </div>
      <span style={{ fontSize: 10.5, color: 'var(--ol-ink-4)', fontFamily: 'var(--ol-font-mono)' }}>
        {formatDuration(session.durationMs ?? 0)}
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

function formatDuration(ms: number): string {
  if (ms <= 0) return '—';
  const sec = ms / 1000;
  if (sec < 60) return `${sec.toFixed(1)}s`;
  return `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
}

function weekDayLabels(): string[] {
  const names = ['日', '一', '二', '三', '四', '五', '六'];
  const today = new Date().getDay();
  const out: string[] = [];
  for (let i = 6; i >= 0; i--) {
    out.push(names[(today - i + 7) % 7]);
  }
  return out;
}
