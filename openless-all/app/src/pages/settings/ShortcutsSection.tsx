import { useDeferredValue, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ShortcutRecorder } from '../../components/ShortcutRecorder';
import { Kbd } from '../../components/Kbd';
import { SelectLite } from '../../components/ui/SelectLite';
import { PageSkeleton } from '../../components/ui/PageSkeleton';
import { detectOS } from '../../components/WindowChrome';
import {
  defaultDictationHotkey,
  defaultLessComputerShortcut,
  defaultOpenAppShortcut,
  defaultQaShortcut,
  defaultSelectionPolishShortcut,
  defaultSwitchStyleShortcut,
  formatComboLabel,
  hotkeyModeSuffix,
} from '../../lib/hotkey';
import {
  listStylePacks,
  setDictationHotkey,
  setOpenAppHotkey,
  setQaHotkey,
  setQuickNoteHotkey,
  setSelectionPolishHotkey,
  setStylePackHotkeys,
  setSwitchStyleHotkey,
  setTranslationHotkey,
} from '../../lib/ipc';
import { getStylePackPresentation } from '../../lib/stylePackPresentation';
import type { ShortcutBinding, StylePack, StylePackHotkey } from '../../lib/types';
import { useHotkeySettings } from '../../state/HotkeySettingsContext';

export function ShortcutsSection() {
  const { t } = useTranslation();
  const os = detectOS();
  const { prefs, hotkey, refresh, updatePrefs } = useHotkeySettings();
  const [packs, setPacks] = useState<StylePack[]>([]);
  const [query, setQuery] = useState('');
  const search = useDeferredValue(query.trim().toLocaleLowerCase());
  const [category, setCategory] = useState('all');
  const [status, setStatus] = useState('all');
  const [draftOpen, setDraftOpen] = useState(false);
  const [draftPackId, setDraftPackId] = useState('');
  const [error, setError] = useState('');
  const tr = (key: string) => t(`uiRefinement.${key}`);
  useEffect(() => {
    let cancelled = false;
    void listStylePacks()
      .then((items) => {
        if (!cancelled) setPacks(items);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);
  if (!prefs || !hotkey) return <PageSkeleton rows={6} />;

  type Command = {
    id: string;
    label: string;
    desc?: string;
    group: string;
    binding: ShortcutBinding | null;
    save: (binding: ShortcutBinding) => Promise<void>;
    disable?: () => Promise<void>;
    reset?: () => Promise<void>;
  };
  const commands: Command[] = [
    {
      id: 'dictation',
      label: t('settings.shortcuts.startStop'),
      desc: hotkeyModeSuffix(hotkey.mode),
      group: 'recording',
      binding: prefs.dictationHotkey,
      save: async (binding) => {
        await setDictationHotkey(binding);
        await refresh();
      },
      disable: async () => {
        await setDictationHotkey(null);
        await refresh();
      },
      reset: async () => {
        await setDictationHotkey(defaultDictationHotkey());
        await refresh();
      },
    },
    {
      id: 'translation',
      label: t('translation.title'),
      group: 'recording',
      binding: prefs.translationHotkey,
      save: async (binding) => {
        await setTranslationHotkey(binding);
        await updatePrefs((current) => ({ ...current, translationHotkey: binding }));
      },
      disable: async () => {
        await setTranslationHotkey(null);
        await refresh();
      },
    },
    {
      id: 'qa',
      label: t('selectionAsk.title'),
      group: 'recording',
      binding: prefs.qaHotkey,
      save: async (binding) => {
        await setQaHotkey(binding);
        await updatePrefs((current) => ({ ...current, qaHotkey: binding }));
      },
      disable: async () => {
        await setQaHotkey(null);
        await updatePrefs((current) => ({ ...current, qaHotkey: null }));
      },
      reset: async () => {
        const binding = defaultQaShortcut();
        await setQaHotkey(binding);
        await updatePrefs((current) => ({ ...current, qaHotkey: binding }));
      },
    },
    {
      id: 'note',
      label: t('settings.shortcuts.quickNote'),
      group: 'recording',
      binding: prefs.quickNoteHotkey,
      save: async (binding) => {
        await setQuickNoteHotkey(binding);
        await updatePrefs((current) => ({ ...current, quickNoteHotkey: binding }));
      },
      disable: async () => {
        await setQuickNoteHotkey(null);
        await updatePrefs((current) => ({ ...current, quickNoteHotkey: null }));
      },
    },
    {
      id: 'selection',
      label: t('settings.selectionWorkspace.polishHotkey'),
      group: 'recording',
      binding: prefs.selectionPolishHotkey,
      save: async (binding) => {
        await setSelectionPolishHotkey(binding);
        await refresh();
      },
      disable: async () => {
        await setSelectionPolishHotkey(null);
        await refresh();
      },
      reset: async () => {
        await setSelectionPolishHotkey(defaultSelectionPolishShortcut());
        await refresh();
      },
    },
    {
      id: 'style',
      label: t('settings.shortcuts.switchStyle'),
      group: 'styles',
      binding: prefs.switchStyleHotkey,
      save: async (binding) => {
        await setSwitchStyleHotkey(binding);
        await updatePrefs((current) => ({ ...current, switchStyleHotkey: binding }));
      },
      disable: async () => {
        await setSwitchStyleHotkey(null);
        await updatePrefs((current) => ({ ...current, switchStyleHotkey: null }));
      },
      reset: async () => {
        const binding = defaultSwitchStyleShortcut();
        await setSwitchStyleHotkey(binding);
        await updatePrefs((current) => ({ ...current, switchStyleHotkey: binding }));
      },
    },
    {
      id: 'open',
      label: t('settings.shortcuts.openApp'),
      group: 'system',
      binding: prefs.openAppHotkey,
      save: async (binding) => {
        await setOpenAppHotkey(binding);
        await updatePrefs((current) => ({ ...current, openAppHotkey: binding }));
      },
      disable: async () => {
        await setOpenAppHotkey(null);
        await updatePrefs((current) => ({ ...current, openAppHotkey: null }));
      },
      reset: async () => {
        const binding = defaultOpenAppShortcut();
        await setOpenAppHotkey(binding);
        await updatePrefs((current) => ({ ...current, openAppHotkey: binding }));
      },
    },
  ];
  if (prefs.testingFeaturesEnabled && (os === 'mac' || os === 'win'))
    commands.push({
      id: 'computer',
      label: t('settings.codingAgent.title'),
      group: 'system',
      binding: prefs.codingAgentVoiceHotkey,
      save: async (binding) => {
        await updatePrefs((current) => ({
          ...current,
          codingAgentEnabled: true,
          codingAgentVoiceHotkey: binding,
        }));
      },
      disable: async () => {
        await updatePrefs((current) => ({ ...current, codingAgentVoiceHotkey: null }));
      },
      reset: async () => {
        await updatePrefs((current) => ({
          ...current,
          codingAgentEnabled: true,
          codingAgentVoiceHotkey: defaultLessComputerShortcut(),
        }));
      },
    });
  const matches = (label: string, binding: ShortcutBinding | null) =>
    (!search ||
      `${label} ${binding?.primary.trim() ? `${formatComboLabel(binding)} ${binding.modifiers.join(' ')} ${binding.primary}` : tr('unassigned')}`
        .toLocaleLowerCase()
        .includes(search)) &&
    (status === 'all' || (status === 'assigned') === Boolean(binding?.primary.trim()));
  const visible = commands.filter(
    (command) =>
      (category === 'all' || category === command.group) && matches(command.label, command.binding),
  );
  const packHotkeys = prefs.stylePackHotkeys ?? [];
  const savePacks = async (next: StylePackHotkey[]) => {
    setError('');
    try {
      await setStylePackHotkeys(next);
      await updatePrefs((current) => ({ ...current, stylePackHotkeys: next }));
    } catch (reason) {
      setError(String(reason));
      throw reason;
    }
  };
  const packName = (id: string) => {
    const pack = packs.find((item) => item.id === id);
    return pack ? getStylePackPresentation(pack, t).name : id;
  };
  const packOptions = (id: string) =>
    packs.map((pack) => ({
      value: pack.id,
      label: getStylePackPresentation(pack, t).name,
      disabled: pack.id !== id && packHotkeys.some((item) => item.packId === pack.id),
    }));
  const showPacks = category === 'all' || category === 'styles';
  const visiblePacks = showPacks
    ? packHotkeys.filter((entry) => matches(packName(entry.packId), entry.binding))
    : [];
  const readonly = [
    { label: t('settings.shortcuts.cancel'), key: 'Esc' },
    ...(os === 'mac'
      ? [{ label: t('settings.shortcuts.confirm'), key: t('settings.shortcuts.confirmHint') }]
      : []),
  ].filter(
    (row) =>
      (category === 'all' || category === 'system') &&
      status !== 'unassigned' &&
      (!search || `${row.label} ${row.key}`.toLocaleLowerCase().includes(search)),
  );
  const count = visible.length + visiblePacks.length + readonly.length;

  return (
    <section className="ol-shortcuts" aria-label={t('settings.shortcuts.title')}>
      <div className="ol-shortcuts-toolbar">
        <input
          type="search"
          className="ol-settings-search-field"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          aria-label={tr('shortcutsSearch')}
          placeholder={tr('shortcutsSearch')}
        />
        <span aria-live="polite" className="ol-shortcuts-count">
          {count} / {commands.length + packHotkeys.length + (os === 'mac' ? 2 : 1)}
        </span>
      </div>
      <div className="ol-shortcuts-layout">
        <aside className="ol-shortcuts-filters">
          <nav aria-label={tr('category')}>
            <h3>{tr('category')}</h3>
            {['all', 'recording', 'styles', 'system'].map((id) => (
              <button
                type="button"
                key={id}
                aria-pressed={category === id}
                onClick={() => setCategory(id)}
              >
                {tr(id)}
              </button>
            ))}
          </nav>
          <nav aria-label={tr('status')}>
            <h3>{tr('status')}</h3>
            {['all', 'assigned', 'unassigned'].map((id) => (
              <button
                type="button"
                key={id}
                aria-pressed={status === id}
                onClick={() => setStatus(id)}
              >
                {tr(id)}
              </button>
            ))}
          </nav>
        </aside>
        <div className="ol-shortcut-results">
          <p className="ol-section-note">{tr('shortcutHint')}</p>
          {visible.map((command) => (
            <div
              className="ol-shortcut-row"
              role="group"
              aria-label={command.label}
              key={command.id}
            >
              <div>
                <span>{command.label}</span>
                {command.desc && <small>{command.desc}</small>}
              </div>
              <ShortcutRecorder
                value={command.binding}
                onSave={command.save}
                onDisable={command.disable}
                onReset={command.reset}
                sideSpecificModifiers={command.id === 'dictation'}
              />
            </div>
          ))}
          {readonly.map((row) => (
            <div className="ol-shortcut-row" key={row.label}>
              <span>{row.label}</span>
              <Kbd>{row.key}</Kbd>
            </div>
          ))}
          {visiblePacks.map((entry) => (
            <div className="ol-shortcut-row ol-shortcut-pack" key={entry.packId}>
              <SelectLite
                value={entry.packId}
                options={packOptions(entry.packId)}
                ariaLabel={t('settings.shortcuts.stylePackSelect')}
                onChange={(packId) =>
                  void savePacks(
                    packHotkeys.map((item) =>
                      item.packId === entry.packId ? { ...item, packId } : item,
                    ),
                  ).catch(() => {})
                }
              />
              <ShortcutRecorder
                value={entry.binding}
                onDisable={() =>
                  savePacks(packHotkeys.filter((item) => item.packId !== entry.packId))
                }
                onSave={(binding) =>
                  savePacks(
                    packHotkeys.map((item) =>
                      item.packId === entry.packId ? { ...item, binding } : item,
                    ),
                  )
                }
              />
              <button
                type="button"
                className="ol-shortcut-remove"
                aria-label={t('settings.shortcuts.stylePackRemove')}
                onClick={() =>
                  void savePacks(packHotkeys.filter((item) => item.packId !== entry.packId)).catch(
                    () => {},
                  )
                }
              >
                ×
              </button>
            </div>
          ))}
          {!count && (
            <p role="status" className="ol-shortcut-empty">
              {tr('noResults')}
            </p>
          )}
          {showPacks && !search && (
            <div className="ol-shortcut-pack-add">
              {draftOpen ? (
                <div className="ol-shortcut-row ol-shortcut-pack">
                  <SelectLite
                    value={draftPackId}
                    options={packOptions(draftPackId)}
                    onChange={setDraftPackId}
                    placeholder={t('settings.shortcuts.stylePackSelect')}
                    ariaLabel={t('settings.shortcuts.stylePackSelect')}
                  />
                  <ShortcutRecorder
                    value={null}
                    disabled={!draftPackId}
                    onSave={async (binding) => {
                      await savePacks([...packHotkeys, { packId: draftPackId, binding }]);
                      setDraftOpen(false);
                      setDraftPackId('');
                    }}
                  />
                  <button
                    type="button"
                    onClick={() => setDraftOpen(false)}
                    aria-label={t('common.cancel')}
                  >
                    ×
                  </button>
                </div>
              ) : (
                <button className="ol-text-action" type="button" onClick={() => setDraftOpen(true)}>
                  + {t('settings.shortcuts.stylePackAdd')}
                </button>
              )}
            </div>
          )}
          {error && <p role="alert">{error}</p>}
        </div>
      </div>
    </section>
  );
}
