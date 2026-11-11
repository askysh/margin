// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { isValidElement } from "react";
import { Markdown } from "./Markdown";

describe("Markdown", () => {
  test("ends on lines with U+2028 and U+2029", () => {
    for (const source of ["# a b", "# x", "## one two\nnext", "- a\n# "]) {
      expect(isValidElement(Markdown({ source }))).toBe(true);
    }
  });

  test("still reads headings, lists and paragraphs", () => {
    const el = Markdown({ source: "# Head\n\n- a\n- b\n\ntext\nmore" });
    const blocks = (el.props as { children: { type: string }[] }).children;
    expect(blocks.map((b) => b.type)).toEqual(["h4", "ul", "p"]);
  });
});
