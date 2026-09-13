import { downloadMarketplacePack } from './ipc/marketplace';

const result = await downloadMarketplacePack(
  '00000000-0000-0000-0000-000000000001',
  '~/Downloads/demo-pack.zip',
);

if (result !== undefined) {
  throw new Error('browser development mode should complete downloads through its mock boundary');
}

console.log('marketplaceDownload tests passed');
