import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { canonicalJson, digest } from "../src/core/jcs.js";

const vectors = JSON.parse(
  readFileSync(new URL("../../conformance/vectors/digest_vectors.json", import.meta.url), "utf8"),
) as { vectors: { name: string; input: unknown; canonical: string; digest: string }[] };

describe("CF001 digest vectors (shared with Rust and Python)", () => {
  for (const v of vectors.vectors) {
    it(v.name, () => {
      expect(canonicalJson(v.input)).toBe(v.canonical);
      expect(digest(v.input)).toBe(v.digest);
    });
  }
});

describe("jcs", () => {
  it("rejects non-finite numbers", () => {
    expect(() => canonicalJson(Number.NaN)).toThrow();
  });
  it("drops undefined members like JSON", () => {
    expect(canonicalJson({ a: undefined, b: 1 })).toBe('{"b":1}');
  });
});
