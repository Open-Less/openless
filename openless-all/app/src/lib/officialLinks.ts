/** Official destinations follow the selected interface language. */
export function officialLinks(locale: string) {
  const language = locale === 'zh-CN' ? 'zh' : 'en';
  return {
    docs: `https://openless.top/${language}/docs/`,
    community: `https://openless.top/${language}/#qq-support`,
  };
}
