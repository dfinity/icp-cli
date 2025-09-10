import { defineConfig, mergeConfig } from "vite";
import { tanstackViteConfig } from "@tanstack/config/vite";

const config = defineConfig({});

export default mergeConfig(
  config,
  tanstackViteConfig({
    entry: ["./src/index.ts", "./src/vite.ts"],
    srcDir: "./src",
    outDir: "./dist",
    tsconfigPath: "./tsconfig.json",
  })
);
