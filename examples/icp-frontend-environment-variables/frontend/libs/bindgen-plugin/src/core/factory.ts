import type { UnpluginFactory } from "unplugin";
import type { Options } from "./types";

export const unpluginFactory: UnpluginFactory<Options | undefined> = (
  options
) => ({
  name: "unplugin-starter",
  transformInclude(id) {
    return id.endsWith("main.ts");
  },
  transform(code) {
    return code.replace("__UNPLUGIN__", `Hello Unplugin! ${options}`);
  },
});
