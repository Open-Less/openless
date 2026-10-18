import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import yaml from 'js-yaml';

const workflow = yaml.load(
  readFileSync(new URL('../../../.github/workflows/release-tauri.yml', import.meta.url), 'utf8'),
);
assert.ok(workflow.on.workflow_dispatch.inputs.platform.options.includes('windows'));
assert.equal(workflow.on.workflow_dispatch.inputs.release_tag.default, '');
const job = workflow.jobs.build;
assert.match(job.strategy.matrix.include, /inputs.platform == 'windows'/);
const steps = job.steps;
const checkout = steps.find((s) => s.uses === 'actions/checkout@v4');
assert.equal(checkout.with.ref, '${{ inputs.release_tag || github.ref }}');
const guard = steps.find((s) => s.name === 'Validate targeted draft release repair');
assert.equal(guard.if, "inputs.release_tag != ''");
assert.match(guard.run, /permission.*admin/);
assert.match(guard.run, /isDraft,isPrerelease/);
assert.match(guard.run, /INPUT_PLATFORM.*windows/);
const source = steps.find((s) => s.name === 'Verify repair source CI and version');
assert.match(source.run, /git rev-parse HEAD/);
assert.match(source.run, /head_sha=\$source_sha/);
assert.match(source.run, /total_count == 7/);
const build = steps.find((s) => s.name === 'Build (Windows)');
const script = build.run.match(/node <<'MSI_CONFIG'\n([\s\S]*?)\nMSI_CONFIG/)[1];
const originalVersion = (version, signing = false) => {
  const writes = new Map();
  const fakeFs = {
    readFileSync: (path) => {
      assert.equal(path, 'src-tauri/tauri.conf.json');
      return JSON.stringify({ version });
    },
    writeFileSync: (path, data) => writes.set(path, JSON.parse(data)),
  };
  vm.runInNewContext(script, {
    require: (name) => {
      assert.equal(name, 'node:fs');
      return fakeFs;
    },
    process: { env: signing ? { TAURI_SIGNING_PRIVATE_KEY: 'test-only' } : {} },
  });
  return writes.get('src-tauri/tauri.msi-version.json');
};
for (const version of ['2.0.0+build.20261016', '1.3.18', '2.0.0-Beta.6+build.20261015']) {
  const config = originalVersion(version, true);
  assert.equal(config.bundle.windows.wix.version, version.split(/[+-]/)[0] + '.0');
  assert.equal(config.bundle.createUpdaterArtifacts, true);
  assert.equal(
    Object.hasOwn(config, 'version'),
    false,
    'Do not replace the full application/build version',
  );
}
assert.equal(originalVersion('2.0.0+build.20261016').bundle.createUpdaterArtifacts, undefined);
for (const version of ['256.0.0', '1.256.0', '1.0.65536', 'invalid'])
  assert.throws(() => originalVersion(version));
assert.equal(
  (build.run.match(/build --bundles msi --config src-tauri\/tauri.msi-version.json/g) ?? []).length,
  2,
);
const publisher = steps.find((s) => s.uses?.startsWith('softprops/action-gh-release@'));
assert.equal(publisher.with.tag_name, '${{ env.OPENLESS_RELEASE_TAG }}');
assert.equal(publisher.with.draft, true);
assert.ok(steps.find((s) => s.name === 'Confirm repair target remains draft'));
const smokeTooling = steps.find((s) => s.name === 'Checkout installer smoke tooling');
assert.equal(smokeTooling.with.ref, '${{ github.workflow_sha }}');
assert.equal(smokeTooling.with['sparse-checkout'], 'openless-all/app/scripts/windows-ime-install-smoke.ps1');
assert.equal(smokeTooling.with['persist-credentials'], false);
const smoke = steps.find((s) => s.name === 'Verify Windows installers register TSF IME');
assert.match(smoke.run, /-File \$smokeScript/);
assert.ok(steps.indexOf(smokeTooling) < steps.indexOf(smoke));
assert.equal(smoke.if, "matrix.platform == 'windows-latest'");
assert.match(smoke.run, /DisableAutomaticApplicationShutdown/);
assert.match(smoke.run, /-PropertyType DWord -Value 1 -Force/);
assert.ok(smoke.run.indexOf('-Value 1 -Force') < smoke.run.indexOf('-InstallerKind nsis'));
assert.match(smoke.run, /finally[\s\S]*-Value \$previous[\s\S]*Remove-ItemProperty/);
for (const kind of ['nsis', 'msi']) {
  assert.match(smoke.run, new RegExp('-InstallerKind ' + kind));
  assert.match(smoke.run, new RegExp(kind.toUpperCase() + ' installer smoke failed'));
}
assert.doesNotMatch(smoke.run, /continue-on-error|SkipUninstall|exit 0/);
const smokeSource = readFileSync(new URL('windows-ime-install-smoke.ps1', import.meta.url), 'utf8');
assert.match(smokeSource, /\[int\[\]\]\$SuccessExitCodes = @\(0\)/);
assert.match(smokeSource, /\$process.ExitCode -notin \$SuccessExitCodes/);
assert.match(smokeSource, /-Label "MSI install" -SuccessExitCodes @\(0, 3010\)/);
assert.match(smokeSource, /-Label "MSI uninstall" -SuccessExitCodes @\(0, 3010\)/);
assert.doesNotMatch(smokeSource, /-Label "NSIS (?:install|uninstall)" -SuccessExitCodes/);
assert.ok(smokeSource.indexOf('$installRoot = Assert-OpenLessImeInstalled') > smokeSource.indexOf('-Label "MSI install"'));
if (process.platform === 'win32') {
  const runtime = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', fileURLToPath(new URL('windows-ime-install-smoke.test.ps1', import.meta.url))], { encoding: 'utf8' });
  assert.equal(runtime.status, 0, runtime.stdout + runtime.stderr);
  console.log(runtime.stdout.trim());
}
const schema = JSON.parse(
  readFileSync(
    new URL('../node_modules/@tauri-apps/cli/config.schema.json', import.meta.url),
    'utf8',
  ),
);
assert.ok(schema.definitions.WixConfig.properties.version.type.includes('string'));
if (process.platform !== 'win32') {
  for (const step of steps.filter((s) => s.shell === 'bash' && s.run)) {
    const result = spawnSync('/bin/bash', ['-n'], { input: step.run, encoding: 'utf8' });
    assert.equal(result.status, 0, `${step.name}: ${result.stderr}`);
  }
}
console.log('windows-msi-release-contract.test.mjs passed');
