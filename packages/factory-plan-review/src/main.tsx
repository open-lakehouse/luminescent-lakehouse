// The MCP App workgraph-mcp serves as `ui://factory/plan`. The host renders
// it next to an `open_plan_review` result and hands it that result; the view
// shows the plan and its changes since the approved revision, and the person's
// approval or return goes back through the host as `approve_revision` or
// `return_plan`, then to the model as `ui/update-model-context`. It has no
// network and no token.

import "./styles.css";

import { App, applyDocumentTheme, applyHostStyleVariables } from "@modelcontextprotocol/ext-apps";
import { ThemeProvider, TooltipProvider } from "@open-lakehouse/ui-kit";
import { StrictMode, useSyncExternalStore } from "react";
import { createRoot } from "react-dom/client";
import type { ReviewHost } from "./decide.js";
import { PlanReviewView } from "./PlanReviewView.js";
import { type PlanReview, parseReview } from "./snapshot.js";

const app = new App({ name: "factory-plan-review", version: "0.0.0" });

const host: ReviewHost = {
  callTool: (name, args) => app.callServerTool({ name, arguments: args }),
  updateModelContext: async (text) => {
    await app.updateModelContext({ content: [{ type: "text", text }] });
  },
};

// The latest tool result's review, and who's listening.
let review: PlanReview | undefined;
let problem: string | undefined;
const listeners = new Set<() => void>();
const store = {
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  get: () => review,
};
const emit = () => {
  for (const l of listeners) l();
};

let dark = false;
function applyTheme() {
  const context = app.getHostContext();
  if (context?.theme) {
    applyDocumentTheme(context.theme);
    dark = context.theme === "dark";
    document.documentElement.classList.toggle("dark", dark);
  }
  if (context?.styles?.variables) applyHostStyleVariables(context.styles.variables);
  emit();
}

// Handlers go in before `connect`, so the first result isn't missed.
app.ontoolresult = (result) => {
  const parsed = parseReview(result.structuredContent);
  if (parsed) {
    review = parsed;
    problem = undefined;
  } else {
    problem = "The tool result carries no plan review.";
  }
  emit();
};
app.onhostcontextchanged = () => applyTheme();

function Root() {
  const current = useSyncExternalStore(store.subscribe, store.get);
  if (!current) {
    return (
      <p className="p-3 text-muted-foreground text-sm" role="status">
        {problem ?? "Loading the plan…"}
      </p>
    );
  }
  return <PlanReviewView review={current} host={host} colorMode={dark ? "dark" : "light"} />;
}

const root = document.getElementById("root");
if (!root) throw new Error("no #root");
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <TooltipProvider>
        <Root />
      </TooltipProvider>
    </ThemeProvider>
  </StrictMode>,
);

app.connect().then(applyTheme, (error: unknown) => {
  console.error("MCP Apps handshake failed", error);
});
