import * as fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';

export const SDK_VERSION = '0.85.1';
export function configDirectory(env = process.env, platform = process.platform, userHome = os.homedir()) {
  if (env.OPENLESS_PI_AGENT_DIR || env.OPENLESS_PI_HOME) return path.resolve(env.OPENLESS_PI_AGENT_DIR || env.OPENLESS_PI_HOME);
  if (platform === 'win32') return path.join(env.APPDATA || path.join(userHome, 'AppData', 'Roaming'), 'OpenLess', 'pi');
  if (platform === 'darwin') return path.join(userHome, 'Library', 'Application Support', 'OpenLess', 'pi');
  return path.join(env.XDG_CONFIG_HOME || path.join(userHome, '.config'), 'openless', 'pi');
}

export async function loadConfig(directory = configDirectory()) {
  const filename = path.join(directory, 'config.json');
  let value;
  try { value = JSON.parse(await fs.readFile(filename, 'utf8')); } catch (error) {
    if (error.code === 'ENOENT') return {};
    throw new Error(`Invalid PI configuration at ${filename}: ${error instanceof SyntaxError ? 'invalid JSON' : error.message}`);
  }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('PI config.json must be an object');
  for (const name of ['provider', 'model', 'apiKey', 'apiKeyEnv', 'baseUrl', 'api']) {
    if (value[name] != null && typeof value[name] !== 'string') throw new Error(`PI configuration ${name} must be a string`);
  }
  if (value.baseUrl) {
    const url = new URL(value.baseUrl);
    if (!['http:', 'https:'].includes(url.protocol)) throw new Error('PI baseUrl must be an HTTP(S) URL');
    if (!value.model) throw new Error('PI config.model is required for a custom baseUrl');
  }
  return value;
}

export async function configureModels(ModelRuntime, { directory = configDirectory(), signal } = {}) {
  await fs.mkdir(directory, { recursive: true, mode: 0o700 });
  const config = await loadConfig(directory);
  const runtime = await ModelRuntime.create({
    authPath: path.join(directory, 'auth.json'),
    modelsPath: path.join(directory, 'models.json'),
    modelsStorePath: path.join(directory, 'models-store.json'),
    allowModelNetwork: false,
    signal,
  });
  const provider = config.provider || (config.baseUrl ? 'openless' : undefined);
  if (config.baseUrl) {
    runtime.registerProvider(provider, {
      name: 'OpenLess configured provider', baseUrl: config.baseUrl, api: config.api || 'openai-completions', authHeader: true,
      models: [{
        id: config.model, name: config.model, reasoning: config.reasoning === true,
        input: config.supportsImages === false ? ['text'] : ['text', 'image'],
        contextWindow: Number.isSafeInteger(config.contextWindow) && config.contextWindow > 0 ? config.contextWindow : 128000,
        maxTokens: Number.isSafeInteger(config.maxTokens) && config.maxTokens > 0 ? config.maxTokens : 8192,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
      }],
    });
  }
  const apiKey = config.apiKeyEnv ? process.env[config.apiKeyEnv] : config.apiKey;
  if (apiKey) {
    if (!provider) throw new Error('PI config.provider is required when apiKey is configured');
    await runtime.setRuntimeApiKey(provider, apiKey, { signal });
  }
  return { runtime, config, defaultModel: provider && config.model ? `${provider}/${config.model}` : config.model };
}
