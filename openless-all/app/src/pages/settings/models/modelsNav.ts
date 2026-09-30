import { createContext, useContext } from 'react';

/**
 * Callback that navigates to the "Services → Local models" view. Provided by the ServicesTab that
 * owns the view state; deep surfaces such as the channel editor use it to bring the user to the
 * model management page. When unavailable (e.g. standalone test rendering), callers should degrade
 * to hiding the navigation entry.
 */
export const LocalModelsNavContext = createContext<(() => void) | null>(null);

export function useLocalModelsNav(): (() => void) | null {
  return useContext(LocalModelsNavContext);
}
