import { Outlet, useParams } from "react-router-dom";
import { isEndpoint } from "@/lib/endpoint";
import NotFound from "./NotFound";

/** `/b/:endpoint/*`: everything below it talks to that branch's Neon Auth. */
export default function BranchLayout() {
  const { endpoint } = useParams();
  if (!isEndpoint(endpoint)) return <NotFound />;
  return (
    <main className="mx-auto flex min-h-full max-w-md flex-col justify-center p-6">
      <Outlet context={endpoint} />
    </main>
  );
}
