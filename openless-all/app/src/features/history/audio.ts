export function audioBytes(dataUrl: string): Uint8Array<ArrayBuffer> {
  const encoded = dataUrl.slice('data:audio/wav;base64,'.length);
  if (!dataUrl.startsWith('data:audio/wav;base64,') || !encoded) throw new Error('empty recording');
  return Uint8Array.from(atob(encoded), (character) => character.charCodeAt(0));
}

export function recordingFileName(sessionId: string): string {
  return `openless-recording-${sessionId}.wav`;
}
