// Less Computer leaf components: avatars, per-bubble actions and the summary
// inspector. The inspector only aggregates the current turn; tool details stay
// in the conversation so no second activity feed is created.
import { useEffect, useRef, useState } from 'react';
import type { useTranslation } from 'react-i18next';
import { CheckIcon, ChevronsRightIcon, CopyIcon } from 'lucide-react';
import { Tooltip } from '../components/Tooltip';
import type { CodingAgentProviderId } from '../lib/types';

type Translate = ReturnType<typeof useTranslation>['t'];

const AGENT_MARKS: Record<CodingAgentProviderId, string> = {
  'claude-code-cli': 'CC',
  'opencode-cli': 'OC',
  'codex-cli': 'Cx',
  'dsh-cli': 'ds',
};

export type RunTone = 'idle' | 'working' | 'waiting' | 'done' | 'error' | 'cancelled';

export function AgentAvatar({
  agentId,
  size = 'md',
  active = false,
}: {
  agentId: CodingAgentProviderId | null;
  size?: 'sm' | 'md' | 'lg';
  active?: boolean;
}) {
  return (
    <span className={`lc-avatar is-${size}${active ? ' is-active' : ''}`} aria-hidden="true">
      {agentId ? AGENT_MARKS[agentId] : 'LC'}
    </span>
  );
}

export function CopyAction({ text, t }: { text: string; t: Translate }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    },
    [],
  );
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      if (timer.current != null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => setCopied(false), 1400);
    } catch {
      /* clipboard unavailable: nothing was copied, so no confirmation */
    }
  };
  const label = copied ? t('lessComputer.desktop.copied') : t('lessComputer.desktop.copy');
  return (
    <div className="lc-message-actions">
      <Tooltip content={label} placement="top">
        <button
          type="button"
          className={`lc-icon-button is-small${copied ? ' is-confirmed' : ''}`}
          aria-label={label}
          onClick={() => void copy()}
        >
          {copied ? <CheckIcon /> : <CopyIcon />}
        </button>
      </Tooltip>
    </div>
  );
}

export interface InspectorSummary {
  agentId: CodingAgentProviderId | null;
  agentName: string | null;
  statusLabel: string;
  tone: RunTone;
  toolCount: number;
  pendingApprovals: number;
  costUsd: number | null;
  voiceHint: string | null;
}

export function LessComputerInspector({
  open,
  summary,
  onClose,
  t,
}: {
  open: boolean;
  summary: InspectorSummary;
  onClose: () => void;
  t: Translate;
}) {
  const closeLabel = t('lessComputer.desktop.hideInspector');
  return (
    <aside
      className="lc-inspector"
      aria-label={t('lessComputer.desktop.inspectorTitle')}
      aria-hidden={!open || undefined}
    >
      <div className="lc-inspector-inner">
        <div className="lc-inspector-head" data-tauri-drag-region>
          <span data-tauri-drag-region>{t('lessComputer.desktop.inspectorTitle')}</span>
          <Tooltip content={closeLabel} placement="bottom">
            <button
              type="button"
              className="lc-icon-button"
              aria-label={closeLabel}
              tabIndex={open ? undefined : -1}
              onClick={onClose}
            >
              <ChevronsRightIcon />
            </button>
          </Tooltip>
        </div>
        <section className="lc-inspector-card lc-inspector-agent">
          <AgentAvatar agentId={summary.agentId} size="lg" active={summary.agentId != null} />
          <div>
            <strong>{summary.agentName ?? t('lessComputer.title')}</strong>
            <span>
              {summary.agentId
                ? t('lessComputer.desktop.configured')
                : t('lessComputer.desktop.agentSettings')}
            </span>
          </div>
        </section>
        <section className="lc-inspector-section">
          <h3>{t('lessComputer.desktop.inspectorStatus')}</h3>
          <div className={`lc-inspector-status is-${summary.tone}`}>
            <span aria-hidden="true" />
            {summary.statusLabel}
          </div>
        </section>
        <section className="lc-inspector-section">
          <h3>{t('lessComputer.desktop.inspectorTurn')}</h3>
          <dl className="lc-inspector-stats">
            <div>
              <dt>{t('lessComputer.desktop.toolCalls')}</dt>
              <dd>{summary.toolCount}</dd>
            </div>
            <div>
              <dt>{t('lessComputer.desktop.pendingApprovals')}</dt>
              <dd className={summary.pendingApprovals > 0 ? 'is-attention' : undefined}>
                {summary.pendingApprovals}
              </dd>
            </div>
            <div>
              <dt>{t('lessComputer.desktop.apiCost')}</dt>
              <dd>
                {summary.costUsd != null
                  ? t('lessComputer.cost', { cost: summary.costUsd.toFixed(3) })
                  : '—'}
              </dd>
            </div>
          </dl>
        </section>
        <section className="lc-inspector-section">
          <h3>{t('lessComputer.desktop.inspectorVoice')}</h3>
          <p className="lc-inspector-text">
            {summary.voiceHint ?? t('lessComputer.desktop.voiceShortcutOff')}
          </p>
          <p className="lc-inspector-note">{t('lessComputer.desktop.voiceModesHint')}</p>
        </section>
      </div>
    </aside>
  );
}
