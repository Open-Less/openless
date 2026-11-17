import { motion, useReducedMotion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SelectLite } from '../../ui/controls/SelectLite';
import '../../ui/styles/select.css';
import { getSettings } from '../../api/preferences';
import { isTauriHost } from '../../runtime/environment';
import {
  createCapsuleSubscriptions,
  getDictationTranslationTarget,
  selectDictationTranslationTarget,
  type DictationTranslationTarget,
} from '../../api/dictation';
import { localizedLanguages, nativeLanguageName } from '../../lib/languageCatalog';
import { isTranslationTargetRedundant } from '../../lib/translationTarget';
import { useCapsuleLanguageMenu } from './useCapsuleLanguageMenu';
import './CapsuleStyles.css';

/** The same Base UI menu as settings, with temporary native headroom above the capsule. */
export function CapsuleTranslationBadge({ visible = true }: { visible?: boolean }) {
  const { t, i18n } = useTranslation();
  const reduced = useReducedMotion();
  const [snapshot, setSnapshot] = useState<DictationTranslationTarget | null>(null);
  const [savedTarget, setSavedTarget] = useState('');
  const [busy, setBusy] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const [error, setError] = useState(false);
  const revision = useRef(0);
  const pendingChange = useRef<symbol | null>(null);
  const current = useRef(snapshot);
  current.current = snapshot;

  useEffect(() => {
    let disposed = false;
    const events = createCapsuleSubscriptions();
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
      pendingChange.current = null;
      return;
    }
    void (async () => {
      if (isTauriHost()) {
        if (!(await events.preferences(() => void refresh()))) return;
        if (
          !(await events.state((payload) => {
            const key = `${payload.sessionId}:${payload.state}:${payload.warming}:${payload.translation}`;
            if (key !== lastRecording) {
              lastRecording = key;
              void refresh();
            }
          }))
        )
          return;
      }
      await refresh();
    })().catch(() => {
      if (!disposed) setSnapshot(null);
    });
    return () => {
      disposed = true;
      revision.current++;
      events.dispose();
      pendingChange.current = null;
    };
  }, [visible]);

  const target = nativeLanguageName(snapshot?.targetLanguage ?? savedTarget);
  const languages = useMemo(
    () => localizedLanguages(i18n.language, [target]),
    [i18n.language, target],
  );
  const selected = languages.find((language) => language.nativeName === target);
  const options = useMemo(
    () =>
      languages
        .filter(
          (language) =>
            language.nativeName === target ||
            !isTranslationTargetRedundant(language.nativeName, snapshot?.workingLanguages ?? []),
        )
        .map((language) => ({ value: language.nativeName, label: language.displayName })),
    [languages, target, snapshot?.workingLanguages],
  );
  useEffect(() => {
    pendingChange.current = null;
    setBusy(false);
    setError(false);
  }, [snapshot?.sessionId]);

  const menu = useCapsuleLanguageMenu({
    anchor,
    target: snapshot,
    visible,
    onFailure: () => setError(true),
  });
  const change = async (value: string) => {
    const sessionId = menu.sessionForChoice();
    menu.close();
    if (
      !sessionId ||
      pendingChange.current ||
      !snapshot?.mutable ||
      snapshot.sessionId !== sessionId
    )
      return;
    const request = Symbol('translation-target');
    pendingChange.current = request;
    setBusy(true);
    setError(false);
    revision.current++;
    try {
      const updated = await selectDictationTranslationTarget(sessionId, value);
      // A native dropdown may outlive the recording which opened it.
      if (pendingChange.current === request && current.current?.sessionId === updated.sessionId) {
        setSnapshot(updated);
        setSavedTarget(updated.targetLanguage);
      }
    } catch {
      if (pendingChange.current === request && current.current?.sessionId === sessionId)
        setError(true);
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
      <div ref={anchor} className="ol-translation-language" data-busy={busy}>
        <SelectLite
          value={target}
          options={options}
          placeholder={selected?.displayName || target || '…'}
          ariaLabel={t('translation.target.title')}
          disabled={!visible || busy || !snapshot?.mutable}
          open={menu.open}
          onOpenChange={menu.onOpenChange}
          onOpenChangeComplete={menu.onOpenChangeComplete}
          onChange={(value) => void change(value)}
          side="top"
          popupStyle={{ width: 240 }}
          popupClassName="ol-capsule-language-popup"
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
