import {
  parseAdvancedAsrConfig,
  serializeAdvancedAsrConfig,
} from './advancedAsrConfig'

function assertEqual(actual: unknown, expected: unknown, name: string) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`)
  }
}

assertEqual(
  parseAdvancedAsrConfig(null),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'null raw falls back to conservative defaults',
)

assertEqual(
  parseAdvancedAsrConfig(''),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'empty raw falls back to conservative defaults',
)

assertEqual(
  parseAdvancedAsrConfig('not-json'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'invalid JSON falls back to conservative defaults',
)

assertEqual(
  parseAdvancedAsrConfig('[1,2]'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'non-object JSON falls back to conservative defaults',
)

assertEqual(
  parseAdvancedAsrConfig('{"verboseJson":true}'),
  { verboseJson: true, chunkDurationMs: null, enableItn: true },
  'missing chunkDurationMs stays null',
)

assertEqual(
  parseAdvancedAsrConfig('{"chunkDurationMs":0}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'zero chunk duration means no chunking',
)

assertEqual(
  parseAdvancedAsrConfig('{"chunkDurationMs":30000.9,"verboseJson":false}'),
  { verboseJson: false, chunkDurationMs: 30000, enableItn: true },
  'chunk duration is floored to integer',
)

assertEqual(
  parseAdvancedAsrConfig('{"verboseJson":"yes"}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: true },
  'non-boolean verboseJson falls back to false',
)

assertEqual(
  parseAdvancedAsrConfig('{"enableItn":false}'),
  { verboseJson: false, chunkDurationMs: null, enableItn: false },
  'explicit false enableItn is honored',
)

assertEqual(
  parseAdvancedAsrConfig(serializeAdvancedAsrConfig({
    verboseJson: true,
    chunkDurationMs: 30000,
    enableItn: false,
  })),
  { verboseJson: true, chunkDurationMs: 30000, enableItn: false },
  'serialize/parse round-trip preserves config',
)
