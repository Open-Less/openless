// shadcn 官方 cn 工具（lib/utils.ts 原样）：clsx 组合 + tailwind-merge 去冲突。
import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
