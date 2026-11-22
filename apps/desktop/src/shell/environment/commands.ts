// An environment's commands for the ⌘K palette: its actions (the same ones as its run
// action and ⋯ menus, so they can't disagree), a restart, its Configure tab's sections, the bottom panel's
// views of it, and the `trestle` CLI equivalents to copy.

import type { Command, HostAction } from "@open-lakehouse/env-editor";
import {
  cliTarget,
  downCommand,
  healthCommand,
  logsCommand,
  restartCommand,
  upCommand,
} from "../cli";
import { copyToClipboard } from "../clipboard";
import { type ConfigureSection, configureSections } from "./ConfigureTab";
import type { EnvironmentModel } from "./model";

/** Each Configure section's command: its title for `name`, and more words to find it. */
const CONFIGURE_COMMANDS: Record<
  ConfigureSection,
  { title: (name: string) => string; keywords: string[] }
> = {
  neon: { title: (name) => `Pick ${name}'s Neon branch…`, keywords: ["neon", "branch"] },
  models: { title: (name) => `Choose ${name}'s models…`, keywords: ["models", "gateway", "llm"] },
  variables: {
    title: (name) => `Edit ${name}'s variables…`,
    keywords: ["env", "secret", "keychain"],
  },
};

export interface CommandHandlers {
  /** Stop, then start again. */
  restart: (m: EnvironmentModel) => void;
  /** Show the bottom panel's tab `panel`. */
  openPanel: (panel: string) => void;
  /** Open the Configure tab, on `section` if given. */
  configure: (m: EnvironmentModel, section?: ConfigureSection) => void;
}

/** The bottom panel's tabs the commands open. */
export interface CommandPanels {
  logs: string;
  effectiveEnv: string;
}

export function environmentCommands(
  m: EnvironmentModel,
  workspaceName: string,
  actions: HostAction[],
  h: CommandHandlers,
  panels: CommandPanels,
): Command[] {
  const { env, name, shown, editorId: envId } = m;
  const busy = m.op !== undefined;
  const up = upCommand(cliTarget(workspaceName, name, env.head), m.target?.branch);
  const down = downCommand(cliTarget(workspaceName, name));
  const shownTarget = shown && cliTarget(workspaceName, name, shown.revision);
  const out: Command[] = [];
  for (const action of actions) {
    const base = {
      section: "Environment",
      keywords: [name],
      enabled: action.enabled,
      reason: action.reason,
      run: action.run,
    };
    if (action.id === "start") {
      const title = env.head && m.live.length > 0 ? `Start #${env.head}` : "Start";
      out.push({ ...base, id: `env.${envId}.start`, title: `${title} ${name}`, hint: up });
    } else if (action.id === "stop") {
      out.push({ ...base, id: `env.${envId}.stop`, title: `Stop ${name}`, hint: down });
    } else if (action.id === "stop-shown" && shownTarget) {
      out.push({
        ...base,
        id: `env.${envId}.stop-shown`,
        title: `${action.label} of ${name}`,
        hint: downCommand(shownTarget),
      });
    } else if (action.id.startsWith("open-")) {
      out.push({
        ...base,
        id: `env.${envId}.${action.id}`,
        title: `Open ${action.label} (${name})`,
      });
    }
  }
  out.push({
    id: `env.${envId}.configure`,
    title: `Configure ${name}`,
    section: "Environment",
    keywords: [name, "settings", "neon", "models", "variables"],
    run: () => h.configure(m),
  });
  for (const section of configureSections(m)) {
    const c = CONFIGURE_COMMANDS[section];
    out.push({
      id: `env.${envId}.${section === "neon" ? "neon-target" : section}`,
      title: c.title(name),
      section: "Environment",
      keywords: [name, ...c.keywords],
      run: () => h.configure(m, section),
    });
  }
  if (m.headRuns && env.head) {
    out.push({
      id: `env.${envId}.restart`,
      title: `Restart ${name}`,
      section: "Environment",
      keywords: [name, "stop", "start"],
      enabled: !busy,
      reason: busy ? `${name} is busy` : undefined,
      hint: restartCommand(down, up),
      run: () => h.restart(m),
    });
  }
  // The CLI's equivalents, whatever the actions offer right now: `up` once there is
  // a revision, `down` while there is an instance, and logs and health for one with
  // containers.
  if (shown && shown.slot !== null) {
    out.push({
      id: `env.${envId}.logs`,
      title: `Show logs of ${name}`,
      section: "Environment",
      keywords: [name, "logs", "containers"],
      run: () => h.openPanel(panels.logs),
    });
  }
  const live = m.live.length > 0;
  const containers = shownTarget && shown.slot !== null ? shownTarget : undefined;
  const cli: [string, string | false | undefined][] = [
    ["start", !!env.head && up],
    ["stop", live && down],
    ["restart", m.headRuns && restartCommand(down, up)],
    ["logs", containers && logsCommand(containers)],
    ["health", containers && healthCommand(containers)],
  ];
  if (shown) {
    out.push({
      id: `env.${envId}.effective-env`,
      title: `Show ${name}'s effective environment`,
      section: "Environment",
      keywords: [name, "env", "variables", "dotenv", "effective"],
      hint: `trestle env show ${cliTarget(workspaceName, name, shown.revision)} --effective`,
      run: () => h.openPanel(panels.effectiveEnv),
    });
  }
  // The branch's name, which the card's Neon detail shows but can't copy.
  const branch = m.neon?.branch;
  if (branch) {
    out.push({
      id: `env.${envId}.copy-neon-branch`,
      title: `Copy ${name}'s Neon branch name`,
      hint: branch,
      section: "Environment",
      keywords: [name, "neon", "branch", "copy"],
      run: () => copyToClipboard(branch),
    });
  }
  for (const [verb, line] of cli) {
    if (!line) continue;
    out.push({
      id: `cli.${envId}.${verb}`,
      title: `Copy CLI: ${line}`,
      section: "CLI",
      keywords: [name, verb, "copy", "trestle"],
      run: () => copyToClipboard(line),
    });
  }
  return out;
}
