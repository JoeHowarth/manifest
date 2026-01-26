import { useRef, useLayoutEffect, useState } from "react";

interface ValueWithDiffProps {
  value: number;
  tick: number; // Current simulation tick to detect changes
  format?: (n: number) => string;
  width?: number; // Fixed width to prevent layout shifts
}

/**
 * Displays a numeric value with diff from previous tick shown by default.
 */
export function ValueWithDiff({
  value,
  tick,
  format = (n) => n.toFixed(1),
  width,
}: ValueWithDiffProps) {
  // Store previous tick's value
  const prevValueRef = useRef<{ value: number; tick: number } | null>(null);
  const [diff, setDiff] = useState(0);

  // Use layout effect to update synchronously before paint
  useLayoutEffect(() => {
    const prev = prevValueRef.current;

    if (prev === null) {
      // First render - no diff
      prevValueRef.current = { value, tick };
      setDiff(0);
    } else if (prev.tick !== tick) {
      // Tick changed - calculate diff from previous tick's value
      const newDiff = value - prev.value;
      setDiff(newDiff);
      // Store current value for next tick
      prevValueRef.current = { value, tick };
    } else {
      // Same tick, value might have updated - just store it
      prevValueRef.current = { value, tick };
    }
  }, [tick, value]);

  const diffColor = diff > 0 ? "#4caf50" : diff < 0 ? "#f44336" : "#666";
  const diffText =
    Math.abs(diff) > 0.01 ? (diff > 0 ? `+${format(diff)}` : format(diff)) : "";

  return (
    <span
      style={{
        display: "inline-block",
        minWidth: width,
        textAlign: "right",
      }}
    >
      {format(value)}
      {diffText && (
        <span style={{ color: diffColor, fontSize: "0.85em", marginLeft: 4 }}>
          {diffText}
        </span>
      )}
    </span>
  );
}

// Simpler version for when you just want fixed-width numeric display
interface FixedValueProps {
  value: number;
  format?: (n: number) => string;
  width?: number;
}

export function FixedValue({
  value,
  format = (n) => n.toFixed(0),
  width = 40,
}: FixedValueProps) {
  return (
    <span
      style={{
        display: "inline-block",
        minWidth: width,
        textAlign: "right",
      }}
    >
      {format(value)}
    </span>
  );
}
