/** Only official-origin credentials may be reused for the DeepSeek suite. */
export function isOfficialDeepSeekEndpoint(endpoint: string): boolean {
  try {
    const url = new URL(endpoint || 'https://api.deepseek.com/v1');
    return (
      url.origin === 'https://api.deepseek.com' &&
      ['', '/v1'].includes(url.pathname.replace(/\/$/, '')) &&
      !url.username &&
      !url.password
    );
  } catch {
    return false;
  }
}
