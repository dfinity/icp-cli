import { describe, it, expect } from "vitest";
import { generate } from "@icp-sdk/bindgen";
import { helloWorldAsset } from "./assets/hello-world";

describe("generate", () => {
  it("should generate a bindgen", () => {
    const result = generate(helloWorldAsset.path);
    expect(result.declarations_js).toBe(helloWorldAsset.result.declarations_js);
    expect(result.declarations_ts).toBe(helloWorldAsset.result.declarations_ts);
    expect(result.interface_ts).toBe(helloWorldAsset.result.interface_ts);
    expect(result.service_ts).toBe(helloWorldAsset.result.service_ts);
  });
});
