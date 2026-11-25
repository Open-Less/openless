// Authored factual disclosure for development builds; this is not a final legal agreement.
// Source: docs/cloud-sync-agreement/runtime-disclosure.candidate.json.
import cloudSyncDisclosure from './disclosure.json';

export function syncDisclosure(language: string) {
  return cloudSyncDisclosure[language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en'];
}
