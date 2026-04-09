function fallbackCopyWithCopyEvent(text: string): boolean {
  if (typeof document === "undefined") {
    return false;
  }

  let copied = false;
  const handleCopy = (event: ClipboardEvent) => {
    event.preventDefault();
    event.clipboardData?.setData("text/plain", text);
    copied = true;
  };

  document.addEventListener("copy", handleCopy, { capture: true, once: true });

  try {
    copied = document.execCommand("copy") || copied;
  } catch {
    copied = false;
  }

  return copied;
}

function fallbackCopyWithTextarea(text: string): boolean {
  if (typeof document === "undefined") {
    return false;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.top = "0";
  textarea.style.left = "-9999px";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);

  textarea.focus();
  textarea.select();
  textarea.setSelectionRange(0, textarea.value.length);

  let copied = false;
  try {
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  } finally {
    document.body.removeChild(textarea);
  }

  return copied;
}

export async function copyText(text: string): Promise<boolean> {
  if (!text) {
    return false;
  }

  if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // Fall through to legacy copy strategies.
    }
  }

  if (fallbackCopyWithCopyEvent(text)) {
    return true;
  }

  return fallbackCopyWithTextarea(text);
}
