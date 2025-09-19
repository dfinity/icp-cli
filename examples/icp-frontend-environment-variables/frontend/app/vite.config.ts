import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { icpBindgen } from "@icp-sdk/bindgen/plugins/vite";

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    react(),
    icpBindgen({
      didFile: "../../backend/dist/hello_world.did",
      outDir: "./src/backend/api",
      additionalFeatures: {
        canisterEnv: {
          variableNames: ["ICP_CANISTER_ID:backend"],
        },
      },
    }),
  ],
});
