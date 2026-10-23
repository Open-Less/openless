import { motion, useReducedMotion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from './ui/SelectLite';
import './ui/Select.css';
import { getSettings } from '../lib/ipc/settings';
import { isTauri } from '../lib/ipc/shared';
import {
  getDictationTranslationTarget,
  setCapsuleLanguageMenu,
  selectDictationTranslationTarget,
  type DictationTranslationTarget,
} from '../lib/ipc/dictation';
import { localizedLanguages, nativeLanguageName } from '../lib/languageCatalog';
import { isTranslationTargetRedundant } from '../lib/translationTarget';
import './CapsuleStyles.css';

/** The same Base UI menu as settings, with temporary native headroom above the capsule. */
export function CapsuleTranslationBadge({ visible = true }: { visible?: boolean }) {
  const { t, i18n } = useTranslation();
  const reduced = useReducedMotion();
  const [snapshot, setSnapshot] = useState<DictationTranslationTarget | null>(null);
  const [savedTarget, setSavedTarget] = useState('');
  const [busy, setBusy] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const menuEpoch = useRef(0);
  const menuOwner = useRef<string | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout>>();
  const shown = useRef(visible);
  shown.current = visible;
  const [error, setError] = useState(false);
  const revision = useRef(0);
  const pickerSession = useRef<string | null>(null);
  const pendingChange = useRef<symbol | null>(null);
  const current = useRef(snapshot);
  current.current = snapshot;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void)[] = [];
    let lastRecording = '';
    const refresh = async () => {
      const request = ++revision.current;
      try {
        const [prefs, active] = await Promise.all([getSettings(), getDictationTranslationTarget()]);
        if (disposed || request !== revision.current) return;
        setSavedTarget(prefs.translationTargetLanguage);
        setSnapshot(active);
      } catch {
        if (!disposed && request === revision.current) setSnapshot(null);
      }
    };
    if (!visible) {
      setError(false);
      setBusy(false);
      pickerSession.current = null;
      pendingChange.current = null;
      return;
    }
    void (async () => {
      if (isTauri) {
        const { listen } = await import('@tauri-apps/api/event');
        const handles = await Promise.all([
          listen('prefs:changed', () => void refresh()),
          listen<{ sessionId?: string; state: string; warming?: boolean; translation?: boolean }>(
            'capsule:state',
            ({ payload }) => {
              const key = `${payload.sessionId}:${payload.state}:${payload.warming}:${payload.translation}`;
              if (key !== lastRecording) {
                lastRecording = key;
                void refresh();
              }
            },
          ),
        ]);
        if (disposed) {
          handles.forEach((stop) => stop());
          return;
        }
        unlisten = handles;
      }
      await refresh();
    })().catch(() => {
      if (!disposed) setSnapshot(null);
    });
    return () => {
      disposed = true;
      revision.current++;
      unlisten.forEach((stop) => stop());
    };
  }, [visible]);

  const target = nativeLanguageName(snapshot?.targetLanguage ?? savedTarget);
  const languages = useMemo(
    () => localizedLanguages(i18n.language, [target]),
    [i18n.language, target],
  );
  const selected = languages.find((language) => language.nativeName === target);
  const options = languages.filter(
    (language) =>
      language.nativeName === target ||
      !isTranslationTargetRedundant(language.nativeName, snapshot?.workingLanguages ?? []),
  );
  const openMenu = async (next: boolean) => {
    const epoch = ++menuEpoch.current;
    if (closeTimer.current) clearTimeout(closeTimer.current);
    if (!next) {
      setMenuOpen(false);
      const owner = menuOwner.current;
      if (owner)
        closeTimer.current = setTimeout(
          () => {
            if (epoch !== menuEpoch.current) return;
            menuOwner.current = null;
            void setCapsuleLanguageMenu(false, owner).catch(() => {});
          },
          reduced ? 0 : 160,
        );
      return;
    }
    const active = current.current;
    if (!shown.current || !active?.mutable) return;
    pickerSession.current = active.sessionId;
    menuOwner.current = active.sessionId;
    try {
      await setCapsuleLanguageMenu(true, active.sessionId);
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      );
      if (
        epoch === menuEpoch.current &&
        shown.current &&
        current.current?.sessionId === active.sessionId &&
        current.current.mutable
      ) {
        setMenuOpen(true);
      } else {
        await setCapsuleLanguageMenu(false, active.sessionId);
      }
    } catch {
      if (epoch === menuEpoch.current) setError(true);
    }
  };
  useEffect(() => {
    if (
      menuOwner.current &&
      (!visible || !snapshot?.mutable || snapshot.sessionId !== menuOwner.current)
    )
      void openMenu(false);
  }, [visible, snapshot?.mutable, snapshot?.sessionId]);
  useEffect(
    () => () => {
      menuEpoch.current++;
      if (closeTimer.current) clearTimeout(closeTimer.current);
      if (menuOwner.current) void setCapsuleLanguageMenu(false, menuOwner.current).catch(() => {});
    },
    [],
  );
  useEffect(() => {
    const closeOnBlur = () => {
      if (menuOwner.current) void openMenu(false);
    };
    window.addEventListener('blur', closeOnBlur);
    return () => window.removeEventListener('blur', closeOnBlur);
  }, []);
  const change = async (value: string) => {
    const sessionId = pickerSession.current ?? snapshot?.sessionId;
    pickerSession.current = null;
    void openMenu(false);
    if (!sessionId || pendingChange.current || !snapshot?.mutable) return;
    const request = Symbol('translation-target');
    pendingChange.current = request;
    setBusy(true);
    setError(false);
    revision.current++;
    try {
      const updated = await selectDictationTranslationTarget(sessionId, value);
      // A native dropdown may outlive the recording which opened it.
      if (current.current?.sessionId === updated.sessionId) {
        setSnapshot(updated);
        setSavedTarget(updated.targetLanguage);
      }
    } catch {
      if (current.current?.sessionId === sessionId) setError(true);
    } finally {
      if (pendingChange.current === request) {
        pendingChange.current = null;
        setBusy(false);
      }
    }
  };

  return (
    <motion.div
      className="ol-classic-translation"
      data-visible={visible}
      aria-hidden={!visible}
      initial={reduced ? false : { opacity: 0, y: 7, scale: 0.94 }}
      animate={{ opacity: visible ? 1 : 0, y: visible ? 0 : 7, scale: visible ? 1 : 0.94 }}
      transition={
        reduced
          ? { duration: 0 }
          : { duration: visible ? 0.2 : 0.12, delay: visible ? 0.04 : 0, ease: [0.22, 1, 0.36, 1] }
      }
    >
      <span>{t('capsule.translateTo')}</span>
      <div className="ol-translation-language" data-busy={busy}>
        <SelectLite
          value={target}
          options={options.map((language) => ({
            value: language.nativeName,
            label: language.displayName,
          }))}
          placeholder={selected?.displayName || target || '…'}
          ariaLabel={t('translation.target.title')}
          disabled={!visible || busy || !snapshot?.mutable}
          open={menuOpen}
          onOpenChange={(next) => void openMenu(next)}
          onChange={(value) => void change(value)}
          side="top"
          popupStyle={{ width: 240 }}
        />
      </div>
      {error && (
        <span className="ol-translation-error" role="alert">
          {t('capsule.translationChangeFailed')}
        </span>
      )}
    </motion.div>
  );
}
