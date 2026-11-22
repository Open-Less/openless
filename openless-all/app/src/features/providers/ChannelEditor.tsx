import { useContext, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import {
  deleteChannel,
  readCredential,
  renameChannel,
  setChannelProviderType,
  setCredential,
  type Channel,
} from '../../api/providers';
import { Icon } from '../../components/Icon';
import { Dialog } from '../../ui/overlays/Dialog';
import { useInteraction } from '../../ui/overlays/InteractionProvider';
import { SelectLite } from '../../ui/controls/SelectLite';
import { useOverlayMotion } from '../../lib/motion';
import { emitSaved } from '../../lib/savedEvent';
import { Button as Btn } from '../../ui/controls/Button';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { ChannelEditorHostContext } from '../settings/shell/ChannelEditorHostContext';
import { ProviderFormContext, useProviderForm } from './ProviderForm';
import { ChannelCredentialFields, ChannelFormRow, ChannelSectionHeading } from './ProvidersSection';

import {
  defaultChannelNameForProvider,
  type ChannelKind,
  type PresetOption,
} from './channelCatalog';

/**
 * Add and edit share the same credential form inside a bounded modal or its setup host.
 * Creating a channel first obtains the ID the credentials belong to; on close only blank drafts that never saw user action are recycled.
 */
export function ChannelEditor({
  kind,
  channel,
  presets,
  isDraft,
  mobile,
  closing = false,
  onClose,
  onChanged,
  onUserMutation,
}: {
  kind: ChannelKind;
  channel: Channel;
  presets: PresetOption[];
  /** Draft card in the create flow: title says "add channel"; recyclable while untouched. */
  isDraft: boolean;
  mobile: boolean;
  /** Keeps the editor mounted until its exit finishes. */
  closing?: boolean;
  onClose: () => void;
  onChanged: () => void | Promise<void>;
  /** The user made a meaningful change to the draft; must fire synchronously before async writes. */
  onUserMutation: () => void;
}) {
  const { t } = useTranslation();
  // Drain the channel form's unwritten async writes before closing / switching provider (#1044 semantics).
  const form = useProviderForm();
  const [name, setName] = useState(channel.name);
  const [providerType, setProviderType] = useState(channel.providerType);
  const [providerChangeError, setProviderChangeError] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const deleteRequest = useRef(false);
  const interaction = useInteraction(`${channel.id}:${closing}`);
  const dialogRef = useRef<HTMLDivElement>(null);
  const nameId = useId();
  const headingId = useId();
  const editorHost = useContext(ChannelEditorHostContext);
  const embedded = Boolean(
    editorHost?.presentation === 'inline' && editorHost.container && editorHost.background,
  );
  useOverlayMotion(dialogRef, closing, 'drawer', embedded);
  const openerRef = useRef(document.activeElement);
  const initialFocus = {
    get current() {
      return (
        dialogRef.current?.querySelector<HTMLElement>('[role="combobox"], input:not([disabled])') ??
        null
      );
    },
  };
  const closeRequestedRef = useRef(false);
  const lifetime = useRef<symbol | null>(null);
  const pendingSwitch = useRef<{ closeInstead: boolean } | null>(null);
  useLayoutEffect(() => {
    lifetime.current = closing ? null : Symbol();
    return () => {
      lifetime.current = null;
    };
  }, [channel.id, closing]);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const closeEditor = () => {
    if (!lifetime.current || closeRequestedRef.current) return false;
    closeRequestedRef.current = true;
    onCloseRef.current();
    return true;
  };
  const requestClose = () => {
    if (!lifetime.current || closeRequestedRef.current) return;
    if (pendingSwitch.current) {
      pendingSwitch.current.closeInstead = true;
      return;
    }
    void form.finish(closeEditor);
  };

  useEffect(() => {
    const opener = openerRef.current;
    const background = embedded ? editorHost?.background : null;
    if (embedded) {
      (initialFocus.current ?? dialogRef.current)?.focus({ preventScroll: true });
      if (background) background.inert = true;
    }
    editorHost?.registerClose(requestClose);
    return () => {
      editorHost?.registerClose(null);
      if (background) background.inert = false;
      if (embedded && opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, []);

  const onDialogKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const dialog = dialogRef.current;
    if (!dialog || event.defaultPrevented) return;
    if (event.target instanceof Element && event.target.closest('[role="dialog"]') !== dialog)
      return;
    if (event.key === 'Escape') {
      // SelectLite handles its open menu first. Never close both layers at once.
      if (dialog.querySelector('[role="combobox"][aria-expanded="true"]')) return;
      event.preventDefault();
      event.stopPropagation();
      requestClose();
    } else if (event.key === 'Tab') {
      const controls = Array.from(
        dialog.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), a[href], [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => element.tabIndex >= 0 && element.getClientRects().length > 0);
      const first = controls[0];
      const last = controls[controls.length - 1];
      if (!first) {
        event.preventDefault();
        dialog.focus();
      } else if (
        event.shiftKey &&
        (document.activeElement === first || document.activeElement === dialog)
      ) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }
  };

  const saveName = async () => {
    if (name.trim() === channel.name.trim()) return;
    try {
      await renameChannel(kind, channel.id, name.trim());
      await onChanged();
    } catch (error) {
      console.error('[channels] rename failed', error);
      emitSaved('failed', t('common.operationFailed'));
    }
  };

  // After switching providers, write the preset's default endpoint / model into EMPTY slots (never overwrite
  // user-entered custom values). Channels are isolated per card, so this reads/writes by card id; Codex OAuth /
  // local engines / custom OpenAI-compatible (empty baseUrl/model) naturally skip. Failures are logged only.
  const fillProviderDefaults = async (next: string, isCurrent: () => boolean) => {
    try {
      const preset = presets.find((item) => item.id === next);
      if (!preset || !isCurrent()) return;
      const endpointAccount = kind === 'llm' ? 'ark.endpoint' : 'asr.endpoint';
      const modelAccount = kind === 'llm' ? 'ark.model_id' : 'asr.model';
      if (next === 'orcarouter') {
        if (preset.defaultEndpoint)
          await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
        if (preset.defaultModel && isCurrent())
          await setCredential(modelAccount, preset.defaultModel, channel.id);
        return;
      }
      if (
        preset.defaultEndpoint &&
        !(await readCredential(endpointAccount, channel.id))?.trim() &&
        isCurrent()
      ) {
        await setCredential(endpointAccount, preset.defaultEndpoint, channel.id);
      }
      if (
        preset.defaultModel &&
        !(await readCredential(modelAccount, channel.id))?.trim() &&
        isCurrent()
      ) {
        await setCredential(modelAccount, preset.defaultModel, channel.id);
      }
    } catch (error) {
      console.error('[channels] failed to fill provider defaults', error);
    }
  };

  const changeProvider = async (next: string) => {
    const owner = lifetime.current;
    if (!owner || closeRequestedRef.current || pendingSwitch.current || next === providerType)
      return;
    const request = { closeInstead: false };
    pendingSwitch.current = request;
    const isCurrent = () =>
      lifetime.current === owner && pendingSwitch.current === request && !closeRequestedRef.current;
    setProviderChangeError(false);
    onUserMutation();
    try {
      const finished = await form.finish(async () => {
        if (!isCurrent()) return false;
        if (request.closeInstead) return closeEditor();
        await setChannelProviderType(kind, channel.id, next);
        if (!isCurrent()) return false;
        await fillProviderDefaults(next, isCurrent);
        if (!isCurrent()) return false;
        // The provider is durable now. A later rename or list refresh cannot undo it.
        setProviderType(next);
        try {
          const defaultName = defaultChannelNameForProvider(next, name);
          if (defaultName !== name) {
            await renameChannel(kind, channel.id, defaultName);
            if (isCurrent()) setName(defaultName);
          }
        } finally {
          if (isCurrent()) await onChanged();
        }
        if (isCurrent() && request.closeInstead) closeEditor();
        return true;
      });
      if (!finished && isCurrent()) setProviderChangeError(true);
    } catch (error) {
      console.error('[channels] change provider failed', error);
      if (isCurrent()) setProviderChangeError(true);
    } finally {
      if (pendingSwitch.current === request) pendingSwitch.current = null;
    }
  };

  const remove = async () => {
    if (deleteRequest.current || closing || form.leaving || !lifetime.current) return;
    const owner = lifetime.current;
    deleteRequest.current = true;
    setDeleting(true);
    try {
      const confirmed = await interaction.confirm({
        title: t('settings.channels.delete'),
        description: t('settings.channels.deleteScope', { name: channel.name }),
        confirmLabel: t('settings.channels.confirmDelete'),
        cancelLabel: t('common.cancel'),
        tone: 'danger',
        icon: 'trash',
        errorMessage: t('common.operationFailed'),
        onConfirm: async () => {
          if (lifetime.current !== owner) throw new Error('Channel editor no longer active');
          const finished = await form.finish(async () => {
            if (lifetime.current !== owner) return false;
            await deleteChannel(kind, channel.id);
            emitSaved('deleted', t('common.deleted'));
          });
          if (!finished) throw new Error('Could not finish pending channel changes');
        },
      });
      if (confirmed && lifetime.current === owner) closeEditor();
    } finally {
      deleteRequest.current = false;
      if (lifetime.current === owner) setDeleting(false);
    }
  };

  const descriptor = presets.find((item) => item.id === providerType);
  const isLocalEngine = descriptor?.authRequirement === 'none' && providerType !== 'apple-speech';

  const content = (
    <ProviderFormContext.Provider value={form}>
      <div
        ref={dialogRef}
        className={`ol-channel-dialog${embedded ? ' ol-channel-dialog-embedded' : ''}`}
        data-closing={closing ? 'true' : undefined}
        role={embedded ? 'dialog' : undefined}
        aria-labelledby={embedded ? headingId : undefined}
        tabIndex={-1}
        onKeyDown={onDialogKeyDown}
      >
        <header className="ol-channel-dialog-header">
          {embedded ? (
            <button
              type="button"
              className="ol-settings-back"
              onClick={requestClose}
              aria-label={t('settings.channels.backToList')}
              title={t('settings.channels.backToList')}
            >
              <Icon name="chevLeft" size={19} />
            </button>
          ) : null}
          <div className="ol-channel-dialog-heading">
            <div className="ol-channel-dialog-title">
              <h2 id={headingId}>
                {t(isDraft ? 'settings.channels.createTitle' : 'settings.channels.editTitle')}
              </h2>
              <span>{t(`settings.channels.${kind}Title`)}</span>
            </div>
            <p>{t('settings.channels.autoSaveHint')}</p>
          </div>
          {!embedded && (
            <button
              type="button"
              className="ol-dialog-icon-close"
              onClick={requestClose}
              aria-label={t('common.close')}
            >
              <Icon name="close" size={18} />
            </button>
          )}
        </header>

        <div className="ol-channel-dialog-body ol-thinscroll">
          <div className="ol-channel-form-sheet">
            <ChannelSectionHeading icon="cloud" title={t('settings.channels.connectionTitle')} />
            <ChannelFormRow label={t('settings.channels.providerLabel')}>
              <SelectLite
                value={providerType}
                disabled={form.leaving}
                onChange={(next) => void changeProvider(next)}
                options={presets.map((p) => ({
                  value: p.id,
                  label: t(`settings.providers.presets.${p.nameKey}`),
                }))}
                ariaLabel={t('settings.channels.providerLabel')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
              {providerChangeError && (
                <p role="alert" className="ol-channel-change-error">
                  {t('common.operationFailed')}
                </p>
              )}
            </ChannelFormRow>
            <ChannelFormRow label={t('settings.channels.nameLabel')} htmlFor={nameId}>
              <input
                id={nameId}
                value={name}
                onChange={(e) => {
                  onUserMutation();
                  setName(e.target.value);
                }}
                onBlur={() => void saveName()}
                placeholder={t('settings.channels.namePlaceholder')}
                style={{ ...inputStyle, width: '100%', maxWidth: '100%', height: 38 }}
              />
              <p className="ol-channel-name-hint">{t('settings.channels.nameHint')}</p>
            </ChannelFormRow>

            {/* Model list, provider-specific fields, and test results all stay in the same scroll area. */}
            <ChannelCredentialFields
              key={`${channel.id}:${providerType}`}
              kind={kind}
              providerType={providerType}
              channelId={channel.id}
              descriptor={descriptor}
              onTested={() => void onChanged()}
              onUserMutation={onUserMutation}
            />
            {isLocalEngine && (
              <p className="ol-channel-local-hint">{t('settings.channels.localEngineModelHint')}</p>
            )}
          </div>
        </div>

        <footer className="ol-channel-dialog-footer">
          <button
            type="button"
            className="ol-channel-delete-button"
            aria-disabled={deleting || form.leaving}
            onClick={() => void remove()}
          >
            <Icon name="trash" size={15} />
            {t('settings.channels.delete')}
          </button>
          <div className="ol-channel-dialog-footer-end">
            <span className="ol-channel-autosave-hint">{t('modal.autoSaveHint')}</span>
            <Btn
              variant={embedded ? 'primary' : 'blue'}
              onClick={requestClose}
              disabled={form.leaving}
            >
              {t(embedded ? 'settings.channels.done' : 'common.close')}
            </Btn>
          </div>
        </footer>
      </div>
    </ProviderFormContext.Provider>
  );
  return embedded ? (
    createPortal(content, editorHost!.container!)
  ) : (
    <Dialog
      layout="terms"
      cardClassName="ol-structured-dialog--channel"
      labelledBy={headingId}
      onClose={requestClose}
      zIndex={1000}
      closing={closing}
      initialFocus={initialFocus}
      width={mobile ? '100%' : undefined}
    >
      {content}
    </Dialog>
  );
}
