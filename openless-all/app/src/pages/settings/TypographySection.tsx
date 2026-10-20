import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Select } from '../../components/ui/Select';
import { Card } from '../_atoms';
import { SectionTitle, SettingRow } from './shared';
import {
  FONT_CATALOG,
  readTypography,
  saveTypography,
  type FontId,
  type Typography,
} from '../../lib/typography';
import { readFontScale, setFontScale, type FontScaleId } from '../../lib/fontScale';

export function TypographySection() {
  const { t } = useTranslation();
  const [value, setValue] = useState(readTypography);
  const [scale, setScale] = useState(readFontScale);
  const [error, setError] = useState('');
  const tr = (key: string) => t(`uiRefinement.${key}`);
  const apply = (next: Typography) => {
    setValue(next);
    try {
      saveTypography(next);
      setError('');
    } catch {
      setError(tr('storageError'));
    }
  };
  return (
    <Card className="ol-typography">
      <SectionTitle>{tr('typography')}</SectionTitle>
      <SettingRow label={tr('family')} desc={tr('fontHint')}>
        <Select
          value={value.fonts.ui}
          ariaLabel={tr('family')}
          onChange={(next) => {
            const font = next as FontId;
            // One font choice for interface and reading; code keeps its monospace setting.
            apply({
              ...value,
              fonts: { ...value.fonts, ui: font, body: font, title: font, numbers: font },
            });
          }}
          options={FONT_CATALOG.map((font) => ({
            value: font.id,
            label: font.id === 'system' ? tr('native') : font.name,
          }))}
        />
      </SettingRow>
      <SettingRow label={tr('fontSize')}>
        <Select
          value={scale}
          ariaLabel={tr('fontSize')}
          onChange={(next) => {
            setScale(next as FontScaleId);
            setFontScale(next as FontScaleId);
          }}
          options={[
            { value: 'small', label: '90%' },
            { value: 'medium', label: '100%' },
            { value: 'large', label: '110%' },
          ]}
        />
      </SettingRow>
      <SettingRow label={tr('width')}>
        <Select
          value={value.width}
          ariaLabel={tr('width')}
          onChange={(next) => apply({ ...value, width: next as Typography['width'] })}
          options={['narrow', 'comfortable', 'wide'].map((id) => ({ value: id, label: tr(id) }))}
        />
      </SettingRow>
      {error && (
        <p className="ol-section-note" role="alert">
          {error}
        </p>
      )}
    </Card>
  );
}
