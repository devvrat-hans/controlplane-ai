import Link from "next/link";
import { BrandMark } from "@/components/layout/brand-mark";
import { BRAND } from "@/lib/brand";

export default function NotFound() {
  return (
    <div className="flex min-h-screen flex-col items-center justify-center gap-4 bg-background p-6 text-center">
      <BrandMark size="lg" />
      <p className="text-[10px] font-mono uppercase tracking-[0.2em] text-muted-foreground">
        {BRAND.full}
      </p>
      <h1 className="text-2xl font-semibold tracking-tight">Page not found</h1>
      <p className="max-w-md text-sm text-muted-foreground">
        That route is not part of the {BRAND.productFull} dashboard. Use the
        sidebar, or start from the overview.
      </p>
      <Link
        href="/"
        className="mt-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
      >
        Back to overview
      </Link>
      <p className="mt-2 text-[10px] font-mono text-muted-foreground/50">
        {BRAND.copyright} · {BRAND.version}
      </p>
    </div>
  );
}
