import { describe, it, expect, vi, afterEach } from "vitest";
import { copyText } from "@/lib/clipboard";

function setSecureContext(secure: boolean) {
  Object.defineProperty(window, "isSecureContext", { value: secure, configurable: true });
}

afterEach(() => {
  vi.restoreAllMocks();
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
});

describe("copyText", () => {
  it("uses the Clipboard API in a secure context", async () => {
    setSecureContext(true);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    await expect(copyText("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("falls back to execCommand over plain HTTP, where navigator.clipboard is missing", async () => {
    // e.g. http://controlplane-ai.rtxcore — not a secure context.
    setSecureContext(false);
    let copied = "";
    const exec = vi.fn(() => {
      copied = (document.activeElement as HTMLTextAreaElement).value;
      return true;
    });
    Object.defineProperty(document, "execCommand", { value: exec, configurable: true });

    await expect(copyText("claude mcp add --transport http controlplane")).resolves.toBe(true);
    expect(exec).toHaveBeenCalledWith("copy");
    expect(copied).toBe("claude mcp add --transport http controlplane");
    // The temporary textarea is cleaned up.
    expect(document.querySelectorAll("textarea")).toHaveLength(0);
  });

  it("reports failure instead of throwing when nothing can copy", async () => {
    setSecureContext(false);
    Object.defineProperty(document, "execCommand", { value: () => { throw new Error("blocked"); }, configurable: true });

    await expect(copyText("x")).resolves.toBe(false);
  });
});
