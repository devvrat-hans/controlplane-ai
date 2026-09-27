import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ThemeProvider, useTheme } from "@/components/providers/theme-provider";

function ThemeDisplay() {
  const { theme, toggle } = useTheme();
  return (
    <div>
      <span data-testid="theme-value">{theme}</span>
      <button onClick={toggle}>Toggle</button>
    </div>
  );
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.classList.remove("dark", "light");
});

describe("ThemeProvider", () => {
  it("defaults to dark theme", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );
    expect(screen.getByTestId("theme-value")).toHaveTextContent("dark");
  });

  it("toggles from dark to light", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );

    fireEvent.click(screen.getByText("Toggle"));
    expect(screen.getByTestId("theme-value")).toHaveTextContent("light");
  });

  it("toggles from light back to dark", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );

    fireEvent.click(screen.getByText("Toggle"));
    expect(screen.getByTestId("theme-value")).toHaveTextContent("light");

    fireEvent.click(screen.getByText("Toggle"));
    expect(screen.getByTestId("theme-value")).toHaveTextContent("dark");
  });

  it("persists theme to localStorage", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );

    fireEvent.click(screen.getByText("Toggle"));
    expect(localStorage.getItem("cp-theme")).toBe("light");
  });

  it("reads stored theme from localStorage on mount", () => {
    localStorage.setItem("cp-theme", "dark");

    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );

    expect(screen.getByTestId("theme-value")).toHaveTextContent("dark");
  });

  it("applies theme class to document element", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );

    expect(document.documentElement.classList.contains("dark")).toBe(true);

    fireEvent.click(screen.getByText("Toggle"));
    expect(document.documentElement.classList.contains("light")).toBe(true);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });
});

describe("useTheme", () => {
  it("provides theme context to children", () => {
    render(
      <ThemeProvider>
        <ThemeDisplay />
      </ThemeProvider>
    );
    expect(screen.getByTestId("theme-value")).toBeInTheDocument();
  });

  it("returns default values outside provider", () => {
    render(<ThemeDisplay />);
    expect(screen.getByTestId("theme-value")).toHaveTextContent("dark");
  });
});
