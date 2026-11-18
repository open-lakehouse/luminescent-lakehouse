import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { useGraph, useGraphs } from "../queries.js";
import { NodeDiffPanel } from "./NodeDiffPanel.js";
import { type DiffMode, PlanDiffView } from "./PlanDiffView.js";

function ApprovedVsCurrent({ mode: initial }: { mode: DiffMode }) {
  const graphs = useGraphs();
  const root = graphs.data?.[0]?.root?.ref;
  const graph = useGraph(root);
  const [selected, setSelected] = useState<string>();
  const [mode, setMode] = useState(initial);
  if (!root || !graph.data) return null;
  return (
    <div className="grid h-full grid-cols-[1fr_420px]">
      <PlanDiffView
        root={root}
        base={graph.data.approvedRevision}
        target={graph.data.revision}
        selected={selected}
        onSelect={setSelected}
        mode={mode}
        onModeChange={setMode}
      />
      <div className="border-l">
        {selected ? (
          <NodeDiffPanel
            root={root}
            base={graph.data.approvedRevision}
            target={graph.data.revision}
            nodeKey={selected}
          />
        ) : null}
      </div>
    </div>
  );
}

const meta: Meta<typeof ApprovedVsCurrent> = {
  title: "Factory/Plan diff",
  component: ApprovedVsCurrent,
};
export default meta;

export const Tree: StoryObj<typeof ApprovedVsCurrent> = { args: { mode: "tree" } };
export const Text: StoryObj<typeof ApprovedVsCurrent> = { args: { mode: "text" } };
