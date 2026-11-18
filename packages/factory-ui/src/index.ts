// `@open-lakehouse/factory-ui`: the factory control room and its parts, bound
// through `@open-lakehouse/host` (the `factory` service key).

export { PlanCanvas, type PlanCanvasProps } from "./canvas/PlanCanvas.js";
export {
  FactoryClientProvider,
  type FactoryClientProviderProps,
  useFactoryClient,
} from "./context.js";
export { NodeDiffPanel, type NodeDiffPanelProps } from "./diff/NodeDiffPanel.js";
export { type DiffMode, PlanDiffView, type PlanDiffViewProps } from "./diff/PlanDiffView.js";
export { FactoryControlRoom, type FactoryControlRoomProps } from "./FactoryControlRoom.js";
export { GraphList, type GraphListProps } from "./GraphList.js";
export { NodeInspector, type NodeInspectorProps } from "./inspector/NodeInspector.js";
export * from "./kinds/index.js";
export * from "./queries.js";
export { RevisionTimeline, type RevisionTimelineProps } from "./RevisionTimeline.js";
