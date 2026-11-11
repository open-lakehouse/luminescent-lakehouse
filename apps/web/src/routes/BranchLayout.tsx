import { Outlet, useParams } from "react-router-dom";
import { branchFromAuthHost, branchFromEndpoint } from "@/lib/branch";
import NotFound from "./NotFound";

/**
 * `/a/:host/:db/*` (and the older `/b/:endpoint/*`): everything below it talks
 * to that branch's Neon Auth, which it gets as the outlet context.
 */
export default function BranchLayout() {
  const { host, db, endpoint } = useParams();
  const branch =
    endpoint !== undefined ? branchFromEndpoint(endpoint) : branchFromAuthHost(host, db);
  if (!branch) return <NotFound />;
  return (
    <main className="mx-auto flex min-h-full max-w-md flex-col justify-center p-6">
      <Outlet context={branch} />
    </main>
  );
}
