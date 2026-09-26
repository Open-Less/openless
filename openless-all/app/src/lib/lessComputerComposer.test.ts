import {
  claimDictationResult,
  mergeDictation,
  transcriptTail,
  voiceHintKey,
} from './lessComputerComposer';

function assert(condition: unknown, name: string): asserts condition {
  if (!condition) throw new Error(name);
}
assert.equal = (actual: unknown, expected: unknown, name = 'value') => {
  if (actual !== expected)
    throw new Error(`${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
};

assert.equal(mergeDictation('', '  打开设置 '), '打开设置', 'an empty draft takes the transcript');
assert.equal(
  mergeDictation('请帮我', '打开设置'),
  '请帮我打开设置',
  'CJK text joins without a space',
);
assert.equal(mergeDictation('open', 'settings'), 'open settings', 'Latin words get one space');
assert.equal(mergeDictation('open ', 'settings'), 'open settings', 'existing whitespace is kept');
assert.equal(mergeDictation('first line\n', 'second'), 'first line\nsecond');
assert.equal(mergeDictation('done', ', thanks'), 'done, thanks', 'punctuation attaches directly');
assert.equal(mergeDictation('draft', '   '), 'draft', 'silence leaves the draft untouched');

assert.equal(transcriptTail('  hello   world  '), 'hello world');
const long = '一二三四五六七八九十'.repeat(12);
const tail = transcriptTail(long, 20);
assert.equal(Array.from(tail).length, 21, 'tail keeps the requested characters plus an ellipsis');
assert(tail.startsWith('…') && long.endsWith(tail.slice(1)), 'the newest words stay visible');

assert.equal(voiceHintKey('hold'), 'holdHint');
assert.equal(voiceHintKey('toggle'), 'toggleHint');
assert.equal(voiceHintKey('doubleClick'), 'toggleHint', 'double click behaves like one toggle');
assert.equal(voiceHintKey('auto'), 'autoHint');

const session = `dictation-${Date.now()}`;
assert.equal(claimDictationResult(session), true, 'the first observer applies the result');
assert.equal(claimDictationResult(session), false, 'replays and remounts never apply it twice');
assert.equal(claimDictationResult(`${session}-next`), true, 'a new dictation is independent');

console.log('lessComputerComposer.test.ts passed');
