// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { withoutPatch } from "./util";

describe("withoutPatch", () => {
  const base = { model: "small", language: "en", name: "A" };

  test("puts a refused change back", () => {
    const current = { ...base, model: "large" };
    expect(withoutPatch(current, { model: "large" }, base)).toEqual(base);
  });

  test("keeps a later change to the same key and other keys", () => {
    const current = { ...base, model: "tiny", name: "B" };
    expect(withoutPatch(current, { model: "large" }, base)).toEqual({ ...base, model: "tiny", name: "B" });
  });
});
