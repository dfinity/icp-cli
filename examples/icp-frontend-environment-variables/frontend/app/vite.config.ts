import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { icpBindgen } from "@icp-sdk/bindgen/plugins/vite";
import { execSync } from "child_process";

// Target environment (default: "local")
// Usage: ICP_ENVIRONMENT=staging npm run dev
const environment = process.env.ICP_ENVIRONMENT || "local";

// Canister name (used for canister ID lookup and binding generation paths)
const CANISTER_NAME = "backend";

export default defineConfig(({ command }) => {
  // Dev server configuration - only needed for `npm run dev`
  // When running `icp deploy`, the asset canister sets the ic_env cookie automatically
  if (command === "serve") {
    // Get network configuration (root key and URL) for the target environment
    const networkStatus = JSON.parse(
      execSync(`icp network status -e ${environment} --json`, { encoding: "utf-8" })
    );
    const rootKey: string = networkStatus.root_key;
    // `api_url` is not yet supported by the CLI, but will be added in a future release
    // const proxyTarget: string = networkStatus.api_url;
    const proxyTarget: string = `http://localhost:8000`;

    // Get canister ID for the target environment (-i outputs just the ID)
    // Note: Backend MUST be deployed before running dev server
    let canisterId: string;
    try {
      canisterId = execSync(`icp canister status ${CANISTER_NAME} -e ${environment} -i`, {
        encoding: "utf-8",
      }).trim();
    } catch {
      console.error(`
❌ Backend canister "${CANISTER_NAME}" not found in environment "${environment}"

   Before running the dev server, deploy the backend canister:

     icp deploy ${CANISTER_NAME} -e ${environment}
`);
      process.exit(1);
    }

    // Log configuration for debugging
    console.log(`
🌐 ICP Dev Server Configuration

   Environment:         ${environment}
   Backend Canister ID: ${canisterId}
   IC API URL:          ${proxyTarget}
   IC Root Key:         ${rootKey.slice(0, 20)}...${rootKey.slice(-20)}
`);

    return {
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
    };
  }

  // Build mode configuration - used by `icp deploy`
  // Asset canister will handle ic_env cookie automatically, no dev server needed
  return {
    plugins: [
      react(),
      icpBindgen({
        didFile: `../../${CANISTER_NAME}/dist/hello_world.did`,
        outDir: `./src/${CANISTER_NAME}/api`,
      }),
    ],
  };
});
