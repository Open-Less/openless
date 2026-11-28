export const dictationReferenceKeyOptions = Array.from('ABCDEFGHIJKLMNOPQRSTUVWXYZ', (key) => ({
  value: key,
  label: key,
}));

/** The preferences provider owns rollback; the page only needs the save outcome. */
export async function saveDictationReferencePreference(save: () => Promise<void>): Promise<boolean> {
  try {
    await save();
    return true;
  } catch {
    return false;
  }
}
