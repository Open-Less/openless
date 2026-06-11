import type { PermissionStatus as AppPermissionStatus } from '../../../src/lib/types';
import { checkMicrophonePermission, requestMicrophonePermission } from '../../../src/lib/ipc';

const ANDROID_MIC_GRANTED_KEY = 'openless.androidMicrophoneGranted';
const ANDROID_MIC_REQUESTED_KEY = 'openless.androidMicrophoneRequested';

export async function checkAndroidMicrophoneAccess(): Promise<AppPermissionStatus> {
  try {
    const nativeStatus = await checkMicrophonePermission();
    if (nativeStatus === 'granted' || nativeStatus === 'notApplicable') {
      localStorage.setItem(ANDROID_MIC_GRANTED_KEY, '1');
      return 'granted';
    }
    if (nativeStatus === 'denied' || nativeStatus === 'restricted') {
      localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
      return localStorage.getItem(ANDROID_MIC_REQUESTED_KEY) === '1' ? 'denied' : 'notDetermined';
    }
  } catch {
    // Fall through to WebView-local checks below.
  }

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
  try {
    localStorage.setItem(ANDROID_MIC_REQUESTED_KEY, '1');
    const nativeStatus = await requestMicrophonePermission();
    if (nativeStatus === 'granted' || nativeStatus === 'notApplicable') {
      localStorage.setItem(ANDROID_MIC_GRANTED_KEY, '1');
      return 'granted';
    }
    if (nativeStatus === 'denied' || nativeStatus === 'restricted') {
      localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
    }
    if (nativeStatus === 'notDetermined') {
      return 'notDetermined';
    }
  } catch {
    // Fall through to WebView-local checks below.
  }

  if (localStorage.getItem(ANDROID_MIC_GRANTED_KEY) === '1') {
    return 'granted';
  }

  try {
    const permissions = navigator.permissions;
    if (permissions?.query) {
      const status = await permissions.query({ name: 'microphone' as PermissionName });
      if (status.state === 'granted') {
        localStorage.setItem(ANDROID_MIC_GRANTED_KEY, '1');
        return 'granted';
      }
      if (status.state === 'denied') {
        localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
        localStorage.setItem(ANDROID_MIC_REQUESTED_KEY, '1');
        return 'denied';
      }
    }
  } catch {
    // Android WebView versions differ on navigator.permissions support.
  }

  const mediaDevices = navigator.mediaDevices;
  if (!mediaDevices?.getUserMedia) {
    return 'notDetermined';
  }

  let stream: MediaStream | null = null;
  try {
    localStorage.setItem(ANDROID_MIC_REQUESTED_KEY, '1');
    stream = await mediaDevices.getUserMedia({ audio: true });
    localStorage.setItem(ANDROID_MIC_GRANTED_KEY, '1');
    return 'granted';
  } catch (error) {
    console.warn('[android-mic] WebView microphone permission request failed', error);
    localStorage.removeItem(ANDROID_MIC_GRANTED_KEY);
    if (error instanceof DOMException && error.name === 'NotAllowedError') {
      return 'denied';
    }
    return 'notDetermined';
  } finally {
    stream?.getTracks().forEach(track => track.stop());
  }
}
