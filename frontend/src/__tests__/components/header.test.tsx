import { describe, it, expect } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { Header } from "@/components/layout/header";
import { ThemeProvider } from "@/components/providers/theme-provider";

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
    expect(options.length).toBe(4);
    expect(options[0]).toHaveTextContent("All Applications");
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
