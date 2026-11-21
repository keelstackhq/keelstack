import { Button } from "@cascivo/react";
import { useEffect, useRef, useState } from "react";
import { buildCommand, CAPTION, DEFAULT_ON, PIECES } from "../lib/command";
import "./PiecePicker.css";

export const COPIED_MS = 1400;

type CopyState = "idle" | "copied" | "failed";
const COPY_LABEL: Record<CopyState, string> = {
  idle: "Copy",
  copied: "Copied",
  failed: "Copy failed",
};

export default function PiecePicker() {
  const [on, setOn] = useState<ReadonlySet<string>>(() => new Set(DEFAULT_ON));
  const [copy, setCopy] = useState<CopyState>("idle");
  const [canCopy, setCanCopy] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    setCanCopy(typeof navigator !== "undefined" && Boolean(navigator.clipboard));
    return () => clearTimeout(timer.current);
  }, []);

  const command = buildCommand(on);

  function toggle(name: string) {
    setOn((prev) => {
      const next = new Set(prev);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  }

  async function onCopy() {
    clearTimeout(timer.current);
    let result: CopyState = "copied";
    try {
      await navigator.clipboard.writeText(command);
    } catch {
      result = "failed";
    }
    clearTimeout(timer.current);
    setCopy(result);
    timer.current = setTimeout(() => setCopy("idle"), COPIED_MS);
  }

  return (
    <div className="pp">
      <div className="pp-chips">
        <span className="pp-base">base · always</span>
        {PIECES.map(({ piece, name }) => {
          const active = on.has(name);
          return (
            <Button
              key={name}
              type="button"
              variant="ghost"
              className="pp-chip"
              aria-pressed={active}
              data-on={active}
              onClick={() => toggle(name)}
            >
              <span aria-hidden="true">{active ? "✓" : "+"}</span> {piece}:{name}
            </Button>
          );
        })}
      </div>
      <div className="pp-card">
        <div className="pp-bar">
          <span className="pp-label">sh</span>
          {canCopy && (
            <Button type="button" variant="ghost" className="pp-copy" onClick={onCopy}>
              {COPY_LABEL[copy]}
            </Button>
          )}
        </div>
        <pre className="pp-code">
          <code>{command}</code>
        </pre>
      </div>
      <p className="pp-caption">{CAPTION}</p>
      <span aria-live="polite" className="visually-hidden">
        {copy === "copied" ? "Copied to clipboard" : copy === "failed" ? "Copy failed" : ""}
      </span>
    </div>
  );
}
