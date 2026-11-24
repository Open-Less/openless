/**
 * Animated eye geometry adapted from Rare UI's Password Input.
 * Copyright (c) 2026 Swami Malode. See public/licenses/rare-ui-LICENSE.txt.
 * https://www.rareui.com/components/passwordinput
 */
import { useId } from 'react';
import { motion, useReducedMotion } from 'framer-motion';

const lashes = ['M4.4 14.6 3 16.4', 'M9 16.6 8.5 19', 'M15 16.6l.5 2.4', 'M19.6 14.6l1.4 1.8'];
const corner = (y: number) => 12 + (y - 12) * 0.64;
const eyePath = (top: number, bottom: number) =>
  `M2 12C4 ${corner(top)} 7.5 ${top} 12 ${top}C16.5 ${top} 20 ${corner(top)} 22 12C20 ${corner(bottom)} 16.5 ${bottom} 12 ${bottom}C7.5 ${bottom} 4 ${corner(bottom)} 2 12Z`;

export function PasswordEye({ open }: { open: boolean }) {
  const clipId = useId();
  const reduceMotion = useReducedMotion();
  const lid = { d: open ? eyePath(5.5, 18.5) : eyePath(17, 17) };
  const transition = reduceMotion
    ? { duration: 0 }
    : { type: 'spring' as const, stiffness: 500, damping: 34 };

  return (
    <svg
      aria-hidden="true"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <clipPath id={clipId}>
        <motion.path initial={false} animate={lid} transition={transition} />
      </clipPath>
      <circle cx={12} cy={12} r={3} clipPath={`url(#${clipId})`} />
      <motion.path initial={false} animate={lid} transition={transition} />
      {lashes.map((d) => (
        <motion.path
          key={d}
          d={d}
          initial={false}
          animate={{ pathLength: open ? 0 : 1, opacity: open ? 0 : 1 }}
          transition={{ duration: reduceMotion ? 0 : 0.16 }}
        />
      ))}
    </svg>
  );
}
