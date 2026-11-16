// The MCP Apps sandbox proxy (ext-apps' double-iframe convention). The shell frames
// this page on its own origin (the `mcp-app://` scheme); it frames the View in an
// inner `srcdoc` iframe with no `allow-same-origin` (an opaque origin) under a CSP
// built from the resource's `_meta.ui.csp`, and relays JSON-RPC between the two.
//
// - It talks only to its parent, and only if the parent's origin is on the list the
//   scheme fills in below (the shell's origins). The scheme's `frame-ancestors`
//   says the same to the browser.
// - The parent's messages count only from `window.parent`, the View's only from the
//   inner frame's window.
// - `ui/notifications/sandbox-*` are between the shell and this proxy: never relayed.
//
// The srcdoc frame inherits this page's CSP (the header), so the View's own CSP can
// only narrow it: `connect-src 'none'` holds whatever the resource declares.
//
// Plain JS, no build: the scheme serves this file as is (src-tauri/src/mcp_app.rs).

/** The parent origins allowed, filled in by whoever serves this file. */
export const PARENT_ORIGINS = /*__MCP_APP_PARENT_ORIGINS__*/ [];

const PROXY_READY = "ui/notifications/sandbox-proxy-ready";
const RESOURCE_READY = "ui/notifications/sandbox-resource-ready";
const RESERVED = "ui/notifications/sandbox-";

/** The inner frame's sandbox: scripts and forms, never same-origin or top navigation. */
export const VIEW_SANDBOX = "allow-scripts allow-forms";
/** What a resource may add to it (`sandbox` in resource-ready). */
const SANDBOX_EXTRAS = new Set([
  "allow-popups",
  "allow-popups-to-escape-sandbox",
  "allow-modals",
  "allow-downloads",
]);
const PERMISSIONS = {
  camera: "camera",
  microphone: "microphone",
  geolocation: "geolocation",
  clipboardWrite: "clipboard-write",
};

/** @param {unknown} data */
export function isJsonRpc(data) {
  return typeof data === "object" && data !== null && data.jsonrpc === "2.0";
}

/** @param {unknown} data */
function isReserved(data) {
  return typeof data.method === "string" && data.method.startsWith(RESERVED);
}

/** https origins (with an optional path) only: nothing that could end a directive. */
export function domains(list) {
  return (Array.isArray(list) ? list : [])
    .filter((d) => typeof d === "string" && /^https:\/\/[\w.-]+(:\d+)?(\/[^\s;,'"]*)?$/.test(d))
    .join(" ");
}

/** The View's CSP: inline script and style (Views are single documents); network,
 *  frames and base URIs only to the declared domains. */
export function viewCsp(csp = {}) {
  const resources = domains(csp.resourceDomains);
  return [
    "default-src 'none'",
    `script-src 'unsafe-inline' ${resources}`,
    `style-src 'unsafe-inline' ${resources}`,
    `img-src data: blob: ${resources}`,
    `font-src data: ${resources}`,
    `media-src data: blob: ${resources}`,
    `connect-src ${domains(csp.connectDomains) || "'none'"}`,
    `frame-src ${domains(csp.frameDomains) || "'none'"}`,
    `base-uri ${domains(csp.baseUriDomains) || "'none'"}`,
    "object-src 'none'",
    "form-action 'none'",
  ]
    .map((d) => d.trim())
    .join("; ");
}

/** The View's HTML with its CSP as the first thing in `<head>`. */
export function withCsp(html, csp) {
  const meta = `<meta http-equiv="Content-Security-Policy" content="${viewCsp(csp ?? {})}">`;
  if (/<head[^>]*>/i.test(html)) return html.replace(/<head[^>]*>/i, (m) => `${m}${meta}`);
  // No <head>: after the doctype, so the document stays in standards mode.
  const doctype = /^\s*<!doctype[^>]*>/i.exec(html);
  if (doctype) return `${doctype[0]}${meta}${html.slice(doctype[0].length)}`;
  return `${meta}${html}`;
}

/** The inner frame's `sandbox`: ours plus any extras the resource asks for. */
export function viewSandbox(requested) {
  const extras = typeof requested === "string" ? requested.split(/\s+/) : [];
  return [VIEW_SANDBOX, ...new Set(extras.filter((t) => SANDBOX_EXTRAS.has(t)))].join(" ");
}

/** The iframe `allow` for `_meta.ui.permissions`. */
export function viewAllow(permissions) {
  if (!permissions || typeof permissions !== "object") return "";
  return Object.entries(PERMISSIONS)
    .filter(([key]) => permissions[key])
    .map(([, feature]) => feature)
    .join("; ");
}

/** The parent's origin, if it is allowed. */
export function parentOrigin(win, allowed) {
  const ancestors = win.location.ancestorOrigins;
  let origin = ancestors && ancestors.length > 0 ? ancestors[0] : undefined;
  if (!origin && win.document.referrer) {
    try {
      origin = new URL(win.document.referrer).origin;
    } catch {
      origin = undefined;
    }
  }
  return origin && allowed.includes(origin) ? origin : undefined;
}

/**
 * Runs the proxy in `win`. Returns false (and does nothing) unless framed by an
 * allowed origin.
 */
export function start(win, allowed = PARENT_ORIGINS) {
  const parent = win.parent;
  if (!parent || parent === win) return false;
  const origin = parentOrigin(win, allowed);
  if (!origin) return false;

  /** @type {HTMLIFrameElement | undefined} */
  let view;
  win.addEventListener("message", (event) => {
    const data = event.data;
    if (!isJsonRpc(data)) return;
    if (event.source === parent) {
      if (event.origin !== origin) return;
      if (data.method === RESOURCE_READY) {
        if (!view) view = mount(win, data.params);
        return;
      }
      if (isReserved(data)) return;
      view?.contentWindow?.postMessage(data, "*");
      return;
    }
    if (view && event.source === view.contentWindow) {
      if (isReserved(data)) return;
      parent.postMessage(data, origin);
    }
  });
  parent.postMessage({ jsonrpc: "2.0", method: PROXY_READY, params: {} }, origin);
  return true;
}

function mount(win, params) {
  if (!params || typeof params.html !== "string") return undefined;
  const frame = win.document.createElement("iframe");
  frame.setAttribute("sandbox", viewSandbox(params.sandbox));
  const allow = viewAllow(params.permissions);
  if (allow) frame.setAttribute("allow", allow);
  frame.setAttribute("referrerpolicy", "no-referrer");
  frame.setAttribute("title", "MCP App");
  frame.srcdoc = withCsp(params.html, params.csp);
  win.document.body.appendChild(frame);
  return frame;
}

if (typeof window !== "undefined" && /\/sandbox\.html$/.test(window.location.pathname)) {
  start(window);
}
