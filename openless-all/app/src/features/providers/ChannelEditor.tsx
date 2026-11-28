import { TextField } from '../../ui/controls/TextField';
import {
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';
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
import { notify } from '../../lib/notifications';
import { channelEditorMessages } from '../../i18n/channelEditorMessages';
import { Button as Btn } from '../../ui/controls/Button';
import { inputStyle } from '../../ui/controls/settingsFieldStyles';
import { ChannelEditorHostContext } from '../settings/shell/ChannelEditorHostContext';
import { ProviderFormContext, useProviderForm } from './ProviderForm';
import { ChannelCredentialFields, ChannelFormRow, ChannelSectionHeading } from './ProvidersSection';
import { CredentialDraft } from './credentialDraft';

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
  const { t, i18n } = useTranslation();
  const messages = channelEditorMessages(i18n.resolvedLanguage ?? i18n.language);
  // Closing, saving and switching all drain the same registered fields.
  const form = useProviderForm();
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;
  const refreshChannel = async () => {
    try {
      await onChangedRef.current();
    } catch (error) {
      console.error('[channels] refresh failed', error);
    }
  };
  const savedName = useRef(channel.name.trim());
  // The host keys the editor by channel ID. List refreshes must not replace an active draft.
  const [nameDraft] = useState(
    () =>
      new CredentialDraft(
        async () => channel.name,
        async (value) => {
          const next = value.trim();
          if (next === savedName.current) return;
          await renameChannel(kind, channel.id, next);
          savedName.current = next;
          await refreshChannel();
        },
      ),
  );
  const name = useSyncExternalStore(nameDraft.subscribe, nameDraft.snapshot, nameDraft.snapshot);
  const { register } = form;
  useLayoutEffect(() => register('channel-name', nameDraft.flush), [nameDraft, register]);
  useEffect(() => {
    void nameDraft.load();
    return () => nameDraft.dispose();
  }, [nameDraft]);
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
  const pendingAction = useRef<{ closeAfterDrain: boolean } | null>(null);
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
    if (!lifetime.current || closeRequestedRef.current || deleteRequest.current) return;
    if (pendingAction.current) {
      pendingAction.current.closeAfterDrain = true;
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

  const saveChannel = async () => {
    const owner = lifetime.current;
    if (!owner || closeRequestedRef.current || deleteRequest.current || form.isBusy()) return;
    const request = { closeAfterDrain: false };
    pendingAction.current = request;
    try {
      const saved = await form.finish(async () => {
        if (lifetime.current !== owner) return false;
        // Explicitly saving a new channel keeps it even if all its fields are blank.
        onUserMutation();
        await refreshChannel();
      });
      if (lifetime.current !== owner) return;
      notify({
        id: `channel-save:${kind}:${channel.id}`,
        state: saved ? 'saved' : 'failed',
        message: saved ? messages.saved : messages.saveFailed,
        durationMs: saved ? undefined : 5000,
      });
      if (saved && request.closeAfterDrain) closeEditor();
    } finally {
      if (pendingAction.current === request) pendingAction.current = null;
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
    if (
      !owner ||
      closeRequestedRef.current ||
      deleteRequest.current ||
      form.isBusy() ||
      next === providerType
    )
      return;
    const request = { closeAfterDrain: false };
    pendingAction.current = request;
    const isCurrent = () =>
      lifetime.current === owner && pendingAction.current === request && !closeRequestedRef.current;
    setProviderChangeError(false);
    onUserMutation();
    try {
      const finished = await form.finish(async () => {
        if (!isCurrent()) return false;
        if (request.closeAfterDrain) return closeEditor();
        await setChannelProviderType(kind, channel.id, next);
        if (!isCurrent()) return false;
        await fillProviderDefaults(next, isCurrent);
        if (!isCurrent()) return false;
        // The provider is durable now. A later rename or list refresh cannot undo it.
        setProviderType(next);
        const currentName = nameDraft.snapshot().value;
        const defaultName = defaultChannelNameForProvider(next, currentName);
        if (defaultName !== currentName) {
          nameDraft.edit(defaultName);
          if (!(await nameDraft.flush())) {
            if (isCurrent()) await refreshChannel();
            return false;
          }
        } else if (isCurrent()) {
          await refreshChannel();
        }
        if (isCurrent() && request.closeAfterDrain) closeEditor();
        return true;
      });
      if (!finished && isCurrent()) setProviderChangeError(true);
    } catch (error) {
      console.error('[channels] change provider failed', error);
      if (isCurrent()) setProviderChangeError(true);
    } finally {
      if (pendingAction.current === request) pendingAction.current = null;
    }
  };

  const remove = async () => {
    if (deleteRequest.current || closeRequestedRef.current || form.isBusy() || !lifetime.current)
      return;
    const owner = lifetime.current;
    deleteRequest.current = true;
    setDeleting(true);
    try {
      const confirmed = await interaction.confirm({
        title: t('settings.channels.delete'),
        description: t('settings.channels.deleteScope', {
          name: nameDraft.snapshot().value.trim(),
        }),
        confirmLabel: t('settings.channels.confirmDelete'),
        cancelLabel: t('common.cancel'),
        tone: 'danger',
        icon: 'trash',
        errorMessage: messages.deleteFailed,
        onConfirm: async () => {
          // Window-level feedback must survive the editor closing after a successful delete.
          const id = `channel-delete:${kind}:${channel.id}`;
          try {
            const finished = await form.finish(async () => {
              if (lifetime.current !== owner) return false;
              notify({ id, state: 'deleting', message: messages.deleting });
              await deleteChannel(kind, channel.id);
              notify({ id, state: 'deleted', message: t('common.deleted') });
            });
            if (!finished) throw new Error('Could not finish pending channel changes');
          } catch (error) {
            notify({
              id,
              state: 'failed',
              severity: 'warning',
              message: messages.deleteFailed,
              durationMs: 5000,
            });
            throw error;
          }
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
              <TextField
                id={nameId}
                value={name.value}
                onChange={(e) => {
                  if (form.isBusy()) return;
                  onUserMutation();
                  nameDraft.edit(e.target.value);
                }}
                onBlur={() => void nameDraft.flush()}
                disabled={!name.loaded || form.leaving}
                placeholder={t('settings.channels.namePlaceholder')}
                style={{ height: 38 }}
              />
              <p className="ol-channel-name-hint">{t('settings.channels.nameHint')}</p>
              {name.status === 'saveError' && (
                <p role="alert" className="ol-channel-change-error">
                  {messages.saveFailed}
                </p>
              )}
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
              onClick={embedded ? requestClose : () => void saveChannel()}
              disabled={form.leaving || deleting}
            >
              {embedded ? t('settings.channels.done') : messages.save}
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
