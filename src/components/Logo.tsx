import { MARK, MARK_TINY } from "./logoMark";

/** The Margin mark: a sound line in the margin crosses the green rule and turns into
 * handwriting. It is the app icon without its tile, drawn from the same geometry
 * (scripts/make-icons.swift), and takes its colours from the theme so it suits light and
 * dark. Square, `size` px a side; under 40 px it switches to the simpler small-size mark.
 * Decorative: put the name in nearby text. */
export function Logo({ size = 32 }: { size?: number }) {
  const m = size < 40 ? MARK_TINY : MARK;
  return (
    <svg className="logo" width={size} height={size} viewBox={m.viewBox} aria-hidden="true" focusable="false" style={{ display: "block", flex: "none" }}>
      <rect className="logo-rule" {...m.rule} rx={m.rule.width / 2} style={{ fill: "var(--rule, #5e9480)" }} />
      <path
        className="logo-line"
        d={m.line}
        style={{ fill: "none", stroke: "var(--ink, #1c2430)", strokeWidth: m.stroke, strokeLinecap: "round", strokeLinejoin: "round" }}
      />
    </svg>
  );
}
