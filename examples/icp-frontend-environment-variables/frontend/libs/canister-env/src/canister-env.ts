const IC_ENV_COOKIE_NAME = "ic_env";

const ENV_VAR_SEPARATOR = ",";
const ENV_VAR_ASSIGNMENT_SYMBOL = "=";

type GetCanisterEnvOptions = {
  cookieName?: string;
};

export function getCanisterEnv(
  options: GetCanisterEnvOptions = {}
): CanisterEnv {
  const { cookieName = IC_ENV_COOKIE_NAME } = options;

  const encodedEnvVars = getEncodedEnvVarsFromCookie(cookieName);
  if (!encodedEnvVars) {
    return {};
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
  const entries = decoded.split(ENV_VAR_SEPARATOR).map((value) => {
    // we only want to split at the first occurrence of the assignment symbol
    const symbolIndex = value.indexOf(ENV_VAR_ASSIGNMENT_SYMBOL);

    return [value.slice(0, symbolIndex), value.substring(symbolIndex + 1)];
  });

  return Object.fromEntries(entries);
}
