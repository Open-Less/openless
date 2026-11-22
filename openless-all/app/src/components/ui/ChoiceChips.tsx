import { Segmented } from '../../ui/controls/Segmented';

export function ChoiceChips<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  label: string;
}) {
  return <Segmented className="ol-choice-chips" options={options} value={value} onChange={onChange} label={label} />;
}
