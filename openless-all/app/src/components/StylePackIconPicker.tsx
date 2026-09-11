import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Icon } from './Icon';
import { readStylePackIcon, setStylePackIcon } from '../lib/ipc';
import { isStyleIconDataUrl, rasterizeStyleSvg } from '../lib/stylePackIcon';
import type { StylePack } from '../lib/types';
import { getStylePackPresentation } from '../lib/stylePackPresentation';

const DEFAULT_ICONS = { raw: 'mic', light: 'feather', structured: 'layout', formal: 'doc' };

export function StylePackIconPicker({
  pack,
  onSaved,
  onStatus,
}: {
  pack: StylePack;
  onSaved: (pack: StylePack) => void;
  onStatus: (failed: boolean, message: string) => void;
}) {
  const { t } = useTranslation();
  const { name } = getStylePackPresentation(pack, t);
  const [src, setSrc] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    let cancelled = false;
    void readStylePackIcon(pack.id)
      .then((value) => {
        if (!cancelled) setSrc(isStyleIconDataUrl(value) ? value : null);
      })
      .catch(() => {
        if (!cancelled) setSrc(null);
      });
    return () => {
      cancelled = true;
    };
  }, [pack.id, pack.iconPath, pack.updatedAt]);

  const save = async (file: File | null) => {
    setBusy(true);
    try {
      const png = file ? await rasterizeStyleSvg(file) : null;
      const saved = await setStylePackIcon(pack.id, png);
      const value = await readStylePackIcon(pack.id);
      setSrc(isStyleIconDataUrl(value) ? value : null);
      onSaved(saved);
      onStatus(false, t('style.pack.iconSaved'));
    } catch (error) {
      onStatus(
        true,
        t(
          error instanceof Error && error.message === 'invalidSvg'
            ? 'style.pack.iconInvalid'
            : 'style.pack.iconSaveFailed',
        ),
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <span className="ol-style-icon-picker">
      <button
        type="button"
        className="ol-style-icon-button"
        data-custom={src ? 'true' : undefined}
        onClick={() => inputRef.current?.click()}
        disabled={busy}
        aria-label={t('style.pack.uploadIcon', { name })}
        title={t('style.pack.uploadIcon', { name })}
      >
        {src ? (
          <img src={src} width={24} height={24} alt="" onError={() => setSrc(null)} />
        ) : (
          <Icon name={DEFAULT_ICONS[pack.baseMode]} size={21} />
        )}
        <span className="ol-style-icon-edit">
          <Icon name="pencil" size={9} />
        </span>
      </button>
      <input
        ref={inputRef}
        type="file"
        accept=".svg,image/svg+xml"
        hidden
        disabled={busy}
        onChange={(event) => {
          const file = event.target.files?.[0];
          event.target.value = '';
          if (file) void save(file);
        }}
      />
      {src && (
        <button
          type="button"
          className="ol-style-icon-reset"
          onClick={() => void save(null)}
          disabled={busy}
          aria-label={t('style.pack.resetIcon')}
          title={t('style.pack.resetIcon')}
        >
          <Icon name="close" size={10} />
        </button>
      )}
    </span>
  );
}
