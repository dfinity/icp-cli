export interface Options {
  /**
   * The path to the `.did` file.
   */
  didFile: string;
  /**
   * The path to the directory where the bindings will be generated.
   */
  bindingsOutDir: string;
  /**
   * Disables watching for changes in the `.did` file when using the dev server.
   *
   * @default false
   */
  disableWatch?: boolean;
}

export const PLUGIN_NAME = "vite-plugin-icp-bindgen";
