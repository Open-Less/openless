import { createContext } from 'react';

/** Channel editor container on the right side of Settings; the onboarding page provides no such
 *  container and keeps using the standalone modal. */
export const ChannelEditorHostContext = createContext<{
  /** Mount point for the subpage, overlaying the right content while keeping the left nav. */
  container: HTMLDivElement | null;
  /** Content covered by the subpage; set to inert while editing so keyboard focus cannot fall
   *  behind. */
  background: HTMLDivElement | null;
  /** When switching settings categories or closing Settings, run the editor's draft cleanup
   *  first. */
  registerClose: (close: (() => void) | null) => void;
} | null>(null);
