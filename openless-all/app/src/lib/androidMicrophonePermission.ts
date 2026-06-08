import type { PermissionStatus as AppPermissionStatus } from './types';

const ANDROID_MIC_GRANTED_KEY = 'openless.androidMicrophoneGranted';

export async function checkAndroidMicrophoneAccess(): Promise<AppPermissionStatus> {
  if (localStorage.getItem(ANDROID_MIC_GRANTED_KEY) === '1') {
    return 'granted';
  }

  try {
    const permissions = navigator.permissions;
    if (permissions?.query) {
      const status = await permissions.query({ name: 'microphone' as PermissionName });
      if (status.state === 'granted') return 'granted';
      if (status.state === 'denied') {
        localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
        return 'denied';
      }
    }
  } catch {
    // Android WebView versions differ on navigator.permissions support.
  }

  return 'notDetermined';
}

export async function requestAndroidMicrophoneAccess(): Promise<AppPermissionStatus> {
  const mediaDevices = navigator.mediaDevices;
  if (!mediaDevices?.getUserMedia) {
    return 'notDetermined';
  }

  let stream: MediaStream | null = null;
  try {
    localStorage.setItem(ANDROID_MIC_GRANTED_KEY, '1');
    stream = await mediaDevices.getUserMedia({ audio: true });
    return 'granted';
  } catch (error) {
    const name = error instanceof DOMException ? error.name : '';
    if (name === 'NotAllowedError' || name === 'SecurityError' || name === 'PermissionDeniedError') {
      localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
      return 'denied';
    }
    console.warn('[android-mic] WebView microphone permission request failed', error);
    return 'granted';
  } finally {
    stream?.getTracks().forEach(track => track.stop());
  }
}
