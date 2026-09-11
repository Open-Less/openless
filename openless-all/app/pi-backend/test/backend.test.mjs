import test from 'node:test';
import assert from 'node:assert/strict';
import { Readable } from 'node:stream';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { jsonLines, normalizeRequest, toolAllowed } from '../src/protocol.mjs';
import { safeWorkspacePath, fileTools } from '../src/files.mjs';
import { computerTools, callComputer } from '../src/computer.mjs';
import { configDirectory, loadConfig } from '../src/config.mjs';
import { createSession, runRequest } from '../src/runtime.mjs';

async function workspace(t) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'openless-pi-test-'));
  t.after(() => fs.rm(directory, { recursive: true, force: true }));
  return directory;
}

test('JSONL preserves split UTF-8, embedded Unicode separators, CRLF and final EOF record', async () => {
  const text = JSON.stringify({ prompt: '中文\u2028第一行\u2029第二行' });
  const bytes = Buffer.from(text + '\r\n' + JSON.stringify({ type: 'cancel' }));
  const chunks = Array.from(bytes, value => Buffer.from([value]));
  const result = [];
  for await (const value of jsonLines(Readable.from(chunks))) result.push(value);
  assert.deepEqual(result, [JSON.parse(text), { type: 'cancel' }]);
});

test('invalid request policy is rejected and unattended default/bypass are read-only', () => {
  assert.throws(() => normalizeRequest({ prompt: 'x', permission_mode: 'typo' }), /permission_mode/);
  for (const permission_mode of [undefined, 'default', 'bypassPermissions', 'plan']) {
    const request = normalizeRequest({ prompt: 'x', permission_mode });
    assert.equal(toolAllowed('computer_click', true, request), false);
    assert.equal(toolAllowed('write', true, request), false);
    assert.equal(toolAllowed('computer_screenshot', false, request), true);
  }
  const request = normalizeRequest({ prompt: 'x', permission_mode: 'acceptEdits', allowed_tools: ['Computer', 'Read'], disallowed_tools: ['computer_key'] });
  assert.equal(toolAllowed('computer_click', true, request), true);
  assert.equal(toolAllowed('computer_key', true, request), false);
  assert.equal(toolAllowed('write', true, request), false);
  assert.equal(toolAllowed('read', false, request), true);
});

test('file policy blocks path escape, links, protected files and configured path rules', async t => {
  const directory = await workspace(t);
  const outside = await workspace(t);
  await fs.writeFile(path.join(directory, '中文.txt'), '你好', 'utf8');
  assert.equal(await safeWorkspacePath(directory, '中文.txt'), path.join(directory, '中文.txt'));
  await assert.rejects(safeWorkspacePath(directory, '../escape.txt', { write: true }), /workspace/);
  for (const name of ['.env', '.env.local', '.git/config', '.ssh/config', 'Startup/app.bat']) {
    await assert.rejects(safeWorkspacePath(directory, name, { write: true }), /blocked/);
  }
  await assert.rejects(safeWorkspacePath(directory, 'secret.txt', { write: true, disallowed: ['Write(secret.txt)'] }), /policy/);
  await fs.symlink(outside, path.join(directory, 'linked'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(safeWorkspacePath(directory, 'linked/file.txt', { write: true }), /links|junctions/);
  await fs.symlink(path.join(outside, 'missing'), path.join(directory, 'dangling'), process.platform === 'win32' ? 'junction' : 'dir');
  await assert.rejects(safeWorkspacePath(directory, 'dangling/file.txt', { write: true }), /links|junctions/);
});

test('workspace tools use UTF-8, exact replacement and cancellation', async t => {
  const directory = await workspace(t);
  const tools = fileTools(directory, normalizeRequest({ prompt: 'x', permission_mode: 'acceptEdits' }));
  const run = (name, params, signal) => tools.find(tool => tool.name === name).execute('test', params, signal);
  await run('write', { path: 'notes/中文.txt', content: '你好 世界' });
  assert.equal((await run('read', { path: 'notes/中文.txt' })).content[0].text, '你好 世界');
  await run('edit', { path: 'notes/中文.txt', oldText: '世界', newText: 'PI' });
  assert.equal(await fs.readFile(path.join(directory, 'notes/中文.txt'), 'utf8'), '你好 PI');
  await assert.rejects(run('edit', { path: 'notes/中文.txt', oldText: '不存在', newText: '' }), /exactly once/);
  const cancelled = AbortSignal.abort();
  await assert.rejects(run('write', { path: 'cancelled.txt', content: 'no' }, cancelled), /abort/i);
  await assert.rejects(fs.stat(path.join(directory, 'cancelled.txt')), { code: 'ENOENT' });
});

test('screenshot is model-visible image content and native coordinates pass through unchanged', async () => {
  const calls = [];
  const tools = computerTools(async (request, options) => {
    calls.push({ request, options });
    return { image_base64: 'aGVsbG8=', mime_type: 'image/png', width: 1280, height: 720, monitor: { id: 8 } };
  });
  const signal = new AbortController().signal;
  const image = await tools.find(tool => tool.name === 'computer_screenshot').execute('s', { monitor_id: 8 }, signal);
  assert.deepEqual(image.content[1], { type: 'image', mimeType: 'image/png', data: 'aGVsbG8=' });
  assert.equal(JSON.stringify(image.details).includes('aGVsbG8='), false);
  await tools.find(tool => tool.name === 'computer_click').execute('c', { monitor_id: 8, x: 310, y: 44 }, signal);
  assert.deepEqual(calls[1].request, { action: 'click', monitor_id: 8, x: 310, y: 44 });
  assert.equal(calls[1].options.signal, signal);
  await assert.rejects(callComputer({ action: 'screenshot' }, { executable: 'unsafe-relative-path' }), /bundled/);
});

function fakeSession(onPrompt) {
  let listener;
  return {
    sessionId: 'fake-session', disposed: false, aborted: false,
    subscribe(callback) { listener = callback; return () => { listener = undefined; }; },
    prompt(text) { return onPrompt(event => listener?.(event), text, this); },
    async abort() { this.aborted = true; this.release?.(); },
    dispose() { this.disposed = true; },
  };
}

test('real event translation emits streamed text, tools and one terminal completion', async () => {
  const emitted = [];
  const session = fakeSession(async emit => {
    emit({ type: 'message_update', assistantMessageEvent: { type: 'text_delta', delta: '完成' } });
    emit({ type: 'tool_execution_start', toolCallId: 'tool-1', toolName: 'computer_screenshot', args: { monitor_id: 0 } });
    emit({ type: 'tool_execution_end', toolCallId: 'tool-1', toolName: 'computer_screenshot', result: { image_base64: 'private' }, isError: false });
    emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'stop', usage: { cost: { total: 0.03 } } } });
  });
  await runRequest({ prompt: '查看屏幕' }, { emit: event => emitted.push(event), sessionFactory: async () => session });
  assert.deepEqual(emitted.map(event => event.type), ['started', 'delta', 'tool_use', 'tool_result', 'complete']);
  assert.equal(emitted.at(-1).text, '完成');
  assert.equal(emitted.at(-1).cost_usd, 0.03);
  assert.equal(JSON.stringify(emitted).includes('private'), false);
  assert.equal(session.disposed, true);
});

test('SDK error event is a failed run and never reported as complete', async () => {
  const emitted = [];
  const session = fakeSession(async emit => emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'error', errorMessage: 'model unavailable' } }));
  await assert.rejects(runRequest({ prompt: 'x' }, { emit: event => emitted.push(event), sessionFactory: async () => session }), /model unavailable/);
  assert.equal(emitted.some(event => event.type === 'complete'), false);
  assert.equal(session.disposed, true);
});

test('a successful SDK retry supersedes its earlier transient error', async () => {
  const emitted = [];
  const session = fakeSession(async emit => {
    emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'error', errorMessage: 'temporarily overloaded' } });
    emit({ type: 'message_update', assistantMessageEvent: { type: 'text_delta', delta: '重试后完成' } });
    emit({ type: 'message_end', message: { role: 'assistant', stopReason: 'stop' } });
  });
  const result = await runRequest({ prompt: 'x' }, { emit: event => emitted.push(event), sessionFactory: async () => session });
  assert.equal(result.text, '重试后完成');
  assert.equal(emitted.at(-1).type, 'complete');
});

test('cancellation aborts the SDK, prevents completion and cleans up', async () => {
  const controller = new AbortController();
  const emitted = [];
  let active;
  const started = new Promise(resolve => { active = resolve; });
  const session = fakeSession(async (_emit, _text, self) => { active(); await new Promise(resolve => { self.release = resolve; }); });
  const running = runRequest({ prompt: 'x' }, { emit: event => emitted.push(event), signal: controller.signal, sessionFactory: async () => session });
  await started;
  controller.abort(new Error('cancelled'));
  await assert.rejects(running, /cancelled/);
  assert.equal(session.aborted, true);
  assert.equal(session.disposed, true);
  assert.equal(emitted.some(event => event.type === 'complete'), false);
});

test('private config supports an OpenAI-compatible endpoint without exposing credentials', async t => {
  const directory = await workspace(t);
  assert.equal(configDirectory({ OPENLESS_PI_HOME: directory }), directory);
  await fs.writeFile(path.join(directory, 'config.json'), JSON.stringify({ provider: 'local', model: 'test', apiKey: 'private-test-value', baseUrl: 'http://127.0.0.1:1/v1' }), 'utf8');
  assert.equal((await loadConfig(directory)).model, 'test');
  await fs.writeFile(path.join(directory, 'config.json'), '{"apiKey":"private-test-value",broken}', 'utf8');
  await assert.rejects(loadConfig(directory), error => !error.message.includes('private-test-value'));
});

test('installed PI SDK creates a restricted session against custom config without a model or desktop call', { timeout: 90000 }, async t => {
  const directory = await workspace(t);
  const old = process.env.OPENLESS_PI_AGENT_DIR;
  process.env.OPENLESS_PI_AGENT_DIR = directory;
  t.after(() => { if (old === undefined) delete process.env.OPENLESS_PI_AGENT_DIR; else process.env.OPENLESS_PI_AGENT_DIR = old; });
  await fs.writeFile(path.join(directory, 'config.json'), JSON.stringify({ provider: 'openless-test', model: 'mock-model', apiKey: 'test-only', baseUrl: 'http://127.0.0.1:1/v1' }), 'utf8');
  const session = await createSession(normalizeRequest({ prompt: 'do not run', cwd: directory, permission_mode: 'plan' }), AbortSignal.timeout(60000));
  try {
    const names = session.agent.state.tools.map(tool => tool.name);
    assert.ok(names.includes('computer_screenshot'));
    assert.ok(names.includes('read'));
    for (const name of ['bash', 'powershell', 'write', 'edit', 'computer_click', 'computer_type_text']) assert.equal(names.includes(name), false);
    assert.equal(session.model.provider, 'openless-test');
    assert.equal(session.model.id, 'mock-model');
  } finally { session.dispose(); }
});

test('real PI SDK calls a computer tool and sends its image to a local OpenAI-compatible server', { timeout: 90000 }, async t => {
  const directory = await workspace(t);
  const requests = [];
  const server = createServer(async (request, response) => {
    let body = '';
    for await (const chunk of request) body += chunk.toString('utf8');
    requests.push(JSON.parse(body));
    response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
    const chunk = (delta, finish_reason = null) => response.write(`data: ${JSON.stringify({
      id: 'test-completion', object: 'chat.completion.chunk', created: 1, model: 'mock-vision',
      choices: [{ index: 0, delta, finish_reason }],
    })}\n\n`);
    if (requests.length === 1) {
      chunk({ role: 'assistant', tool_calls: [{ index: 0, id: 'screen-call', type: 'function', function: { name: 'computer_screenshot', arguments: '{"monitor_id":8}' } }] });
      chunk({}, 'tool_calls');
    } else {
      chunk({ role: 'assistant', content: '已通过截图查看桌面。' });
      chunk({}, 'stop');
    }
    response.end('data: [DONE]\n\n');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const old = process.env.OPENLESS_PI_AGENT_DIR;
  process.env.OPENLESS_PI_AGENT_DIR = directory;
  t.after(() => { if (old === undefined) delete process.env.OPENLESS_PI_AGENT_DIR; else process.env.OPENLESS_PI_AGENT_DIR = old; });
  await fs.writeFile(path.join(directory, 'config.json'), JSON.stringify({
    provider: 'openless-test', model: 'mock-vision', apiKey: 'test-only', baseUrl: `http://127.0.0.1:${server.address().port}/v1`,
  }), 'utf8');
  const calls = [];
  const emit = [];
  const screenshot = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aSxkAAAAASUVORK5CYII=';
  const result = await runRequest({ prompt: '查看显示器 8', cwd: directory, permission_mode: 'plan' }, {
    emit: event => emit.push(event), signal: AbortSignal.timeout(60000),
    sessionFactory: (request, signal) => createSession(request, signal, {
      computerCall: async value => {
        calls.push(value);
        return { image_base64: screenshot, mime_type: 'image/png', width: 1, height: 1, monitor: { id: 8 } };
      },
    }),
  });
  assert.deepEqual(calls, [{ action: 'screenshot', monitor_id: 8 }]);
  assert.equal(requests.length, 2);
  assert.ok(requests[0].tools.some(tool => tool.function.name === 'computer_screenshot'));
  assert.ok(!requests[0].tools.some(tool => tool.function.name === 'computer_click' || tool.function.name === 'bash'));
  assert.ok(JSON.stringify(requests[1].messages).includes(`data:image/png;base64,${screenshot}`));
  assert.equal(result.text, '已通过截图查看桌面。');
  assert.equal(emit.filter(event => event.type === 'complete').length, 1);
  assert.equal(JSON.stringify(emit).includes(screenshot), false);

  // Exercise the real CLI transport with stdin intentionally held open, as a cancelling host would do.
  const child = spawn(process.execPath, [fileURLToPath(new URL('../index.mjs', import.meta.url)), '--request'], {
    cwd: directory, env: { ...process.env, OPENLESS_PI_AGENT_DIR: directory },
    stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true,
  });
  t.after(() => child.kill());
  const stdout = [];
  const stderr = [];
  child.stdout.on('data', chunk => stdout.push(chunk));
  child.stderr.on('data', chunk => stderr.push(chunk));
  child.stdin.write(JSON.stringify({ prompt: '文字回复', cwd: directory, permission_mode: 'plan' }) + '\n', 'utf8');
  const code = await new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', resolve);
  });
  assert.equal(code, 0, Buffer.concat(stderr).toString('utf8'));
  const cliEvents = Buffer.concat(stdout).toString('utf8').trim().split('\n').map(JSON.parse);
  assert.equal(cliEvents.at(-1).type, 'complete');
  assert.equal(cliEvents.at(-1).text, '已通过截图查看桌面。');
  assert.equal(cliEvents.some(event => event.type === 'error'), false);
});
