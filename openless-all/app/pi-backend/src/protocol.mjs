import { StringDecoder } from 'node:string_decoder';

export const MAX_REQUEST_BYTES = 4 * 1024 * 1024;

// LF is the only delimiter. Preserve UTF-8 across chunks and U+2028/U+2029 in strings.
export async function* jsonLines(stream) {
  const decoder = new StringDecoder('utf8');
  let pending = '';
  for await (const chunk of stream) {
    pending += typeof chunk === 'string' ? chunk : decoder.write(chunk);
    let newline;
    while ((newline = pending.indexOf('\n')) !== -1) {
      const line = pending.slice(0, newline).replace(/\r$/, '');
      pending = pending.slice(newline + 1);
      if (Buffer.byteLength(line, 'utf8') > MAX_REQUEST_BYTES) throw new Error('Request exceeds 4 MiB');
      if (line.trim()) yield JSON.parse(line);
    }
    if (Buffer.byteLength(pending, 'utf8') > MAX_REQUEST_BYTES) throw new Error('Request exceeds 4 MiB');
  }
  pending += decoder.end();
  if (pending.trim()) yield JSON.parse(pending);
}

export function normalizeRequest(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Expected a JSON object');
  if (value.type && value.type !== 'prompt') throw new Error('Expected type=prompt');
  if (typeof value.prompt !== 'string' || !value.prompt.trim()) throw new Error('prompt must not be empty');
  for (const name of ['cwd', 'model', 'session_id', 'continuation_context', 'extra_system_prompt']) {
    if (value[name] != null && typeof value[name] !== 'string') throw new Error(`${name} must be a string`);
  }
  for (const name of ['allowed_tools', 'disallowed_tools']) {
    if (value[name] != null && (!Array.isArray(value[name]) || !value[name].every(item => typeof item === 'string'))) {
      throw new Error(`${name} must be a string array`);
    }
  }
  if (value.permission_mode != null && !['plan', 'default', 'acceptEdits', 'bypassPermissions'].includes(value.permission_mode)) {
    throw new Error('Unknown permission_mode');
  }
  return {
    ...value,
    // Unattended default and legacy unrestricted mode have no approval transport.
    permission_mode: value.permission_mode === 'acceptEdits' ? 'acceptEdits' : 'plan',
    allowed_tools: value.allowed_tools ?? [],
    disallowed_tools: value.disallowed_tools ?? [],
    session_persistence: value.session_persistence === true,
    continue_session: value.continue_session === true,
  };
}

const aliases = { list: 'ls', listdirectory: 'ls', glob: 'find', screenshot: 'computer_screenshot', computer: 'computer_*' };
function normalizeRule(rule) {
  const name = rule.trim().split('(')[0].toLowerCase();
  return aliases[name] ?? name;
}

function matches(rule, name) {
  const normalized = normalizeRule(rule);
  return normalized === '*' || normalized === name || (normalized.endsWith('*') && name.startsWith(normalized.slice(0, -1)));
}

export function toolAllowed(name, mutating, request) {
  if (mutating && request.permission_mode !== 'acceptEdits') return false;
  // Parameter-scoped deny rules are handled at the file-path layer; bare tool rules apply here.
  if (request.disallowed_tools.some(rule => !rule.includes('(') && matches(rule, name))) return false;
  return !request.allowed_tools.length || request.allowed_tools.some(rule => matches(rule, name));
}
