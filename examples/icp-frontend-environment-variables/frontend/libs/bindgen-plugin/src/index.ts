import { createUnplugin } from "unplugin";
import { unpluginFactory } from "./core/factory";

export const unplugin = /* #__PURE__ */ createUnplugin(unpluginFactory) as any;

export default unplugin;
