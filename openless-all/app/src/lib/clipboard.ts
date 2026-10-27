/** Copy text through the async clipboard API, with a browser/webview fallback. */
export async function copyTextToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // Some embedded webviews expose neither a permissioned async clipboard nor a focused document.
    let textarea: HTMLTextAreaElement | null = null;
    try {
      textarea = document.createElement('textarea');
      textarea.value = text;
      textarea.style.position = 'fixed';
      textarea.style.opacity = '0';
      textarea.setAttribute('aria-hidden', 'true');
      document.body.appendChild(textarea);
      textarea.select();
      return document.execCommand('copy');
    } catch {
      return false;
    } finally {
      if (textarea?.parentNode) textarea.parentNode.removeChild(textarea);
    }
  }
}
