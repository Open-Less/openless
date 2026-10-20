import { motion } from 'framer-motion';
import { useReducedMotion } from '../../lib/motion';
import { keyboardInteraction } from '../../lib/interactionMode';

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
  const reduced = useReducedMotion();
  const quiet = reduced || keyboardInteraction();
  return (
    <div className="ol-choice-chips" role="group" aria-label={label}>
      {options.map((option) => (
        <motion.button
          type="button"
          key={option.value}
          layout={quiet ? false : 'position'}
          aria-pressed={value === option.value}
          animate={{ transform: value === option.value && !quiet ? 'scale(1.025)' : 'scale(1)' }}
          transition={{
            layout: { type: 'spring', stiffness: 480, damping: 38 },
            duration: quiet ? 0 : 0.16,
          }}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </motion.button>
      ))}
    </div>
  );
}
