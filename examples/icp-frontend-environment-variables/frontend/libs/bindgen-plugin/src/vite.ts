import { PLUGIN_NAME } from ".";
import { generate } from "./core/generate";
import type { Options } from ".";
import { type Plugin } from "vite";
import { watchDidFileChanges } from "./core/watch";

export function icpBindgen(options: Options): Plugin {
  return {
    name: PLUGIN_NAME,
    async buildStart() {
      await generate(options);
    },
    configureServer(server) {
      if (!options.disableWatch) {
        watchDidFileChanges(server, options);
      }
    },
    sharedDuringBuild: true,
  };
}

export type { Options };
