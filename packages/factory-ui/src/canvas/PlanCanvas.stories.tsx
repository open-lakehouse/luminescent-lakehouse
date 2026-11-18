import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { useGraph, useGraphMutations, useGraphs, useSchema } from "../queries.js";
import { PlanCanvas } from "./PlanCanvas.js";

function Editable({ readOnly }: { readOnly?: boolean }) {
  const graphs = useGraphs();
  const schema = useSchema();
  const root = graphs.data?.[0]?.root?.ref;
  const graph = useGraph(root);
  const m = useGraphMutations(root);
  const [selected, setSelected] = useState<string>();
  return (
    <PlanCanvas
      graph={graph.data?.graph}
      schema={schema.data}
      selected={selected}
      onSelect={setSelected}
      readOnly={readOnly}
      onAddChild={(parent, kind) =>
        m.createNode.mutate({ parent, kind, spec: { title: "New node" } })
      }
      onMove={(ref, newParent) => m.moveNode.mutate({ ref, newParent })}
      onAddEdge={(edge) => m.addEdge.mutate(edge)}
      onRemoveEdge={(edge) => m.removeEdge.mutate(edge)}
    />
  );
}

const meta: Meta<typeof Editable> = { title: "Factory/Plan canvas", component: Editable };
export default meta;

export const Interactive: StoryObj<typeof Editable> = {};
export const ReadOnly: StoryObj<typeof Editable> = { args: { readOnly: true } };
