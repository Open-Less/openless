export const initBackend: (dataDir: string, cacheDir: string) => string;
export const invoke: (command: string, args: string) => string;
export const invokeAsync: (command: string, args: string) => Promise<string>;
export const rimeAsync: (operation: string, session: string, argument: string) => Promise<string>;
export const feedPcm: (session: string, pcm: ArrayBuffer) => Promise<string>;
