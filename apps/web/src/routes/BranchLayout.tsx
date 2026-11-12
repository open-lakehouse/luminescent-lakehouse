import { Outlet, useParams } from "react-router-dom";
import { branchFromAuthHost } from "@/lib/branch";
import NotFound from "./NotFound";

/**
 * `/a/:host/:db/*`: everything below it talks to that branch's Neon Auth,
 * which it gets as the outlet context.
 */
export default function BranchLayout() {
  const { host, db } = useParams();
  const branch = branchFromAuthHost(host, db);
  if (!branch) return <NotFound />;
  return (
    <main className="mx-auto flex min-h-full max-w-md flex-col justify-center p-6">
      <Outlet context={branch} />
    </main>
  );
}
