import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

const mockPush = vi.fn();
const mockReplace = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: mockPush,
    replace: mockReplace,
    back: vi.fn(),
    prefetch: vi.fn(),
  }),
  usePathname: () => "/login",
  useSearchParams: () => new URLSearchParams(),
}));

import LoginPage from "@/app/login/page";

describe("Login Flow Integration", () => {
  beforeEach(() => {
    sessionStorage.clear();
    mockPush.mockReset();
    mockReplace.mockReset();
  });

  afterEach(() => {
    sessionStorage.clear();
  });

  it("renders the login form with demo accounts", () => {
    render(<LoginPage />);

    expect(screen.getByText("ControlPlane")).toBeInTheDocument();
    expect(screen.getByText("admin@controlplane.ai")).toBeInTheDocument();
    expect(screen.getByText("viewer@controlplane.ai")).toBeInTheDocument();
    expect(screen.getByText("reviewer@controlplane.ai")).toBeInTheDocument();
  });

  it("shows role badges for each demo account", () => {
    render(<LoginPage />);

    expect(screen.getByText("admin")).toBeInTheDocument();
    expect(screen.getByText("reviewer")).toBeInTheDocument();
    expect(screen.getByText("viewer")).toBeInTheDocument();
  });

  it("fills email/password when clicking a demo account button", () => {
    render(<LoginPage />);

    const adminButton = screen.getByText("admin@controlplane.ai");
    fireEvent.click(adminButton);

    const emailInput = screen.getByPlaceholderText("admin@controlplane.ai") as HTMLInputElement;
    expect(emailInput.value).toBe("admin@controlplane.ai");
  });

  it("submitting form with valid credentials stores user and redirects", async () => {
    render(<LoginPage />);

    // Fill the form
    const emailInput = screen.getByPlaceholderText("admin@controlplane.ai");
    const passwordInput = screen.getByPlaceholderText("••••••••");

    fireEvent.change(emailInput, { target: { value: "admin@controlplane.ai" } });
    fireEvent.change(passwordInput, { target: { value: "admin123" } });

    // Submit
    const submitButton = screen.getByText("Sign In");
    fireEvent.click(submitButton);

    await waitFor(() => {
      const stored = sessionStorage.getItem("cp-user");
      expect(stored).not.toBeNull();
      const user = JSON.parse(stored!);
      expect(user.email).toBe("admin@controlplane.ai");
      expect(user.role).toBe("admin");
    });

    await waitFor(() => {
      const token = sessionStorage.getItem("cp-token");
      expect(token).not.toBeNull();
      expect(token!.length).toBeGreaterThan(0);
    });

    expect(mockPush).toHaveBeenCalledWith("/");
  });

  it("shows error on invalid credentials", async () => {
    render(<LoginPage />);

    const emailInput = screen.getByPlaceholderText("admin@controlplane.ai");
    const passwordInput = screen.getByPlaceholderText("••••••••");

    fireEvent.change(emailInput, { target: { value: "wrong@email.com" } });
    fireEvent.change(passwordInput, { target: { value: "wrongpassword" } });

    const submitButton = screen.getByText("Sign In");
    fireEvent.click(submitButton);

    await waitFor(() => {
      expect(screen.getByText("Invalid email or password")).toBeInTheDocument();
    });

    expect(sessionStorage.getItem("cp-token")).toBeNull();
  });
});
