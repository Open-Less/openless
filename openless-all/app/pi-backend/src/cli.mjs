import path from 'node:path';
import { format } from 'node:util';
import { configDirectory, configureModels, SDK_VERSION } from './config.mjs';
import { callComputer } from './computer.mjs';
import { jsonLines } from './protocol.mjs';

export async function main() {
  const emit = value => process.stdout.write(`${JSON.stringify(value)}\n`, 'utf8');
  // stdout belongs exclusively to our protocol, including during SDK initialization.
  console.log = (...args) => process.stderr.write(`${format(...args)}\n`, 'utf8');
  const flag = process.argv[2];
  try {
    if (flag === '--version') {
      process.stdout.write(`OpenLess PI 0.1.0 (pi-coding-agent ${SDK_VERSION})\n`, 'utf8');
      return;
    }
    if (flag === '--health') {
      const sdk = await import('@earendil-works/pi-coding-agent');
      if (typeof sdk.createAgentSession !== 'function') throw new Error('PI SDK is unavailable');
      emit({ ok: true, sdk_version: SDK_VERSION, node_version: process.versions.node });
      return;
    }
    if (flag === '--capabilities') {
      let computer_status;
      try { computer_status = await callComputer({ action: 'capabilities' }, { timeoutMs: 8000 }); }
      catch (error) { computer_status = { available: false, error: error.message }; }
      emit({
        protocol_version: 1, sdk_version: SDK_VERSION,
        computer: computer_status.available === true || computer_status.supported === true || computer_status.can_capture === true,
        computer_status, config_dir: configDirectory(), config_file: path.join(configDirectory(), 'config.json'),
        permission_modes: ['plan', 'acceptEdits'],
      });
      return;
    }
    if (flag === '--list-models') {
      const { ModelRuntime } = await import('@earendil-works/pi-coding-agent');
      const { runtime } = await configureModels(ModelRuntime, { signal: AbortSignal.timeout(20000) });
      const models = runtime.getModels();
      process.stdout.write(models.map(model => `${model.provider}/${model.id}`).join('\n') + (models.length ? '\n' : ''), 'utf8');
      return;
    }
    if (flag && flag !== '--request') throw new Error(`Unknown option: ${flag}`);
    const controller = new AbortController();
    let pending;
    let timer;
    let received = false;
    let finished = false;
    const interrupt = () => controller.abort(new Error('PI request cancelled'));
    process.once('SIGTERM', interrupt);
    process.once('SIGINT', interrupt);
    try {
      for await (const value of jsonLines(process.stdin)) {
        if (value?.type === 'cancel') { interrupt(); continue; }
        if (received) throw new Error('Only one prompt is accepted per process');
        received = true;
        const timeout = Number.isFinite(value?.timeout_secs) ? Math.max(1, Math.min(3600, value.timeout_secs)) : 300;
        timer = setTimeout(() => controller.abort(new Error('PI request timed out')), timeout * 1000);
        const { runRequest } = await import('./runtime.mjs');
        pending = runRequest(value, { emit, signal: controller.signal });
        // Attach immediately; input remains readable for a cancellation record.
        pending.catch(() => {});
        pending.finally(() => { finished = true; process.stdin.destroy(); }).catch(() => {});
      }
      if (!pending) throw new Error('Expected one prompt JSON record on stdin');
      await pending;
    } catch (error) {
      // A completed run closes an otherwise idle stdin, which async iteration reports as premature close.
      if (finished && (error.code === 'ERR_STREAM_PREMATURE_CLOSE' || error.code === 'ABORT_ERR')) {
        await pending;
        return;
      }
      controller.abort(error);
      await pending?.catch(() => {});
      throw error;
    } finally {
      clearTimeout(timer);
      process.removeListener('SIGTERM', interrupt);
      process.removeListener('SIGINT', interrupt);
    }
  } catch (error) {
    emit({ type: 'error', message: error?.message || String(error) });
    process.exitCode = 1;
  }
}
