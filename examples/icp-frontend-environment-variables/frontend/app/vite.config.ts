import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { icpBindgen } from "@icp-sdk/bindgen-plugin/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    icpBindgen({
      didFile: "../../backend/dist/hello_world.did",
      bindingsOutDir: "./src/backend/api",
    }),
  ],
});
