// The official shadcn cn utility (verbatim from lib/utils.ts): clsx composition +
// tailwind-merge conflict resolution.
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
