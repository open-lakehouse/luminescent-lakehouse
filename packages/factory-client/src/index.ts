// `@open-lakehouse/factory-client`: the factory.v1 client, the work-graph
// types, and the host-bound transport.

export * from "./client.js";
export * from "./gen/factory/v1/service_pb.js";
export * from "./gen/factory/workgraph/v1/execution_pb.js";
export * from "./gen/factory/workgraph/v1/graph_pb.js";
export * from "./gen/factory/workgraph/v1/kinds_pb.js";
export * from "./gen/factory/workgraph/v1/records_pb.js";
export * from "./kinds.js";
export {
  clientTransport,
  createDefaultTransport,
  FACTORY_SERVICE_KEY,
  registerTransport,
  transportFor,
} from "./transport.js";
