/**
 * Copy text to the clipboard, returning whether it worked.
 *
 * `navigator.clipboard` only exists in secure contexts (HTTPS or localhost). The
 * dashboard is usually served over plain HTTP on a custom host name
 * (http://controlplane-ai.rtxcore), where it is undefined — so fall back to a hidden
 * textarea + `document.execCommand("copy")`, which browsers still honour on a click.
 */
export async function copyText(text: string): Promise<boolean> {
  if (typeof window !== "undefined" && window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Permission denied or document not focused — try the fallback below.
    }
  }
  return legacyCopy(text);
}

function legacyCopy(text: string): boolean {
  if (typeof document === "undefined") return false;
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  // Off-screen but still selectable; avoids scrolling or a visible flash.
  textarea.style.position = "fixed";
  textarea.style.top = "-1000px";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  const previousFocus = document.activeElement as HTMLElement | null;
  // Focus first: some browsers only copy a selection inside the focused element.
  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, text.length);
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  document.body.removeChild(textarea);
  previousFocus?.focus?.();
  return ok;
}
