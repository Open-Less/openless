import * as fs from 'node:fs/promises';
import path from 'node:path';
import { Type } from 'typebox';

const MAX_FILE_BYTES = 1024 * 1024;
const protectedNames = new Set(['.git', '.env', '.pi', '.codex', '.ssh', '.aws', '.zshrc', '.zprofile', '.bashrc', '.bash_profile']);

function inside(root, candidate) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!path.isAbsolute(relative) && relative !== '..' && !relative.startsWith(`..${path.sep}`));
}

export async function safeWorkspacePath(cwd, input, { write = false, disallowed = [] } = {}) {
  if (typeof input !== 'string' || !input || input.includes('\0')) throw new Error('Invalid file path');
  const root = await fs.realpath(cwd);
  const requested = path.resolve(root, input);
  if (!inside(root, requested)) throw new Error('File tools are limited to the selected workspace');
  const relative = path.relative(root, requested);
  const parts = relative.toLowerCase().split(path.sep);
  if (write && (parts.some(part => protectedNames.has(part) || part.startsWith('.env.')) || parts.includes('launchagents') || parts.includes('startup'))) {
    throw new Error('Writing credentials, repository internals or startup configuration is blocked');
  }
  for (const rule of disallowed) {
    const match = /^(read|write|edit)\((.+)\)$/i.exec(rule.trim());
    if (!match || (write ? !['write', 'edit'].includes(match[1].toLowerCase()) : match[1].toLowerCase() !== 'read')) continue;
    const glob = match[2].replaceAll('\\', '/');
    const escaped = glob.replace(/[.+^${}()|[\]\\]/g, '\\$&').replaceAll('**', '\u0000').replaceAll('*', '[^/]*').replaceAll('\u0000', '.*');
    const normalized = relative.replaceAll(path.sep, '/');
    if (new RegExp(`^${escaped}$`, 'i').test(normalized) || new RegExp(`^${escaped}$`, 'i').test(path.basename(requested))) {
      throw new Error('File path is blocked by the configured tool policy');
    }
  }
  // Inspect every component, including dangling symlinks and Windows junctions.
  let cursor = root;
  for (const component of relative.split(path.sep).filter(Boolean)) {
    cursor = path.join(cursor, component);
    let stat;
    try { stat = await fs.lstat(cursor); } catch (error) {
      if (write && error.code === 'ENOENT') continue;
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error('File tools do not follow symbolic links or junctions');
    const real = await fs.realpath(cursor);
    if (!inside(root, real)) throw new Error('File path escapes the selected workspace');
  }
  return requested;
}

const textResult = text => ({ content: [{ type: 'text', text }], details: {} });

export function fileTools(cwd, request) {
  const checked = (input, write = false) => safeWorkspacePath(cwd, input, { write, disallowed: request.disallowed_tools });
  const readText = async input => {
    const target = await checked(input);
    const handle = await fs.open(target, 'r');
    try {
      const stat = await handle.stat();
      if (!stat.isFile() || stat.size > MAX_FILE_BYTES) throw new Error('Read supports UTF-8 files up to 1 MiB');
      const buffer = await handle.readFile();
      if (buffer.includes(0)) throw new Error('Binary file; use computer_screenshot for images');
      return buffer.toString('utf8');
    } finally { await handle.close(); }
  };
  const persist = async (input, text, signal) => {
    if (Buffer.byteLength(text, 'utf8') > MAX_FILE_BYTES) throw new Error('Write exceeds 1 MiB');
    const target = await checked(input, true);
    signal?.throwIfAborted();
    await fs.mkdir(path.dirname(target), { recursive: true });
    // Recheck after creating parents; reject late links introduced before the write.
    await checked(input, true);
    signal?.throwIfAborted();
    await fs.writeFile(target, text, { encoding: 'utf8', signal });
    return textResult(`Saved ${path.relative(cwd, target)}`);
  };
  return [
    {
      name: 'read', label: 'Read file', description: 'Read a UTF-8 file inside the selected workspace (up to 1 MiB).',
      parameters: Type.Object({ path: Type.String() }), mutating: false,
      execute: async (_id, params, signal) => { signal?.throwIfAborted(); return textResult(await readText(params.path)); },
    },
    {
      name: 'ls', label: 'List files', description: 'List one directory inside the selected workspace.',
      parameters: Type.Object({ path: Type.Optional(Type.String()) }), mutating: false,
      execute: async (_id, params, signal) => {
        signal?.throwIfAborted();
        const entries = await fs.readdir(await checked(params.path || '.'), { withFileTypes: true });
        return textResult(entries.slice(0, 1000).map(item => `${item.name}${item.isDirectory() ? '/' : item.isSymbolicLink() ? ' [link]' : ''}`).join('\n'));
      },
    },
    {
      name: 'find', label: 'Find files', description: 'Find workspace files by a case-insensitive substring in their relative paths. Does not follow links or search repository internals.',
      parameters: Type.Object({ query: Type.String(), path: Type.Optional(Type.String()) }), mutating: false,
      execute: async (_id, params, signal) => {
        const result = [];
        let visited = 0;
        const walk = async (directory, depth) => {
          signal?.throwIfAborted();
          if (depth > 12 || visited > 10000 || result.length >= 500) return;
          for (const entry of await fs.readdir(directory, { withFileTypes: true })) {
            visited++;
            if (entry.isSymbolicLink() || ['.git', 'node_modules'].includes(entry.name)) continue;
            const target = path.join(directory, entry.name);
            const relative = path.relative(cwd, target);
            if (relative.toLowerCase().includes(params.query.toLowerCase())) result.push(relative);
            if (entry.isDirectory()) await walk(target, depth + 1);
            if (visited > 10000 || result.length >= 500) break;
          }
        };
        await walk(await checked(params.path || '.'), 0);
        return textResult(result.join('\n'));
      },
    },
    {
      name: 'write', label: 'Write file', description: 'Create or replace a UTF-8 workspace file. Credentials, repository internals and startup files are protected.',
      parameters: Type.Object({ path: Type.String(), content: Type.String({ maxLength: MAX_FILE_BYTES }) }), mutating: true,
      execute: async (_id, params, signal) => persist(params.path, params.content, signal),
    },
    {
      name: 'edit', label: 'Edit file', description: 'Replace exactly one literal text occurrence in a UTF-8 workspace file.',
      parameters: Type.Object({ path: Type.String(), oldText: Type.String({ minLength: 1 }), newText: Type.String({ maxLength: MAX_FILE_BYTES }) }), mutating: true,
      execute: async (_id, params, signal) => {
        await checked(params.path, true);
        const content = await readText(params.path);
        const index = content.indexOf(params.oldText);
        if (index < 0 || content.indexOf(params.oldText, index + params.oldText.length) !== -1) throw new Error('oldText must match exactly once');
        return persist(params.path, content.slice(0, index) + params.newText + content.slice(index + params.oldText.length), signal);
      },
    },
  ];
}
