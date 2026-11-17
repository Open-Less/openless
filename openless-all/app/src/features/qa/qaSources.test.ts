import { qaSources } from './qaSources';
const assert = (value: unknown, message: string) => {
  if (!value) throw new Error(message);
};
const parsed = qaSources('Answer.\n\n[1](<https://example.com/a>) [2](<https://example.org/b>) ');
assert(parsed.body === 'Answer.', 'move the native citation footer out of the article body');
assert(
  parsed.sources.length === 2 && parsed.sources[0].host === 'example.com',
  'retain source URLs and website identity',
);
const links = qaSources(
  '[article](https://example.com/a) [again](https://example.com/a) [unsafe](javascript:alert) [secret](https://user:pass@example.com)',
);
assert(
  links.sources.length === 1,
  'deduplicate references and reject unsafe or credential-bearing links',
);
assert(links.body.includes('[article]'), 'retain inline links in the article');
console.log('QA source footer parsing passed');
