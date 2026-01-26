import { useState, ReactNode } from "react";

interface CollapsibleSectionProps {
  title: string;
  defaultOpen?: boolean;
  children: ReactNode;
}

export function CollapsibleSection({
  title,
  defaultOpen = false,
  children,
}: CollapsibleSectionProps) {
  const [isOpen, setIsOpen] = useState(defaultOpen);

  return (
    <div style={{ marginBottom: 8 }}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        style={{
          width: "100%",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "6px 8px",
          background: "#2a3a5e",
          border: "none",
          borderRadius: 4,
          color: "#ccc",
          fontSize: 12,
          fontWeight: 500,
          cursor: "pointer",
          textAlign: "left",
        }}
      >
        <span>{title}</span>
        <span style={{ color: "#888", fontSize: 10 }}>{isOpen ? "▼" : "▶"}</span>
      </button>
      {isOpen && (
        <div
          style={{
            padding: "8px 8px 4px",
            background: "#1a1a2e",
            borderRadius: "0 0 4px 4px",
            marginTop: 1,
          }}
        >
          {children}
        </div>
      )}
    </div>
  );
}
