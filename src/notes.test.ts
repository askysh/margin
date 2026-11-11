// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { sameNotes, storedNotes } from "./notes";

describe("storedNotes", () => {
  test("trims and rewrites level-2 headings as splice_notes does", () => {
    expect(storedNotes("\n- a\n## Big\n\n")).toBe("- a\n### Big");
    expect(storedNotes("### Keep\n#### Keep")).toBe("### Keep\n#### Keep");
    expect(storedNotes("a\r\nb")).toBe("a\nb");
  });
});

describe("sameNotes", () => {
  test("keeps what the user typed when the file has its stored copy", () => {
    expect(sameNotes("- a\n", "- a")).toBe(true);
    expect(sameNotes("## Plan\n- x ", "### Plan\n- x")).toBe(true);
    expect(sameNotes("draft as typed\n", "draft as typed\n")).toBe(true);
  });

  test("takes an edit made somewhere else", () => {
    expect(sameNotes("- a", "- a\n- from Obsidian")).toBe(false);
  });
});
