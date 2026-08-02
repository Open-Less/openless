import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));
const marketplaceSource = readFileSync(join(appRoot, 'src/pages/Marketplace.tsx'), 'utf8');

if (marketplaceSource.includes('await logClientError(')) {
  throw new Error('Marketplace install must not await client error logging');
}
if (!marketplaceSource.includes('void logClientError(clientFailureLog)')) {
  throw new Error('Marketplace install failure logging must be fire-and-forget');
}

console.log('marketplace-install-contract.test.mjs passed');
