import { createVitePlugin } from "unplugin";

/**
 * @example
 * ```ts
 * export default defineConfig({
 *   plugins: [icpBindgen()],
 *   // ...
 * })
 * ```
 */
export const icpBindgen = createVitePlugin();
