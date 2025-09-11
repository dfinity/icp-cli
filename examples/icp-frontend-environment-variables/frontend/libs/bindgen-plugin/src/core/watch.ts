import { resolve } from "node:path";
import { type ViteDevServer } from "vite";
import { type Options } from "..";
import { generate } from "./generate";

export function watchDidFileChanges(server: ViteDevServer, options: Options) {
  const didFilePath = resolve(options.didFile);
  server.watcher.add(didFilePath);

  server.watcher.on("change", async (changedPath) => {
    if (resolve(changedPath) === resolve(didFilePath)) {
      await generate(options);
    }
  });
}
