import { createContext } from 'react';

/** Optional editor host inside a bounded setup dialog. Settings uses a standalone modal. */
export const ChannelEditorHostContext = createContext<{
  /** Mount point for the subpage, overlaying the right content while keeping the left nav. */
  container: HTMLDivElement | null;
  presentation?: 'inline' | 'modal';
  /** Content covered by the subpage; set to inert while editing so keyboard focus cannot fall
   *  behind. */
  background: HTMLDivElement | null;
  /** When switching settings categories or closing Settings, run the editor's draft cleanup
   *  first. */
  registerClose: (close: (() => void) | null) => void;
} | null>(null);
