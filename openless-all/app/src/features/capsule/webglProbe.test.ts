import { isWebGLAvailable } from './SiriGL';

let contexts = 0;
let released = 0;
const original = Object.getOwnPropertyDescriptor(globalThis, 'document');
Object.defineProperty(globalThis, 'document', {
  configurable: true,
  value: {
    createElement: () => ({
      getContext: () => {
        contexts += 1;
        return {
          getExtension: (name: string) => {
            if (name !== 'WEBGL_lose_context') throw new Error('Wrong resource released');
            return {
              loseContext: () => {
                released += 1;
              },
            };
          },
        };
      },
    }),
  },
});

try {
  if (!isWebGLAvailable() || !isWebGLAvailable() || contexts !== 1 || released !== 1) {
    throw new Error(
      'The WebGL capability probe must allocate once and release its temporary context',
    );
  }
} finally {
  if (original) Object.defineProperty(globalThis, 'document', original);
  else Reflect.deleteProperty(globalThis, 'document');
}
