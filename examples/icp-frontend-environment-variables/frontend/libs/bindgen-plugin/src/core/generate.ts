import * as bindgen from "@icp-sdk/bindgen";
import type { Options } from "./types";
import { emptyDir, ensureDir } from "./fs";
import { resolve, basename } from "node:path";
import { writeFile } from "node:fs/promises";
import { prepareBinding } from "./bindings";

const DID_FILE_EXTENSION = ".did";

export async function generate(options: Options) {
  const { didFile, bindingsOutDir } = options;
  const didFilePath = resolve(didFile);
  const outputFileName = basename(didFile, DID_FILE_EXTENSION);

  await emptyDir(bindingsOutDir);
  await ensureDir(bindingsOutDir);
  await ensureDir(resolve(bindingsOutDir, "declarations"));

  const result = bindgen.generate(didFilePath);

  await writeBindings({
    bindings: result,
    bindingsOutDir,
    outputFileName,
  });

  console.log("ICP Bindings generated successfully at", bindingsOutDir);
}

type WriteBindingsOptions = {
  bindings: bindgen.GenerateResult;
  bindingsOutDir: string;
  outputFileName: string;
};

export async function writeBindings({
  bindings,
  bindingsOutDir,
  outputFileName,
}: WriteBindingsOptions) {
  const declarationsTsFile = resolve(
    bindingsOutDir,
    "declarations",
    `${outputFileName}.did.d.ts`
  );
  const declarationsJsFile = resolve(
    bindingsOutDir,
    "declarations",
    `${outputFileName}.did.js`
  );
  const interfaceTsFile = resolve(bindingsOutDir, `${outputFileName}.d.ts`);
  const serviceTsFile = resolve(bindingsOutDir, `${outputFileName}.ts`);

  const declarationsTs = prepareBinding(bindings.declarations_ts);
  const declarationsJs = prepareBinding(bindings.declarations_js);
  const interfaceTs = prepareBinding(bindings.interface_ts);
  const serviceTs = prepareBinding(bindings.service_ts);

  await writeFile(declarationsTsFile, declarationsTs);
  await writeFile(declarationsJsFile, declarationsJs);
  await writeFile(interfaceTsFile, interfaceTs);
  await writeFile(serviceTsFile, serviceTs);
}
