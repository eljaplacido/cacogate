// RFC 8785 JSON Canonicalization Scheme + `sha256:` digests.
// Must match crates/cacogate-contracts/src/canon.rs byte for byte;
// conformance/vectors/digest_vectors.json pins both.
import { createHash } from "node:crypto";

export type Json = null | boolean | number | string | Json[] | { [k: string]: Json };

export function canonicalJson(value: unknown): string {
  if (value === null || value === undefined) return "null";
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "number") {
    if (!Number.isFinite(value)) throw new Error("non-finite number is not representable in JCS");
    // ECMAScript Number.prototype.toString is exactly what JCS specifies; -0 serialises as 0.
    return JSON.stringify(value);
  }
  if (typeof value === "string") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map((v) => canonicalJson(v)).join(",")}]`;
  if (typeof value === "object") {
    const obj = value as Record<string, unknown>;
    // Default sort compares UTF-16 code units, as JCS requires. Undefined members are dropped.
    const keys = Object.keys(obj)
      .filter((k) => obj[k] !== undefined)
      .sort();
    return `{${keys.map((k) => `${JSON.stringify(k)}:${canonicalJson(obj[k])}`).join(",")}}`;
  }
  throw new Error(`unsupported type ${typeof value}`);
}

export function digest(value: unknown): string {
  return `sha256:${createHash("sha256").update(canonicalJson(value), "utf8").digest("hex")}`;
}
