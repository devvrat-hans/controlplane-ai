import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { canEditPolicies, canResolveEscalations, canAccessSettings, useUser } from "@/lib/auth";

describe("Role-based access control functions", () => {
  describe("canEditPolicies", () => {
    it("returns true for admin", () => {
      expect(canEditPolicies("admin")).toBe(true);
    });

    it("returns false for reviewer", () => {
      expect(canEditPolicies("reviewer")).toBe(false);
    });

    it("returns false for viewer", () => {
      expect(canEditPolicies("viewer")).toBe(false);
    });

    it("returns false for unknown role", () => {
      expect(canEditPolicies("unknown")).toBe(false);
    });
  });

  describe("canResolveEscalations", () => {
    it("returns true for admin", () => {
      expect(canResolveEscalations("admin")).toBe(true);
    });

    it("returns true for reviewer", () => {
      expect(canResolveEscalations("reviewer")).toBe(true);
    });

    it("returns false for viewer", () => {
      expect(canResolveEscalations("viewer")).toBe(false);
    });

    it("returns false for unknown role", () => {
      expect(canResolveEscalations("unknown")).toBe(false);
    });
  });

  describe("canAccessSettings", () => {
    it("returns true for admin", () => {
      expect(canAccessSettings("admin")).toBe(true);
    });

    it("returns false for reviewer", () => {
      expect(canAccessSettings("reviewer")).toBe(false);
    });

    it("returns false for viewer", () => {
      expect(canAccessSettings("viewer")).toBe(false);
    });
  });
});

describe("useUser hook", () => {
  beforeEach(() => {
    sessionStorage.clear();
  });

  afterEach(() => {
    sessionStorage.clear();
  });

  it("returns null when no user in session", () => {
    const { result } = renderHook(() => useUser());
    expect(result.current).toBeNull();
  });

  it("returns user from session storage", async () => {
    const user = { email: "admin@controlplane.test", role: "admin" };
    sessionStorage.setItem("cp-user", JSON.stringify(user));

    const { result, rerender } = renderHook(() => useUser());
    
    // useEffect runs asynchronously
    await act(async () => {
      rerender();
    });
    
    expect(result.current).toEqual(user);
  });

  it("handles corrupted session data gracefully", async () => {
    sessionStorage.setItem("cp-user", "not-valid-json{{{");

    const { result, rerender } = renderHook(() => useUser());
    await act(async () => {
      rerender();
    });

    expect(result.current).toBeNull();
  });

  it("distinguishes between admin and viewer roles", async () => {
    const viewer = { email: "viewer@controlplane.test", role: "viewer" };
    sessionStorage.setItem("cp-user", JSON.stringify(viewer));

    const { result, rerender } = renderHook(() => useUser());
    await act(async () => {
      rerender();
    });

    expect(result.current?.role).toBe("viewer");
    expect(canEditPolicies(result.current!.role)).toBe(false);
    expect(canResolveEscalations(result.current!.role)).toBe(false);
  });
});
