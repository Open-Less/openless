import { spawn } from 'node:child_process';
import { isAbsolute } from 'node:path';
import { Type } from 'typebox';

const MAX_OUTPUT_BYTES = 48 * 1024 * 1024;

export function callComputer(request, { signal, executable = process.env.OPENLESS_COMPUTER_BIN, timeoutMs = 30000 } = {}) {
  if (!executable || !isAbsolute(executable)) return Promise.reject(new Error('OPENLESS_COMPUTER_BIN must point to the bundled Computer executable'));
  if (signal?.aborted) return Promise.reject(new Error('Computer operation cancelled'));
  return new Promise((resolve, reject) => {
    // Inherit the host's process group / Windows Job so cancellation owns descendants.
    const child = spawn(executable, [], { stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true, shell: false });
    const stdout = [];
    const stderr = [];
    let size = 0;
    let stderrSize = 0;
    let settled = false;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
      if (error) { child.kill(); reject(error); } else resolve(value);
    };
    const abort = () => finish(new Error('Computer operation cancelled'));
    const timer = setTimeout(() => finish(new Error('Computer operation timed out')), timeoutMs);
    signal?.addEventListener('abort', abort, { once: true });
    child.once('error', error => finish(error));
    child.stdin.on('error', error => finish(error));
    child.stdout.on('data', chunk => {
      size += chunk.length;
      if (size > MAX_OUTPUT_BYTES) finish(new Error('Computer response exceeds 48 MiB'));
      else stdout.push(chunk);
    });
    child.stderr.on('data', chunk => {
      if (stderrSize < 8192) stderr.push(chunk.subarray(0, 8192 - stderrSize));
      stderrSize += chunk.length;
    });
    child.once('close', code => {
      if (settled) return;
      try {
        const result = JSON.parse(Buffer.concat(stdout).toString('utf8').trim());
        if (result.ok !== true) throw new Error(result.error?.message || `Computer exited with status ${code}`);
        if (code !== 0) throw new Error(`Computer exited with status ${code}`);
        finish(null, result.data);
      } catch (error) {
        finish(new Error(`Computer: ${error.message}${stderr.length ? ` (${Buffer.concat(stderr).toString('utf8').trim()})` : ''}`));
      }
    });
    if (signal?.aborted) abort();
    else child.stdin.end(`${JSON.stringify(request)}\n`, 'utf8');
  });
}

const monitor = { monitor_id: Type.Optional(Type.Integer({ minimum: 0 })) };
const position = { ...monitor, x: Type.Integer({ minimum: 0 }), y: Type.Integer({ minimum: 0 }) };
const schemas = {
  capabilities: Type.Object({}),
  displays: Type.Object({}),
  screenshot: Type.Object(monitor),
  move: Type.Object(position),
  click: Type.Object({ ...position, button: Type.Optional(Type.Union(['left', 'right', 'middle'].map(Type.Literal))), clicks: Type.Optional(Type.Union([Type.Literal(1), Type.Literal(2)])) }),
  scroll: Type.Object({ amount: Type.Integer({ minimum: -100, maximum: 100 }), axis: Type.Optional(Type.Union([Type.Literal('vertical'), Type.Literal('horizontal')])) }),
  key: Type.Object({ key: Type.String({ minLength: 1, maxLength: 32 }), modifiers: Type.Optional(Type.Array(Type.Union(['ctrl', 'alt', 'shift', 'meta'].map(Type.Literal)), { maxItems: 4 })) }),
  type_text: Type.Object({ text: Type.String({ maxLength: 100000 }) }),
};
const descriptions = {
  capabilities: 'Read native computer availability, session type and permission status.',
  displays: 'List displays and their IDs and native pixel sizes.',
  screenshot: 'Capture a display and return its image. Coordinates for later pointer actions are native pixels relative to this image, not global desktop coordinates.',
  move: 'Move the pointer to x/y in the selected display screenshot. Call screenshot first.',
  click: 'Click x/y in the selected display screenshot. Call screenshot first; inspect the result after clicking.',
  scroll: 'Scroll at the current pointer position. Positive amount scrolls down (or right for horizontal).',
  key: 'Press and release one key with optional modifiers. No keys remain held between calls.',
  type_text: 'Type literal text into the focused application. Confirm the intended focus using a screenshot first.',
};

export function computerTools(call = callComputer) {
  return Object.entries(schemas).map(([action, parameters]) => ({
    name: `computer_${action}`,
    label: `Computer ${action}`,
    description: descriptions[action],
    parameters,
    mutating: !['capabilities', 'displays', 'screenshot'].includes(action),
    async execute(_id, params, signal) {
      const data = await call({ ...params, action }, { signal });
      if (action === 'screenshot') {
        if (typeof data?.image_base64 !== 'string') throw new Error('Computer returned no screenshot');
        const { image_base64, ...metadata } = data;
        return {
          content: [
            { type: 'text', text: JSON.stringify(metadata) },
            { type: 'image', mimeType: data.mime_type || 'image/png', data: image_base64 },
          ],
          details: metadata,
        };
      }
      return { content: [{ type: 'text', text: JSON.stringify(data) }], details: data };
    },
  }));
}
