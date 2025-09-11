import { hexToBytes } from "@noble/hashes/utils";

const IC_ENV_COOKIE_NAME = "ic_env";

const ENV_VAR_SEPARATOR = "&";
const ENV_VAR_ASSIGNMENT_SYMBOL = "=";

const IC_ROOT_KEY_VALUE_NAME = "ic_root_key";

type GetCanisterEnvOptions = {
  cookieName?: string;
};

export function getCanisterEnv(
  options: GetCanisterEnvOptions = {}
): CanisterEnv {
  const { cookieName = IC_ENV_COOKIE_NAME } = options;

  const encodedEnvVars = getEncodedEnvVarsFromCookie(cookieName);
  if (!encodedEnvVars) {
    throw new Error("No environment variables found in cookie");
  }

  const decodedEnvVars = decodeURIComponent(encodedEnvVars);
  const envVars = getEnvVars(decodedEnvVars);

  return envVars;
}

function getEncodedEnvVarsFromCookie(cookieName: string): string | undefined {
  return document.cookie
    .split(";")
    .find((cookie) => cookie.trim().startsWith(`${cookieName}=`))
    ?.split("=")[1]
    .trim();
}

function getEnvVars(decoded: string): CanisterEnv {
  const entries = decoded.split(ENV_VAR_SEPARATOR).map((v) => {
    // we only want to split at the first occurrence of the assignment symbol
    const symbolIndex = v.indexOf(ENV_VAR_ASSIGNMENT_SYMBOL);

    const key = v.slice(0, symbolIndex);
    const value = v.substring(symbolIndex + 1);

    if (key === IC_ROOT_KEY_VALUE_NAME) {
      return [key, hexToBytes(value)];
    }

    return [key, value];
  });

  return Object.fromEntries(entries);
}
