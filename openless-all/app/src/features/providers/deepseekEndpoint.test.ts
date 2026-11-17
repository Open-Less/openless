import { isOfficialDeepSeekEndpoint } from './deepseekEndpoint';
for (const endpoint of ['', 'https://api.deepseek.com', 'https://api.deepseek.com/v1/']) {
  if (!isOfficialDeepSeekEndpoint(endpoint)) throw new Error('official endpoint must be reusable');
}
for (const endpoint of [
  'invalid',
  'http://api.deepseek.com/v1',
  'https://gateway.example/v1',
  'https://api.deepseek.com.evil.test/v1',
  'https://user:secret@api.deepseek.com/v1',
  'https://api.deepseek.com:8443/v1',
]) {
  if (isOfficialDeepSeekEndpoint(endpoint))
    throw new Error('gateway credentials must not be reused');
}
