import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { TooltipProvider } from "@/components/ui/tooltip";
import { QueryProvider } from "@/components/providers/query-provider";
import { ThemeProvider } from "@/components/providers/theme-provider";
import { AppProvider } from "@/components/providers/app-provider";
import { BRAND } from "@/lib/brand";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  // Product brand, from the single source of truth in lib/brand.ts. `default` is
  // what the browser tab shows; `template` appends the brand to any page that
  // sets its own title, so the tab never falls back to the dev-server URL.
  // Dashboard pages set their own document.title in DashboardShell.
  title: {
    default: BRAND.full,
    template: `%s · ${BRAND.full}`,
  },
  applicationName: BRAND.full,
  description: `${BRAND.full} — ${BRAND.tagline}`,
  authors: [{ name: BRAND.vendor }],
  creator: BRAND.vendor,
  publisher: BRAND.vendor,
  keywords: [
    BRAND.vendor,
    BRAND.productFull,
    "AI governance",
    "LLM guardrails",
    "AI observability",
    "policy enforcement",
  ],
  openGraph: {
    title: BRAND.full,
    description: BRAND.tagline,
    siteName: BRAND.full,
    type: "website",
  },
  twitter: {
    card: "summary",
    title: BRAND.full,
    description: BRAND.tagline,
  },
  icons: {
    icon: "/favicon.svg",
    shortcut: "/favicon.svg",
    apple: "/favicon.svg",
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html
      lang="en"
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased dark`}
      suppressHydrationWarning
    >
      <body className="min-h-full flex flex-col bg-background text-foreground">
        <ThemeProvider>
          <QueryProvider>
            <AppProvider>
              <TooltipProvider>{children}</TooltipProvider>
            </AppProvider>
          </QueryProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
