export function getEnvVarNames(): string[] {
  return Object.keys(process.env).filter((key) =>
    key.startsWith("ICP_CANISTER_ID")
  );
}
