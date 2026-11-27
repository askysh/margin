// Run with `bun test`.
import { describe, expect, test } from "bun:test";
import { isRawSpeaker, learnHome, speakerNames, tildify, withoutPatch } from "./util";

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

describe("tildify", () => {
  test("replaces a recognisable home", () => {
    expect(tildify("/Users/sam/Documents/Meetings")).toBe("~/Documents/Meetings");
    expect(tildify("/home/sam/notes")).toBe("~/notes");
    expect(tildify("C:\\Users\\sam\\Meetings")).toBe("~\\Meetings");
  });

  test("leaves a path outside the home, and a longer name that only starts with it", () => {
    expect(tildify("/Volumes/Work/Meetings", "/Users/sam")).toBe("/Volumes/Work/Meetings");
    expect(tildify("/Users/samuel/Meetings", "/Users/sam")).toBe("/Users/samuel/Meetings");
  });

  test("an explicit home wins over the guess", () => {
    expect(tildify("/var/folders/x/home/Documents/Meetings", "/var/folders/x/home")).toBe("~/Documents/Meetings");
  });

  test("learns an unrecognisable home from the default meetings folder", () => {
    learnHome("/var/folders/x/home/Documents/Meetings");
    expect(tildify("/var/folders/x/home/Documents/Meetings")).toBe("~/Documents/Meetings");
    expect(tildify("/var/folders/x/home")).toBe("~");
    learnHome("/Users/sam/Documents/Meetings");
    expect(tildify("/var/folders/x/home/Documents/Meetings")).toBe("/var/folders/x/home/Documents/Meetings");
  });
});

describe("speakerNames", () => {
  test("shows UNKNOWN as Unknown speaker and keeps the other numbers", () => {
    const names = speakerNames(["SPEAKER_0", "UNKNOWN", "SPEAKER_2"]);
    expect([...names.values()]).toEqual(["Speaker 1", "Unknown speaker", "Speaker 3"]);
    expect(isRawSpeaker("UNKNOWN")).toBe(true);
  });
});
