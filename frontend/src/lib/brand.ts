/**
 * Single source of truth for product branding.
 *
 * RTXCore is the vendor, ControlPlane AI is the product. Anything user-facing
 * that names them should read from here, so a rename is one edit instead of a
 * hunt through the UI. Mirrors the `<Metadata>` in `app/layout.tsx` and the
 * monogram in `public/favicon.svg`.
 */
export const BRAND = {
  /** Vendor — the company that ships the product. */
  vendor: "RTXCore",
  /** Vendor styling used under the sidebar wordmark. */
  vendorShort: "RTXCore AI",
  /** Product, without the suffix — the big wordmark. */
  product: "ControlPlane",
  /** Product, full name. */
  productFull: "ControlPlane AI",
  /** Vendor + product, as one string. Used in titles, footers and the login card. */
  full: "RTXCore ControlPlane AI",
  /** Monogram inside the brand tile (same letters as the favicon). */
  mark: "RTX",
  /** One-line description, reused in the dashboard footer and metadata. */
  tagline: "Real-time control layer for AI deployments",
  /** Fallback label when a route has no explicit title. */
  short: "Governance console",
  version: "v0.7.0",
  copyright: "© RTXCore",
} as const;

/** Browser-tab format: `Overview · RTXCore ControlPlane AI`. */
export function brandTitle(page?: string): string {
  return page ? `${page} · ${BRAND.full}` : BRAND.full;
}

/** Pathname → tab title. Applied once, in DashboardShell, for every page. */
export const PAGE_TITLES: Record<string, string> = {
  "/": "Overview",
  "/stream": "Live Stream",
  "/requests": "Requests",
  "/analytics": "Analytics",
  "/policies": "Policies",
  "/escalations": "Escalations",
  "/cost": "Cost",
  "/audit": "Audit",
  "/docs": "API Docs",
  "/settings": "Settings",
  "/api-keys": "API Keys",
  "/profile": "Profile",
  "/playground": "Playground",
  "/login": "Sign in",
};

/**
 * Resolve a tab title from a pathname, longest-prefix first, so nested routes
 * like `/requests/abc123` still read as "Requests".
 */
export function pageTitleFor(pathname: string): string {
  const exact = PAGE_TITLES[pathname];
  if (exact) return exact;

  const prefix = Object.keys(PAGE_TITLES)
    .filter((route) => route !== "/" && pathname.startsWith(route))
    .sort((a, b) => b.length - a.length)[0];

  return prefix ? PAGE_TITLES[prefix] : BRAND.short;
}
