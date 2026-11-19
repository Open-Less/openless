import { createCapsuleRegionPublisher } from './capsuleInput';

const first = [{ left: 10, top: 210, width: 100, height: 25 }];
const menu = [{ left: 10, top: 18, width: 100, height: 210 }];
const calls: (typeof first)[] = [];
let finishFirst!: () => void;
let didStart!: () => void;
const started = new Promise<void>((resolve) => {
  didStart = resolve;
});
const publish = createCapsuleRegionPublisher(async (rects) => {
  calls.push(rects);
  if (calls.length === 1)
    await new Promise<void>((resolve) => {
      finishFirst = resolve;
      didStart();
    });
});

const opening = publish(first);
await started;
const pendingMenu = publish(menu);
const teardown = publish([]);
finishFirst();
await Promise.all([opening, pendingMenu, teardown]);

if (calls.length !== 2 || calls[0] !== first || calls[1].length !== 0) {
  throw new Error('Teardown must supersede a queued popup region');
}
console.log('Capsule input writes serialized with only the most recent pending frame retained');
