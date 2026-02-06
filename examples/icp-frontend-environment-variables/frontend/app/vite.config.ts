import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { icpBindgen } from "@icp-sdk/bindgen/plugins/vite";
import { execSync } from "child_process";

// Target environment (default: "local")
// Usage: ICP_ENVIRONMENT=staging npm run dev
const environment = process.env.ICP_ENVIRONMENT || "local";

// Canister name (used for canister ID lookup and binding generation paths)
const CANISTER_NAME = "backend";

// Get network configuration (root key and URL) for the target environment.
// Works for both managed networks (local) and connected networks (IC mainnet).
const networkStatus = JSON.parse(
  execSync(`icp network status -e ${environment} --json`, { encoding: "utf-8" })
);
const rootKey: string = networkStatus.root_key;
const proxyTarget: string = networkStatus.api_url;

// Get canister ID for the target environment (-i outputs just the ID)
const canisterId = execSync(`icp canister status ${CANISTER_NAME} -e ${environment} -i`, {
  encoding: "utf-8",
}).trim();

// Log configuration for debugging
console.log(`
🌐 ICP Dev Server Configuration

   Environment:         ${environment}
   Backend Canister ID: ${canisterId}
   IC API URL:          ${proxyTarget}
   IC Root Key:         ${rootKey.slice(0, 20)}...${rootKey.slice(-20)}
`);

export default defineConfig({
  plugins: [
    react(),
    icpBindgen({
      // Path to the backend's Candid interface file (generated during build)
      didFile: `../../${CANISTER_NAME}/dist/hello_world.did`,
      // Output directory for generated TypeScript bindings
      outDir: `./src/${CANISTER_NAME}/api`,
    }),
  ],
  server: {
    headers: {
      // Set the ic_env cookie with canister ID and root key.
      // This mimics what the asset canister does in production.
      // Note: ic_root_key must be lowercase - the library expects this format
      // and converts it to uppercase IC_ROOT_KEY in the returned object.
      "Set-Cookie": `ic_env=${encodeURIComponent(
        `PUBLIC_CANISTER_ID:${CANISTER_NAME}=${canisterId}&ic_root_key=${rootKey}`
      )}; SameSite=Lax;`,
    },
    proxy: {
      // Proxy API requests to the target network.
      // The agent sends requests to /api/... which need to be forwarded.
      "/api": {
        target: proxyTarget,
        changeOrigin: true,
      },
    },
  },
});
