import { createVitePlugin } from "unplugin";
import { unpluginFactory } from "./core/factory";

/**
 * @example
 * ```ts
 * export default defineConfig({
 *   plugins: [icpBindgen()],
 *   // ...
 * })
 * ```
 */
export const icpBindgen = createVitePlugin(unpluginFactory);
