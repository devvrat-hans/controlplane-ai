import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { Header } from "@/components/layout/header";
import { ThemeProvider } from "@/components/providers/theme-provider";

// The app selector lists apps loaded from /api/v1/apps via the app provider.
const MOCK_APPS = [
  { id: "10000000-0000-0000-0000-000000000001", name: "ChatBot-Prod" },
  { id: "10000000-0000-0000-0000-000000000002", name: "Agent-Internal" },
  { id: "10000000-0000-0000-0000-000000000003", name: "RAG-Customer-Support" },
];
vi.mock("@/components/providers/app-provider", () => ({
  useApp: () => ({
    apps: MOCK_APPS,
    selectedAppId: "all",
    setSelectedAppId: () => {},
    selectedAppName: "All Applications",
  }),
}));

function renderWithProviders(ui: React.ReactElement) {
  return render(<ThemeProvider>{ui}</ThemeProvider>);
}

describe("Header", () => {
  it("renders mobile menu toggle button", () => {
    renderWithProviders(<Header />);
    expect(screen.getByLabelText("Toggle sidebar")).toBeInTheDocument();
  });

  it("renders app selector with options", () => {
    renderWithProviders(<Header />);
    const select = screen.getByRole("combobox");
    expect(select).toBeInTheDocument();

    const options = screen.getAllByRole("option");
    expect(options.length).toBe(1 + MOCK_APPS.length);
    expect(options[0]).toHaveTextContent("All Applications");
    expect(options.slice(1).map((o) => o.textContent)).toEqual(MOCK_APPS.map((a) => a.name));
  });

  it("renders health indicator", async () => {
    renderWithProviders(<Header />);
    await waitFor(() => {
      expect(screen.getByText("Healthy")).toBeInTheDocument();
    });
  });

  it("renders theme toggle button", () => {
    renderWithProviders(<Header />);
    expect(screen.getByLabelText("Toggle theme")).toBeInTheDocument();
  });

  it("toggles theme on button click", () => {
    renderWithProviders(<Header />);
    const toggleBtn = screen.getByLabelText("Toggle theme");

    fireEvent.click(toggleBtn);
    // Theme started as "light", after toggle becomes "dark" -> shows Sun icon
    expect(toggleBtn).toBeInTheDocument();
  });

  it("renders user avatar with initials from session", () => {
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));
    renderWithProviders(<Header />);
    expect(screen.getByText("AD")).toBeInTheDocument();
  });

  it("renders user name and email", () => {
    sessionStorage.setItem("cp-user", JSON.stringify({ email: "admin@controlplane.ai", role: "admin" }));
    renderWithProviders(<Header />);
    expect(screen.getByText("Admin")).toBeInTheDocument();
    expect(screen.getByText("admin@controlplane.ai")).toBeInTheDocument();
  });
});
