// Registers the TypeScript resolve hook so scripts can import `src/i18n/*.ts`.
import { register } from 'node:module';

register('./ts-resolve-hook.mjs', import.meta.url);
