import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const mockFetch = vi.fn();
global.fetch = mockFetch;

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: vi.fn(),
    replace: vi.fn(),
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => "/policies",
  useSearchParams: () => new URLSearchParams(),
}));

import PoliciesPage from "@/app/policies/page";

describe("Policies Page", () => {
  beforeEach(() => {
    sessionStorage.clear();
    mockFetch.mockReset();
    mockFetch.mockResolvedValue({
      ok: true,
      json: () => Promise.resolve({ policies: [] }),
    });
  });

  afterEach(() => {
    sessionStorage.clear();
  });

  function renderAsAdmin() {
    sessionStorage.setItem("cp-token", "mock-token");
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));
    return render(<PoliciesPage />);
  }

  function renderAsViewer() {
    sessionStorage.setItem("cp-token", "mock-token");
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "viewer@controlplane.ai", role: "viewer" }));
    return render(<PoliciesPage />);
  }

  it("renders policies heading when authenticated", async () => {
    renderAsAdmin();

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Policies" })).toBeInTheDocument();
    });
  });

  it("shows app selector with application options", async () => {
    renderAsAdmin();

    await waitFor(() => {
      expect(screen.getByText("ChatBot-Prod")).toBeInTheDocument();
    });
    expect(screen.getByText("Agent-Internal")).toBeInTheDocument();
    expect(screen.getByText("RAG-Customer-Support")).toBeInTheDocument();
  });

  it("admin can see save button", async () => {
    renderAsAdmin();

    await waitFor(() => {
      const saveButton = screen.getByRole("button", { name: /Save Policy/i });
      expect(saveButton).toBeInTheDocument();
    });
  });

  it("viewer cannot see save button", async () => {
    renderAsViewer();

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Policies" })).toBeInTheDocument();
    });

    // Give time for the useUser hook to update
    await waitFor(() => {
      const saveButton = screen.queryByRole("button", { name: /Save Policy/i });
      expect(saveButton).toBeNull();
    });
  });

  it("fetches policy data when app is changed", async () => {
    renderAsAdmin();

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Policies" })).toBeInTheDocument();
    });

    // Change the app selector to trigger a loadPolicy fetch
    const select = screen.getByDisplayValue("ChatBot-Prod");
    fireEvent.change(select, { target: { value: "10000000-0000-0000-0000-000000000002" } });

    await waitFor(() => {
      const policyCalls = mockFetch.mock.calls.filter(
        (call: [string]) => call[0].includes("/api/v1/policies/10000000-0000-0000-0000-000000000002")
      );
      expect(policyCalls.length).toBeGreaterThan(0);
    });
  });

  it("sends PUT request when save button clicked", async () => {
    renderAsAdmin();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Save Policy/i })).toBeInTheDocument();
    });

    const saveButton = screen.getByRole("button", { name: /Save Policy/i });
    fireEvent.click(saveButton);

    await waitFor(() => {
      const putCalls = mockFetch.mock.calls.filter(
        (call: [string, RequestInit]) => call[1]?.method === "PUT"
      );
      expect(putCalls.length).toBeGreaterThan(0);
      expect(putCalls[0][0]).toContain("/api/v1/policies/10000000");
    });
  });

  it("shows threshold configuration inputs", async () => {
    renderAsAdmin();

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Policies" })).toBeInTheDocument();
    });

    // Should show configuration description
    expect(screen.getByText(/Configure detection thresholds/i)).toBeInTheDocument();
  });
});
