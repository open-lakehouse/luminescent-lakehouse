// Types for sandbox.js, for its tests (src/host/mcp-app-sandbox.test.ts).
export interface ViewCsp {
  connectDomains?: string[];
  resourceDomains?: string[];
  frameDomains?: string[];
  baseUriDomains?: string[];
}
export declare const PARENT_ORIGINS: string[];
export declare const VIEW_SANDBOX: string;
export declare function isJsonRpc(data: unknown): boolean;
export declare function domains(list: unknown): string;
export declare function viewCsp(csp?: ViewCsp): string;
export declare function withCsp(html: string, csp?: ViewCsp): string;
export declare function viewSandbox(requested?: unknown): string;
export declare function viewAllow(permissions?: unknown): string;
export declare function parentOrigin(win: Window, allowed: string[]): string | undefined;
export declare function start(win: Window, allowed?: string[]): boolean;
