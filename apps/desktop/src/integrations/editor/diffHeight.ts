// How tall one file's diff is in the stacked Changes list: its longer side's
// lines, within bounds (Monaco needs a fixed height).

const LINE_PX = 19;
const MIN_LINES = 4;
const MAX_PX = 420;

const lines = (text: string | undefined) => (text ? text.split("\n").length : 0);

/** The height a diff of these sides gets in the stacked list. */
export function diffHeight(original?: string, modified?: string): number {
  const longest = Math.max(lines(original), lines(modified), MIN_LINES);
  return Math.min(longest * LINE_PX + 8, MAX_PX);
}
