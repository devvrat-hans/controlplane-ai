"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { BrandMark } from "@/components/layout/brand-mark";
import { BRAND, brandTitle } from "@/lib/brand";

const DEMO_ACCOUNTS = [
  { email: "admin@controlplane.ai", password: "admin123", role: "admin" },
  { email: "viewer@controlplane.ai", password: "viewer123", role: "viewer" },
  { email: "reviewer@controlplane.ai", password: "reviewer123", role: "reviewer" },
];

export default function LoginPage() {
  const router = useRouter();
  const [email, setEmail] = useState("");

  // This route lives outside DashboardShell, so brand its tab here.
  useEffect(() => {
    document.title = brandTitle("Sign in");
  }, []);
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(false);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setLoading(true);

    // Demo mode: check against seeded accounts
    const account = DEMO_ACCOUNTS.find(
      (a) => a.email === email && a.password === password
    );

    if (account) {
      // Generate a simple demo token (in production, this would call the API)
      const token = btoa(
        JSON.stringify({
          sub: account.email.split("@")[0],
          email: account.email,
          role: account.role,
          exp: Math.floor(Date.now() / 1000) + 86400,
        })
      );
      sessionStorage.setItem("cp-token", token);
      sessionStorage.setItem("cp-user", JSON.stringify(account));
      router.push("/");
    } else {
      setError("Invalid email or password");
    }

    setLoading(false);
  };

  const quickLogin = (account: (typeof DEMO_ACCOUNTS)[number]) => {
    setEmail(account.email);
    setPassword(account.password);
  };

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <div className="w-full max-w-md space-y-6">
        {/* Branding */}
        <div className="text-center">
          <BrandMark size="lg" className="mx-auto" />
          <h1 className="mt-4 text-2xl font-semibold tracking-tight-brand">{BRAND.product}</h1>
          <p className="mt-1 text-sm font-medium text-muted-foreground">
            {BRAND.full}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            Sign in to the {BRAND.short.toLowerCase()}
          </p>
        </div>

        {/* Login form */}
        <Card>
          <CardContent className="pt-6">
            <form onSubmit={handleLogin} className="space-y-4">
              <div>
                <label className="text-sm font-medium">Email</label>
                <input
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                  placeholder="admin@controlplane.ai"
                  required
                />
              </div>
              <div>
                <label className="text-sm font-medium">Password</label>
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="mt-1 w-full rounded-md border border-input bg-background px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                  placeholder="••••••••"
                  required
                />
              </div>
              {error && (
                <p className="text-sm text-destructive">{error}</p>
              )}
              <button
                type="submit"
                disabled={loading}
                className="w-full rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-50 transition-colors"
              >
                {loading ? "Signing in..." : "Sign In"}
              </button>
            </form>
          </CardContent>
        </Card>

        {/* Demo accounts */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-xs font-medium text-muted-foreground">
              Demo Accounts (click to fill)
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-2">
            {DEMO_ACCOUNTS.map((account) => (
              <button
                key={account.email}
                onClick={() => quickLogin(account)}
                className="w-full flex items-center justify-between rounded-md border border-border p-2 text-left hover:bg-accent/50 transition-colors"
              >
                <div>
                  <p className="text-sm font-medium">{account.email}</p>
                  <p className="text-xs text-muted-foreground">
                    {account.password}
                  </p>
                </div>
                <RoleBadge role={account.role} />
              </button>
            ))}
          </CardContent>
        </Card>

        {/* Role descriptions */}
        <div className="text-xs text-muted-foreground space-y-1 text-center">
          <p>
            <strong>Admin</strong>: Full access to everything
          </p>
          <p>
            <strong>Reviewer</strong>: Resolve escalations, read-only policies
          </p>
          <p>
            <strong>Viewer</strong>: Read-only access to all pages
          </p>
        </div>

        {/* Brand footer */}
        <div className="flex items-center justify-between border-t border-border pt-4 text-[10px] text-muted-foreground/60">
          <span className="font-mono uppercase tracking-[0.12em]">{BRAND.copyright}</span>
          <span className="font-mono">{BRAND.version} · local deployment</span>
        </div>
      </div>
    </div>
  );
}

function RoleBadge({ role }: { role: string }) {
  const styles: Record<string, string> = {
    admin: "bg-purple-500/10 text-purple-500 border-purple-500/20",
    reviewer: "bg-blue-500/10 text-blue-500 border-blue-500/20",
    viewer: "bg-green-500/10 text-green-500 border-green-500/20",
  };

  return (
    <span
      className={`rounded-full border px-2 py-0.5 text-[10px] font-medium capitalize ${styles[role] ?? ""}`}
    >
      {role}
    </span>
  );
}
