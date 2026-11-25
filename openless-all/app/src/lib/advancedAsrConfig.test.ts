import { parseAdvancedAsrConfig, serializeAdvancedAsrConfig } from './advancedAsrConfig';

function assertEqual(actual: unknown, expected: unknown, name: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

assertEqual(
  parseAdvancedAsrConfig(null),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'null raw falls back to conservative defaults',
);

assertEqual(
  parseAdvancedAsrConfig(''),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'empty raw falls back to conservative defaults',
);

assertEqual(
  parseAdvancedAsrConfig('not-json'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'invalid JSON falls back to conservative defaults',
);

assertEqual(
  parseAdvancedAsrConfig('[1,2]'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'non-object JSON falls back to conservative defaults',
);

assertEqual(
  parseAdvancedAsrConfig('{"verboseJson":true}'),
  { verboseJson: true, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'missing chunkDurationMs stays null',
);

assertEqual(
  parseAdvancedAsrConfig('{"chunkDurationMs":0}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'zero chunk duration means no chunking',
);

assertEqual(
  parseAdvancedAsrConfig('{"chunkDurationMs":30000.9,"verboseJson":false}'),
  { verboseJson: false, chunkDurationMs: 30000, enableItn: true, enableDdc: false, enableNonstream: false },
  'chunk duration is floored to integer',
);

assertEqual(
  parseAdvancedAsrConfig('{"verboseJson":"yes"}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: false, enableNonstream: false },
  'non-boolean verboseJson falls back to false',
);

assertEqual(
  parseAdvancedAsrConfig('{"enableItn":false}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: false, enableDdc: false, enableNonstream: false },
  'explicit false enableItn is honored',
);

assertEqual(
  parseAdvancedAsrConfig('{"enableDdc":true,"enableNonstream":true}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true, enableDdc: true, enableNonstream: true },
  'explicit true enableDdc and enableNonstream are honored',
);

assertEqual(
  parseAdvancedAsrConfig(
    serializeAdvancedAsrConfig({
      verboseJson: true,
      chunkDurationMs: 30000,
      enableItn: false,
      enableDdc: true,
      enableNonstream: true,
    }),
  ),
  { verboseJson: true, chunkDurationMs: 30000, enableItn: false, enableDdc: true, enableNonstream: true },
  'serialize/parse round-trip preserves config',
);
