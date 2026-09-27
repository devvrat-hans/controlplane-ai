"use client";

import { useState, useEffect } from "react";
import { useRouter, usePathname } from "next/navigation";
import { useTheme } from "@/components/providers/theme-provider";
import {
  Dropdown,
  DropdownTrigger,
  DropdownContent,
  DropdownItem,
  DropdownSeparator,
  DropdownLabel,
} from "@/components/ui/dropdown-menu";
import { Select } from "@/components/ui/select";
import { useApp } from "@/components/providers/app-provider";
import { BrandLockup } from "./brand-mark";

interface UserInfo {
  email: string;
  role: string;
}

const APP_FILTER_PAGES = ["/", "/requests", "/analytics"];

export function Header() {
  const { theme, toggle } = useTheme();
  const router = useRouter();
  const pathname = usePathname();
  const { apps, selectedAppId, setSelectedAppId } = useApp();
  const showAppSelector = APP_FILTER_PAGES.includes(pathname);
  const [user, setUser] = useState<UserInfo | null>(null);
  const [apiHealthy, setApiHealthy] = useState<boolean | null>(null);
  const [sseConnected, setSseConnected] = useState(false);
  const [notifications, setNotifications] = useState<{id: string; type: string; reason: string; time: string}[]>([]);
  const [showNotifs, setShowNotifs] = useState(false);

  // Live health check
  useEffect(() => {
    const checkHealth = async () => {
      try {
        const res = await fetch(`${process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080"}/health`, { signal: AbortSignal.timeout(3000) });
        setApiHealthy(res.ok);
      } catch {
        setApiHealthy(false);
      }
    };
    checkHealth();
    const id = setInterval(checkHealth, 15000);
    return () => clearInterval(id);
  }, []);

  // SSE connection monitor + live notifications
  useEffect(() => {
    const apiBase = process.env.NEXT_PUBLIC_API_URL || "http://localhost:8080";
    const es = new EventSource(`${apiBase}/api/v1/verdicts/stream`);
    es.onopen = () => setSseConnected(true);
    es.onerror = () => setSseConnected(false);
    es.onmessage = (event) => {
      try {
        const data = JSON.parse(event.data);
        if (data.outcome === "block" || data.outcome === "escalate") {
          setNotifications((prev) => [
            { id: data.id ?? Date.now().toString(), type: data.outcome, reason: data.reason ?? data.check_name ?? "", time: new Date().toLocaleTimeString() },
            ...prev,
          ].slice(0, 20));
        }
      } catch { /* ignore non-JSON */ }
    };
    return () => { es.close(); setSseConnected(false); };
  }, []);

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

        {/* Vendor lockup — the sidebar is hidden below lg, so brand the top bar too */}
        <BrandLockup className="lg:hidden" />

        {/* App selector — only shown on pages that support app filtering */}
        {showAppSelector && (
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-muted-foreground font-mono hidden sm:inline">
              APP
            </span>
            <Select
              value={selectedAppId}
              onValueChange={setSelectedAppId}
              options={[
                { value: "all", label: "All Applications" },
                ...apps.map((a) => ({ value: a.id, label: a.name })),
              ]}
              size="sm"
            />
          </div>
        )}
      </div>

      <div className="flex items-center gap-2">
        {/* Live health + SSE status */}
        <div className="flex items-center gap-2 rounded-full border border-border px-3 py-1.5 bg-background">
          <div className={`h-2 w-2 rounded-full ${apiHealthy === false ? "bg-red-500" : apiHealthy === null ? "bg-yellow-500 animate-pulse" : "bg-emerald-500"}`} />
          <span className="text-[12px] text-muted-foreground font-mono hidden sm:inline">
            {apiHealthy === false ? "Offline" : apiHealthy === null ? "Checking" : "Healthy"}
          </span>
          <div className={`h-2 w-2 rounded-full ml-1 ${sseConnected ? "bg-emerald-500" : "bg-red-500"}`} title={sseConnected ? "SSE connected" : "SSE disconnected"} />
          <span className="text-[12px] text-muted-foreground font-mono hidden lg:inline">
            {sseConnected ? "Live" : "No stream"}
          </span>
        </div>

        {/* Notifications bell */}
        <div className="relative">
          <button
            onClick={() => setShowNotifs((s) => !s)}
            className="relative rounded-full p-2 text-muted-foreground hover:bg-accent hover:text-foreground border border-transparent hover:border-border transition-all"
            aria-label="Notifications"
          >
            <BellIcon className="h-4 w-4" />
            {notifications.length > 0 && (
              <span className="absolute -top-0.5 -right-0.5 h-4 w-4 rounded-full bg-red-500 text-[9px] font-bold text-white flex items-center justify-center">
                {Math.min(notifications.length, 99)}
              </span>
            )}
          </button>
          {showNotifs && (
            <div className="absolute right-0 top-full mt-2 w-[320px] rounded-xl border border-border bg-card shadow-xl z-50">
              <div className="flex items-center justify-between px-4 py-2.5 border-b border-border">
                <span className="text-xs font-semibold">Recent Alerts</span>
                {notifications.length > 0 && (
                  <button onClick={() => setNotifications([])} className="text-[10px] text-muted-foreground hover:text-foreground">Clear all</button>
                )}
              </div>
              <div className="max-h-[300px] overflow-y-auto">
                {notifications.length === 0 ? (
                  <p className="py-6 text-center text-xs text-muted-foreground">No recent alerts</p>
                ) : (
                  notifications.map((n) => (
                    <div key={n.id} className="px-4 py-2.5 border-b border-border/50 last:border-0">
                      <div className="flex items-center gap-2">
                        <span className={`h-1.5 w-1.5 rounded-full ${n.type === "block" ? "bg-red-500" : "bg-purple-500"}`} />
                        <span className="text-[11px] font-medium capitalize">{n.type}</span>
                        <span className="text-[10px] text-muted-foreground ml-auto">{n.time}</span>
                      </div>
                      <p className="text-[10px] text-muted-foreground mt-0.5 truncate">{n.reason}</p>
                    </div>
                  ))
                )}
              </div>
            </div>
          )}
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

        {/* User dropdown */}
        <Dropdown>
          <DropdownTrigger>
            <div className="h-8 w-8 rounded-full bg-gradient-to-br from-chart-1 to-chart-3 flex items-center justify-center">
              <span className="text-[11px] font-semibold text-white">{initials}</span>
            </div>
            <div className="hidden md:block text-left">
              <p className="text-[13px] font-medium leading-none">{displayName}</p>
              <p className="text-[11px] text-muted-foreground mt-0.5">{user?.email ?? "guest"}</p>
            </div>
            <ChevronDownIcon className="h-3.5 w-3.5 text-muted-foreground" />
          </DropdownTrigger>

          <DropdownContent align="end">
            <DropdownLabel>
              <p className="text-[13px] font-medium">{displayName}</p>
              <p className="text-[12px] text-muted-foreground">{user?.email ?? "guest"}</p>
            </DropdownLabel>

            <div className="py-1">
              <DropdownItem
                href="/settings"
                icon={<SettingsIcon />}
              >
                Settings
              </DropdownItem>
            </div>

            <DropdownSeparator />

            <div className="py-1">
              <DropdownItem onClick={handleLogout} icon={<LogOutIcon />} danger>
                Log out
              </DropdownItem>
            </div>
          </DropdownContent>
        </Dropdown>
      </div>
    </header>
  );
}

// ─── Icons ───────────────────────────────────────────────────────────────

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
      <path d="M12 2v2" /><path d="M12 20v2" />
      <path d="m4.93 4.93 1.41 1.41" /><path d="m17.66 17.66 1.41 1.41" />
      <path d="M2 12h2" /><path d="M20 12h2" />
      <path d="m6.34 17.66-1.41 1.41" /><path d="m19.07 4.93-1.41 1.41" />
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

function ChevronDownIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="m6 9 6 6 6-6" />
    </svg>
  );
}

function UserIcon() {
  return (
    <svg className="w-4 h-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" />
      <circle cx="12" cy="7" r="4" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg className="w-4 h-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
      <circle cx="12" cy="12" r="3" />
    </svg>
  );
}

function KeyIcon() {
  return (
    <svg className="w-4 h-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="m21 2-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0 3 3L22 7l-3-3m-3.5 3.5L19 4" />
    </svg>
  );
}

function BellIcon({ className }: { className?: string }) {
  return (
    <svg className={className} xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
      <path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
    </svg>
  );
}

function LogOutIcon() {
  return (
    <svg className="w-4 h-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
      <polyline points="16 17 21 12 16 7" />
      <line x1="21" x2="9" y1="12" y2="12" />
    </svg>
  );
}
