import { describe, it, expect } from "vitest";
import { generate } from "@icp-sdk/bindgen";
import { helloWorldAsset } from "./assets/hello-world";

describe("generate", () => {
  it("should generate a bindgen", () => {
    const result = generate(helloWorldAsset.path);
    expect(result).toBe(helloWorldAsset.result);
  });
});
