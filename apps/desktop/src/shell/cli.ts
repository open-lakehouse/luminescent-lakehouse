// The `trestle` CLI equivalents of the runtime commands, for the ⌘K palette's hints and
// its Copy CLI commands. The CLI and the app run the same instances in the same shared
// state, so a copied command does what the palette's would.

import type { BranchChoice } from "../host/bindings";

/** `<ws>/<env>[@<rev>]`, the CLI's target. */
export function cliTarget(workspace: string, env: string, revision?: number | null): string {
  return revision == null ? `${workspace}/${env}` : `${workspace}/${env}@${revision}`;
}

/** `trestle up`, on `branch` when the revision runs on Neon. */
export function upCommand(target: string, branch?: BranchChoice): string {
  if (!branch) return `trestle up ${target}`;
  if (branch.kind === "existing")
    return `trestle up ${target} --neon-branch ${quote(branch.branch)}`;
  const ttl = branch.ttlHours == null ? "" : ` --ttl ${branch.ttlHours}h`;
  return `trestle up ${target} --neon-branch new${ttl}`;
}

/** `trestle down`: the environment's instances, or one revision's. */
export function downCommand(target: string): string {
  return `trestle down ${target}`;
}

/** A stop then a start, as one line. */
export function restartCommand(down: string, up: string): string {
  return `${down} && ${up}`;
}

/** `trestle logs --follow`. */
export function logsCommand(target: string): string {
  return `trestle logs --follow ${target}`;
}

/** `trestle health`. */
export function healthCommand(target: string): string {
  return `trestle health ${target}`;
}

/** `value` as one shell word: as is when that's safe, else single-quoted. */
export function quote(value: string): string {
  return /^[\w@%+=:,./-]+$/.test(value) ? value : `'${value.replaceAll("'", `'\\''`)}'`;
}
