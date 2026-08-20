import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Sidebar, MobileSidebar } from "@/components/layout/sidebar";

describe("Sidebar", () => {
  it("renders the brand name", () => {
    render(<Sidebar />);
    expect(screen.getByText("ControlPlane")).toBeInTheDocument();
  });

  it("renders all navigation items", () => {
    render(<Sidebar />);
    const navItems = [
      "Overview",
      "Live Stream",
      "Policies",
      "Escalations",
      "Cost",
      "Audit",
      "Settings",
    ];
    for (const item of navItems) {
      expect(screen.getByText(item)).toBeInTheDocument();
    }
  });

  it("renders correct navigation links", () => {
    render(<Sidebar />);
    const links = screen.getAllByRole("link");
    const hrefs = links.map((link) => link.getAttribute("href"));

    expect(hrefs).toContain("/");
    expect(hrefs).toContain("/stream");
    expect(hrefs).toContain("/policies");
    expect(hrefs).toContain("/escalations");
    expect(hrefs).toContain("/cost");
    expect(hrefs).toContain("/audit");
    expect(hrefs).toContain("/settings");
  });

  it("shows system health indicator", () => {
    render(<Sidebar />);
    expect(screen.getByText("System healthy")).toBeInTheDocument();
  });

  it("shows version number", () => {
    render(<Sidebar />);
    expect(screen.getByText("v0.1.0")).toBeInTheDocument();
  });
});

describe("MobileSidebar", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <MobileSidebar open={false} onClose={() => {}} />
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders navigation when open", () => {
    render(<MobileSidebar open={true} onClose={() => {}} />);
    expect(screen.getByText("Overview")).toBeInTheDocument();
    expect(screen.getByText("Live Stream")).toBeInTheDocument();
  });

  it("renders close button when open", () => {
    render(<MobileSidebar open={true} onClose={() => {}} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
  });
});
