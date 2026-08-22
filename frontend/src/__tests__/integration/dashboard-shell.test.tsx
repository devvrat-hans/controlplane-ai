import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";

const mockPush = vi.fn();
const mockReplace = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: mockPush,
    replace: mockReplace,
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => "/",
  useSearchParams: () => new URLSearchParams(),
}));

import { DashboardShell } from "@/components/layout/dashboard-shell";

describe("DashboardShell - Auth Guard", () => {
  beforeEach(() => {
    sessionStorage.clear();
    mockPush.mockReset();
    mockReplace.mockReset();
  });

  afterEach(() => {
    sessionStorage.clear();
  });

  it("redirects to /login when no token in session", async () => {
    render(
      <DashboardShell>
        <div>Dashboard Content</div>
      </DashboardShell>
    );

    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith("/login");
    });
  });

  it("does not show content while unauthenticated", () => {
    render(
      <DashboardShell>
        <div>Dashboard Content</div>
      </DashboardShell>
    );

    expect(screen.queryByText("Dashboard Content")).toBeNull();
  });

  it("shows spinner while checking auth", () => {
    render(
      <DashboardShell>
        <div>Dashboard Content</div>
      </DashboardShell>
    );

    const spinner = document.querySelector(".animate-spin");
    expect(spinner).not.toBeNull();
  });

  it("renders children when authenticated", async () => {
    sessionStorage.setItem("cp-token", "valid-token");
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));

    render(
      <DashboardShell>
        <div>Dashboard Content</div>
      </DashboardShell>
    );

    await waitFor(() => {
      expect(screen.getByText("Dashboard Content")).toBeInTheDocument();
    });
  });

  it("does not redirect when authenticated", async () => {
    sessionStorage.setItem("cp-token", "valid-token");
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));

    render(
      <DashboardShell>
        <div>Dashboard Content</div>
      </DashboardShell>
    );

    await waitFor(() => {
      expect(screen.getByText("Dashboard Content")).toBeInTheDocument();
    });

    expect(mockReplace).not.toHaveBeenCalledWith("/login");
  });
});
