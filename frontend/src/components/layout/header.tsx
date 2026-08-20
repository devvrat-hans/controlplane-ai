"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { useTheme } from "@/components/providers/theme-provider";

interface UserInfo {
  email: string;
  role: string;
}

export function Header() {
  const { theme, toggle } = useTheme();
  const router = useRouter();
  const [user, setUser] = useState<UserInfo | null>(null);

  useEffect(() => {
    const stored = sessionStorage.getItem("cp-user");
    if (stored) {
      try {
        setUser(JSON.parse(stored));
      } catch { /* ignore */ }
    }
  }, []);

  const initials = user
    ? user.email.split("@")[0].slice(0, 2).toUpperCase()
    : "??";

  const displayName = user
    ? user.email.split("@")[0].replace(/^\w/, (c) => c.toUpperCase())
    : "Guest";

  const handleLogout = () => {
    sessionStorage.removeItem("cp-token");
    sessionStorage.removeItem("cp-user");
    router.replace("/login");
  };

  return (
    <header className="flex h-16 items-center justify-between border-b border-border px-6 bg-card">
      <div className="flex items-center gap-4">
        {/* Mobile menu */}
        <button
          className="lg:hidden rounded-md p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground transition-colors"
          aria-label="Toggle sidebar"
        >
          <MenuIcon className="h-5 w-5" />
        </button>

        {/* App selector */}
        <div className="flex items-center gap-2">
          <span className="text-[12px] text-muted-foreground font-mono hidden sm:inline">
            APP
          </span>
          <select className="h-8 rounded-md border border-border bg-background px-2.5 text-[13px] font-medium focus:outline-none focus:ring-2 focus:ring-ring/20 transition-colors">
            <option>All Applications</option>
            <option>chatbot-prod</option>
            <option>copilot-internal</option>
            <option>support-agent</option>
          </select>
        </div>
      </div>

      <div className="flex items-center gap-2">
        {/* Health */}
        <div className="flex items-center gap-2 rounded-full border border-border px-3 py-1.5 bg-background">
          <div className="h-2 w-2 rounded-full bg-emerald-500" />
          <span className="text-[12px] text-muted-foreground font-mono hidden sm:inline">
            Healthy
          </span>
        </div>

        {/* Theme toggle */}
        <button
          onClick={toggle}
          className="rounded-full p-2 text-muted-foreground hover:bg-accent hover:text-foreground border border-transparent hover:border-border transition-all"
          aria-label="Toggle theme"
        >
          {theme === "dark" ? (
            <SunIcon className="h-4 w-4" />
          ) : (
            <MoonIcon className="h-4 w-4" />
          )}
        </button>

        {/* User */}
        <div className="flex items-center gap-2.5 ml-1">
          <div className="h-8 w-8 rounded-full bg-gradient-to-br from-chart-1 to-chart-2 flex items-center justify-center">
            <span className="text-[11px] font-semibold text-white">{initials}</span>
          </div>
          <div className="hidden md:block">
            <p className="text-[13px] font-medium leading-none">{displayName}</p>
            <p className="text-[11px] text-muted-foreground mt-0.5">
              {user?.role ?? "guest"}
            </p>
          </div>
          <button
            onClick={handleLogout}
            className="ml-2 rounded-md px-2 py-1 text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground border border-border transition-colors"
            title="Sign out"
          >
            <LogOutIcon className="h-3.5 w-3.5" />
          </button>
        </div>
      </div>
    </header>
  );
}

function MenuIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <line x1="4" x2="20" y1="12" y2="12" />
      <line x1="4" x2="20" y1="6" y2="6" />
      <line x1="4" x2="20" y1="18" y2="18" />
    </svg>
  );
}

function SunIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2" /><path d="M12 20v2" /><path d="m4.93 4.93 1.41 1.41" /><path d="m17.66 17.66 1.41 1.41" /><path d="M2 12h2" /><path d="M20 12h2" /><path d="m6.34 17.66-1.41 1.41" /><path d="m19.07 4.93-1.41 1.41" />
    </svg>
  );
}

function MoonIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z" />
    </svg>
  );
}

function LogOutIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <polyline points="16 17 21 12 16 7" />
      <line x1="21" x2="9" y1="12" y2="12" />
    </svg>
  );
}
