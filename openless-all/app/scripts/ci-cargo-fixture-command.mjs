import { spawnSync } from 'node:child_process';

export function createCargoFixtureRunner(
  directory,
  env,
  { platform = process.platform, spawn = spawnSync, report = console.error } = {},
) {
  let first = true;
  return (args) => {
    // The first isolated Windows test-profile link exceeded 30s on hosted CI.
    // Give only that cold invocation 15s extra; do not retry or enlarge later runs.
    const timeout = first && platform === 'win32' ? 45_000 : 30_000;
    first = false;
    const commandArgs = [...args, '--offline'];
    const result = spawn('cargo', commandArgs, {
      cwd: directory,
      env,
      encoding: 'utf8',
      timeout,
    });
    if (result.error) {
      report(
        `[cargo-fixture] ${JSON.stringify({
          args: commandArgs,
          timeout,
          code: result.error.code,
          status: result.status,
          signal: result.signal,
          stdout: result.stdout,
          stderr: result.stderr,
        })}`,
      );
    }
    // Preserve the exact spawn result/error for expectSuccess and cleanup aggregation.
    return result;
  };
}
