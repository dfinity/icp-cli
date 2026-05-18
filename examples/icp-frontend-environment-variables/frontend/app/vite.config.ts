import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { icpBindgen } from "@icp-sdk/bindgen/plugins/vite";
import { execSync } from "child_process";

// Usage: ICP_ENVIRONMENT=staging npm run dev
const environment = process.env.ICP_ENVIRONMENT || "local";
const CANISTER_NAME = "backend";

export default defineConfig(({ command }) => {
  const plugins = [
    react(),
    icpBindgen({
      didFile: `../../${CANISTER_NAME}/dist/hello_world.did`,
      outDir: `./src/${CANISTER_NAME}/api`,
    }),
  ];

  // Build mode: asset canister handles ic_env cookie automatically
  if (command !== "serve") {
    return { plugins };
  }

  // Dev server mode: configure ic_env cookie and proxy
  const networkStatus = JSON.parse(
    execSync(`icp network status -e ${environment} --json`, { encoding: "utf-8" })
  );
  const rootKey: string = networkStatus.root_key;
  const proxyTarget: string = networkStatus.api_url;

  // Backend must be deployed before starting dev server
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

  console.log(`
🌐 ICP Dev Server Configuration

   Environment:         ${environment}
   Backend Canister ID: ${canisterId}
   IC API URL:          ${proxyTarget}
   IC Root Key:         ${rootKey.slice(0, 20)}...${rootKey.slice(-20)}
`);

  return {
    plugins,
    server: {
      headers: {
        // Note: ic_root_key must be lowercase - library converts to uppercase IC_ROOT_KEY
        "Set-Cookie": `ic_env=${encodeURIComponent(
          `PUBLIC_CANISTER_ID:${CANISTER_NAME}=${canisterId}&ic_root_key=${rootKey}`
        )}; SameSite=Lax;`,
      },
      proxy: {
        "/api": {
          target: proxyTarget,
          changeOrigin: true,
        },
      },
    },
  };
});
