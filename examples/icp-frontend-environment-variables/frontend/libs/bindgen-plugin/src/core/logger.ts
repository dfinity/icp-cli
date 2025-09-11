import { PLUGIN_NAME } from "..";

export const logger = {
  info: (...args: any[]) => {
    console.log(`[${PLUGIN_NAME}]`, ...args);
  },
  error: (...args: any[]) => {
    console.error(`[${PLUGIN_NAME}]`, ...args);
  },
};
