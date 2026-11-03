import { Check, TriangleAlert, X } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useOutletContext, useSearchParams } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { type AuthFailure, consent, publicClient, useSession } from "@/lib/auth";
import { consentRequest } from "@/lib/oauth";
import { Failure } from "./SignIn";

/**
 * The OAuth server's consent page: what the client asks for, and Allow / Deny.
 * A client's name is self-asserted, so the page also shows where the code goes
 * and what it unlocks, and warns when only an app on this computer gets it.
 * Either answer returns to the client's redirect URI (with a code, or
 * `error=access_denied`).
 */
export default function Consent() {
  const endpoint = useOutletContext<string>();
  const [search] = useSearchParams();
  const request = consentRequest(search);
  const { data: session, isPending } = useSession(endpoint);
  const [clientName, setClientName] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState<AuthFailure>();

  const clientId = request?.clientId;
  useEffect(() => {
    if (!clientId || !session) return;
    publicClient(endpoint, clientId)
      .then((c) => setClientName(c.client_name))
      .catch(() => setClientName(undefined));
  }, [endpoint, clientId, session]);

  const answer = async (accept: boolean) => {
    setBusy(true);
    const result = await consent(endpoint, accept);
    if (result) {
      setFailed(result);
      setBusy(false);
    }
  };

  if (!request || !search.has("sig")) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Nothing to approve</CardTitle>
          <CardDescription>There is no authorization request here.</CardDescription>
        </CardHeader>
      </Card>
    );
  }
  if (isPending) return null;
  if (!session) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Not signed in</CardTitle>
        </CardHeader>
        <CardContent>
          <Link className="text-sm underline" to={`/b/${endpoint}/auth/sign-in?${search}`}>
            Sign in to continue
          </Link>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Allow {clientName ?? "this app"}?</CardTitle>
        <CardDescription>
          Signed in as {session.user.email}. The app asks to act as you.
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
          <dt className="text-muted-foreground">Client</dt>
          <dd className="font-mono text-xs break-all">{request.clientId}</dd>
          <dt className="text-muted-foreground">Sends you to</dt>
          <dd className="font-mono text-xs break-all">{request.redirectHost}</dd>
          <dt className="text-muted-foreground">Scopes</dt>
          <dd className="font-mono text-xs">{request.scopes.join(" ") || "none"}</dd>
          <dt className="text-muted-foreground">Access to</dt>
          <dd className="font-mono text-xs break-all">
            {request.resources.length ? request.resources.join(", ") : "your profile only"}
          </dd>
        </dl>
        {request.loopback && (
          <p role="note" className="flex gap-2 rounded-md border border-amber-500/50 p-3 text-sm">
            <TriangleAlert className="size-4 shrink-0" />
            Only an app running on this computer receives the code. Allow it only if you started
            this sign-in yourself, from a tool you trust.
          </p>
        )}
        {failed && <Failure failure={failed} />}
        <div className="flex gap-2">
          <Button disabled={busy} onClick={() => answer(true)}>
            <Check />
            Allow
          </Button>
          <Button variant="outline" disabled={busy} onClick={() => answer(false)}>
            <X />
            Deny
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
