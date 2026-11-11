import { LogIn, LogOut } from "lucide-react";
import { useOutletContext } from "react-router-dom";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { signIn, signOut, useSession } from "@/lib/auth";
import type { Branch } from "@/lib/branch";

/** The branch's signed-in user, or a GitHub sign-in button. */
export default function WhoAmI() {
  const branch = useOutletContext<Branch>();
  const { data, isPending, error } = useSession(branch.authUrl);

  return (
    <Card>
      <CardHeader>
        <CardTitle>Neon Auth</CardTitle>
        <CardDescription className="font-mono break-all">{branch.label}</CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        {isPending ? (
          <p className="text-muted-foreground text-sm">Loading the session…</p>
        ) : data ? (
          <>
            <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 text-sm">
              <dt className="text-muted-foreground">Name</dt>
              <dd>{data.user.name}</dd>
              <dt className="text-muted-foreground">Email</dt>
              <dd>{data.user.email}</dd>
              <dt className="text-muted-foreground">User id</dt>
              <dd className="font-mono text-xs">{data.user.id}</dd>
            </dl>
            <Button
              variant="outline"
              className="self-start"
              onClick={() => signOut(branch.authUrl)}
            >
              <LogOut />
              Sign out
            </Button>
          </>
        ) : (
          <>
            {error && <p className="text-destructive text-sm">{error.message}</p>}
            <Button className="self-start" onClick={() => signIn(branch.authUrl)}>
              <LogIn />
              Sign in with GitHub
            </Button>
          </>
        )}
      </CardContent>
    </Card>
  );
}
