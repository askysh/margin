/** The notes as the meeting file stores them. `library::splice_notes` trims the text and
 * turns a `## ` line into `### `, so a level-2 heading can't end the Notes section. Keep
 * the two in step. */
export function storedNotes(notes: string): string {
  return notes
    .trim()
    .split("\n")
    .map((l) => l.replace(/\r$/, ""))
    .map((l) => (l.startsWith("## ") ? `### ${l.slice(3)}` : l))
    .join("\n");
}

/** True when `disk` is what saving `local` writes. Then the editor keeps `local`, with the
 * caret where it is, and does not take the stored copy. */
export function sameNotes(local: string, disk: string): boolean {
  return local === disk || storedNotes(local) === disk;
}
