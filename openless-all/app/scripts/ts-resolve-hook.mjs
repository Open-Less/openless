// ESM resolve hook: let Node import the Tauri i18n TypeScript modules, which
// use extension-less relative specifiers (`./en`) the way a bundler allows but
// Node's ESM resolver does not.
export async function resolve(specifier, context, nextResolve) {
  if (specifier.startsWith('.') && !/\.[cm]?[jt]sx?$/.test(specifier)) {
    try {
      return await nextResolve(`${specifier}.ts`, context);
    } catch {
      // fall through to the default resolution for a clearer error
    }
  }
  return nextResolve(specifier, context);
}
