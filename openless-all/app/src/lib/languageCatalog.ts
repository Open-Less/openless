/// <reference lib="es2021.intl" />

import catalog from '../../contract/language-catalog.json';

export interface LanguageDefinition {
  code: string;
  nativeName: string;
  asrCode: string;
  appleLocale: string;
  aliases?: readonly string[];
}

export interface LocalizedLanguage {
  code: string;
  nativeName: string;
  displayName: string;
  label: string;
  searchText: string;
}

/** Core and the Apple Speech adapter read the same catalog; stored values remain native names. */
export const LANGUAGE_CATALOG: readonly LanguageDefinition[] = catalog;
export const SUPPORTED_LANGUAGES: readonly string[] = catalog.map(
  (language) => language.nativeName,
);

export function nativeLanguageName(value: string): string {
  const key = value.trim();
  return (
    LANGUAGE_CATALOG.find(
      (language) =>
        language.nativeName === key ||
        language.code.toLowerCase() === key.toLowerCase() ||
        language.aliases?.some((alias) => alias.toLowerCase() === key.toLowerCase()),
    )?.nativeName ?? key
  );
}

function searchText(value: string): string {
  return value.normalize('NFKD').replace(/\p{M}/gu, '').toLocaleLowerCase();
}

const localizedCatalogCache = new Map<string, readonly LocalizedLanguage[]>();

function getBaseLocalizedLanguages(locale: string): readonly LocalizedLanguage[] {
  const cached = localizedCatalogCache.get(locale);
  if (cached) return cached;
  let names: Intl.DisplayNames | undefined;
  try {
    if (typeof Intl.DisplayNames === 'function') {
      names = new Intl.DisplayNames([locale], { type: 'language', languageDisplay: 'standard' });
    }
  } catch {
    // Older webviews can still show and save the native names without a locale-name implementation.
  }
  const languages = LANGUAGE_CATALOG.map((language) => {
    const displayName = names?.of(language.code) ?? language.nativeName;
    return {
      code: language.code,
      nativeName: language.nativeName,
      displayName,
      label:
        displayName === language.nativeName
          ? displayName
          : `${displayName} · ${language.nativeName}`,
      searchText: searchText(
        [
          displayName,
          language.nativeName,
          language.code,
          language.asrCode,
          ...(language.aliases ?? []),
        ].join(' '),
      ),
    };
  });
  const collator = new Intl.Collator(locale, { sensitivity: 'base', numeric: true });
  languages.sort((left, right) => collator.compare(left.displayName, right.displayName));
  if (localizedCatalogCache.size >= 4) localizedCatalogCache.clear();
  localizedCatalogCache.set(locale, languages);
  return languages;
}

export function localizedLanguages(
  locale: string,
  existingValues: readonly string[] = [],
): LocalizedLanguage[] {
  const base = getBaseLocalizedLanguages(locale);
  if (existingValues.length === 0) return [...base];
  const known = new Set(base.map((language) => language.nativeName));
  const extras: LocalizedLanguage[] = [];
  for (const value of existingValues) {
    const nativeName = nativeLanguageName(value);
    if (!nativeName || known.has(nativeName)) continue;
    known.add(nativeName);
    // Keep a previously saved custom name visible and removable instead of silently dropping it.
    extras.push({
      code: '',
      nativeName,
      displayName: nativeName,
      label: nativeName,
      searchText: searchText(nativeName),
    });
  }
  if (extras.length === 0) return [...base];
  const collator = new Intl.Collator(locale, { sensitivity: 'base', numeric: true });
  return [...base, ...extras].sort((left, right) =>
    collator.compare(left.displayName, right.displayName),
  );
}

export function filterLanguages(
  languages: readonly LocalizedLanguage[],
  query: string,
): LocalizedLanguage[] {
  const needle = searchText(query.trim());
  return languages.filter((language) => language.searchText.includes(needle));
}
