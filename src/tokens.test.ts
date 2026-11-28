// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("./tokens.css", import.meta.url), "utf8");
const darkStart = css.indexOf("prefers-color-scheme: dark");

function palette(source: string): Record<string, string> {
  return Object.fromEntries([...source.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]));
}

const light = palette(css.slice(0, darkStart));
const dark = { ...light, ...palette(css.slice(darkStart)) };

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe.each([["light", light], ["dark", dark]] as const)("%s theme text colors", (_name, p) => {
  test("placeholders reach WCAG AA (4.5:1) on every surface they sit on", () => {
    for (const surface of ["paper", "paper-2", "sheet"]) expect(contrast(p.placeholder, p[surface])).toBeGreaterThanOrEqual(4.5);
  });

  test("the unnamed speaker chip's text and dashed border reach AA on the chip", () => {
    expect(contrast(p["rule-ink"], p.sheet)).toBeGreaterThanOrEqual(4.5);
  });
});
