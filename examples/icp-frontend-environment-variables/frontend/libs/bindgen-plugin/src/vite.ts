import { generate } from "./core/generate";
import type { Options } from "./core/types";
import { type Plugin } from "vite";

export function icpBindgen(options: Options): Plugin {
  return {
    name: "vite-plugin-icp-bindgen",
    async buildStart() {
      await generate(options);
    },
    sharedDuringBuild: true,
  };
}

export type { Options };
