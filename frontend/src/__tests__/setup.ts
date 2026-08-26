import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";

afterEach(() => {
  cleanup();
});

// Mock next/navigation
vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => "/",
  useSearchParams: () => new URLSearchParams(),
}));

// Mock EventSource (not available in jsdom)
class MockEventSource {
  url: string;
  onopen: (() => void) | null = null;
  onmessage: (() => void) | null = null;
  onerror: (() => void) | null = null;
  readyState = 0;
  CONNECTING = 0;
  OPEN = 1;
  CLOSED = 2;
  constructor(url: string) {
    this.url = url;
    // Auto-fire onopen on next microtask
    Promise.resolve().then(() => {
      this.readyState = 1;
      this.onopen?.();
    });
  }
  close() { this.readyState = 2; }
  addEventListener() {}
  removeEventListener() {}
  dispatchEvent() { return true; }
}
// @ts-expect-error mock
global.EventSource = MockEventSource;

// Mock fetch for health checks
const originalFetch = global.fetch;
global.fetch = vi.fn((url: string | URL | Request) => {
  const urlStr = typeof url === "string" ? url : url instanceof URL ? url.url : url.toString();
  if (urlStr.includes("/health")) {
    return Promise.resolve(new Response("ok", { status: 200 })) as Promise<Response>;
  }
  return originalFetch(url as string | URL | Request) as Promise<Response>;
});
