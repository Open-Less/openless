import type { PermissionStatus as AppPermissionStatus } from './types';

export async function requestAndroidMicrophoneAccess(): Promise<AppPermissionStatus> {
  const mediaDevices = navigator.mediaDevices;
  if (!mediaDevices?.getUserMedia) {
    return 'notDetermined';
  }

  let stream: MediaStream | null = null;
  try {
    stream = await mediaDevices.getUserMedia({ audio: true });
    return 'granted';
  } catch (error) {
    const name = error instanceof DOMException ? error.name : '';
    if (name === 'NotAllowedError' || name === 'SecurityError' || name === 'PermissionDeniedError') {
      return 'denied';
    }
    console.warn('[android-mic] WebView microphone permission request failed', error);
    return 'notDetermined';
  } finally {
    stream?.getTracks().forEach(track => track.stop());
  }
}
