import { useEffect, useState } from "preact/hooks";

interface Props {
  text: string;
  label: string;
}

// Progressive enhancement: renders nothing until it knows the Clipboard API exists, so the page
// (and the selectable snippet) work the same without JS.
export function CopyButton({ text, label }: Props) {
  const [supported, setSupported] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    setSupported(typeof navigator !== "undefined" && Boolean(navigator.clipboard?.writeText));
  }, []);

  if (!supported) return null;

  async function copy() {
    setMessage("");
    try {
      await navigator.clipboard.writeText(text);
      setMessage("Copied");
    } catch {
      setMessage("Copy failed; select the text instead");
    }
  }

  return (
    <span class="copy">
      <button type="button" class="copy-button" aria-label={label} onClick={copy}>
        Copy
      </button>
      <span class="copy-status" aria-live="polite">
        {message}
      </span>
    </span>
  );
}
