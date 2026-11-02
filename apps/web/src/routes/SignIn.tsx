import { LogIn } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useOutletContext, useSearchParams } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { type AuthFailure, continueAuthorization, signIn, useSession } from "@/lib/auth";

/**
 * The OAuth server's login page. Neon Auth sends an authorization request here
 * when nobody is signed in, with the request as a signed, expiring query. Sign-in
 * returns to this same URL; whenever there is a session, the page resumes the
 * authorization, which goes on to consent or straight back to the client.
 */
export default function SignIn() {
  const endpoint = useOutletContext<string>();
  const [search] = useSearchParams();
  const { data, isPending } = useSession(endpoint);
  const [failed, setFailed] = useState<AuthFailure>();
  const resumed = useRef(false);
  const signed = search.has("sig");

  useEffect(() => {
    if (!signed || !data || resumed.current) return;
    resumed.current = true;
    continueAuthorization(endpoint).then(setFailed);
  }, [endpoint, signed, data]);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Sign in</CardTitle>
        <CardDescription>An app is asking you to sign in to this Neon branch.</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {!signed ? (
          <p className="text-muted-foreground text-sm">
            There is no sign-in request here. Start signing in from your app.
          </p>
        ) : failed ? (
          <Failure failure={failed} />
        ) : isPending || data ? (
          <p className="text-muted-foreground text-sm">Continuing…</p>
        ) : (
          <Button
            className="self-start"
            onClick={() =>
              signIn(endpoint).catch((e: Error) => setFailed({ status: 0, message: e.message }))
            }
          >
            <LogIn />
            Sign in with GitHub
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

/** A 400 means the signed request has expired (or was tampered with). */
export function Failure({ failure }: { failure: AuthFailure }) {
  return (
    <p role="alert" className="text-destructive text-sm">
      {failure.status === 400
        ? "This sign-in link has expired. Start again from your app."
        : `Sign-in failed: ${failure.message || `HTTP ${failure.status}`}`}
    </p>
  );
}
