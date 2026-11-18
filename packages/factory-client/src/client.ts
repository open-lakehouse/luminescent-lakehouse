import { type Client, createClient, type Transport } from "@connectrpc/connect";
import { FactoryService } from "./gen/factory/v1/service_pb.js";
import { clientTransport } from "./transport.js";

/** A typed factory.v1 client. */
export type FactoryClient = Client<typeof FactoryService>;

/**
 * A factory.v1 client. Without a transport it resolves the host's `factory`
 * binding on every call (see `transport.ts`).
 */
export function createFactoryClient(transport: Transport = clientTransport): FactoryClient {
  return createClient(FactoryService, transport);
}
