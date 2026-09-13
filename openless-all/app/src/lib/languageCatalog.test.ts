import {
  filterLanguages,
  localizedLanguages,
  nativeLanguageName,
  SUPPORTED_LANGUAGES,
} from './languageCatalog.ts';

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const english = localizedLanguages('en', ['My custom language']);
const french = english.find((language) => language.nativeName === 'Français');
assert(french?.displayName === 'French', 'display language names follow the UI locale');
assert(french.label.includes('Français'), 'native language names remain visible');
assert(
  SUPPORTED_LANGUAGES.includes('简体中文'),
  'existing native-name preference values stay compatible',
);
assert(
  nativeLanguageName('繁體中文') === '繁体中文',
  'traditional Chinese aliases resolve without changing scripts',
);
assert(
  nativeLanguageName('zh-CN') === '简体中文',
  'language-code preferences resolve to their existing native value',
);
assert(
  filterLanguages(english, 'francais').some((language) => language.nativeName === 'Français'),
  'search accepts native names without accents',
);
assert(
  filterLanguages(english, 'nl').some((language) => language.nativeName === 'Nederlands'),
  'search also accepts language codes',
);
assert(
  filterLanguages(localizedLanguages('de'), 'Französisch')[0]?.nativeName === 'Français',
  'search accepts localized names',
);
assert(
  english.some((language) => language.nativeName === 'My custom language'),
  'previously saved custom values remain visible',
);
assert(
  filterLanguages(english, 'language-that-is-not-present').length === 0,
  'no matching language yields an empty result',
);
console.log('languageCatalog tests passed');
