import type { StateSnapshot } from "../types";
import type { Selection, SelectionItem } from "../App";

interface SidebarProps {
  state: StateSnapshot;
  selection: Selection;
  onSelect: (item: SelectionItem | null) => void;
  onAdvanceTick: () => void;
}

function isSelected(selection: Selection, type: SelectionItem["type"], id: number): boolean {
  return selection.some((s) => s.type === type && s.id === id);
}

export function Sidebar({
  state,
  selection,
  onSelect,
  onAdvanceTick,
}: SidebarProps) {
  const playerOrg = state.orgs[0];

  return (
    <div
      style={{
        width: 280,
        height: "100%",
        backgroundColor: "#16213e",
        borderLeft: "1px solid #0f3460",
        color: "#e4e4e4",
        fontFamily: "system-ui, sans-serif",
        fontSize: 14,
        overflowY: "auto",
        padding: 16,
      }}
    >
      <h1
        style={{
          fontSize: 18,
          fontWeight: 600,
          marginBottom: 16,
          color: "#fff",
        }}
      >
        Manifest
      </h1>

      {/* Time controls */}
      <Section title="Time">
        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 12,
          }}
        >
          <span>Day {state.tick}</span>
          <button
            onClick={onAdvanceTick}
            style={{
              padding: "6px 12px",
              backgroundColor: "#4a90a4",
              border: "none",
              borderRadius: 4,
              color: "white",
              cursor: "pointer",
              fontSize: 13,
            }}
          >
            Advance Day
          </button>
        </div>
      </Section>

      {/* Player info */}
      {playerOrg && (
        <Section title="Your Organization">
          <div style={{ marginBottom: 8 }}>{playerOrg.name}</div>
          <div style={{ color: "#f4d03f" }}>
            Treasury: {playerOrg.treasury.toLocaleString()} silver
          </div>
        </Section>
      )}

      {/* Ships */}
      <Section title="Your Ships">
        {state.ships
          .filter((s) => s.owner === playerOrg?.id)
          .map((ship) => {
            const location = state.settlements.find(
              (s) => s.id === ship.location
            );
            const selected = isSelected(selection, "ship", ship.id);
            return (
              <div
                key={ship.id}
                onClick={() => onSelect({ type: "ship", id: ship.id })}
                style={{
                  padding: 8,
                  backgroundColor: selected ? "#2a3a5e" : "#1a1a2e",
                  borderRadius: 4,
                  marginBottom: 8,
                  cursor: "pointer",
                  border: selected
                    ? "1px solid #4a90a4"
                    : "1px solid transparent",
                }}
              >
                <div style={{ fontWeight: 500, marginBottom: 4 }}>
                  {ship.name}
                </div>
                <div style={{ fontSize: 12, color: "#aaa" }}>
                  {ship.status === "InPort"
                    ? `At ${location?.name ?? "Unknown"}`
                    : `En route (${ship.days_remaining} days)`}
                </div>
              </div>
            );
          })}
      </Section>

      {/* Settlements */}
      <Section title="Settlements">
        {state.settlements.map((settlement) => {
          const selected = isSelected(selection, "settlement", settlement.id);
          return (
            <div
              key={settlement.id}
              onClick={() =>
                onSelect({ type: "settlement", id: settlement.id })
              }
              style={{
                padding: 8,
                backgroundColor: selected ? "#2a3a5e" : "#1a1a2e",
                borderRadius: 4,
                marginBottom: 8,
                cursor: "pointer",
                border: selected
                  ? "1px solid #4a90a4"
                  : "1px solid transparent",
              }}
            >
              <div style={{ fontWeight: 500, marginBottom: 4 }}>
                {settlement.name}
              </div>
              <div style={{ fontSize: 12, color: "#aaa" }}>
                Pop: {settlement.population.toLocaleString()} · Wealth:{" "}
                {settlement.wealth.toFixed(1)}
              </div>
            </div>
          );
        })}
      </Section>
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div style={{ marginBottom: 20 }}>
      <h2
        style={{
          fontSize: 12,
          textTransform: "uppercase",
          letterSpacing: 1,
          color: "#888",
          marginBottom: 8,
        }}
      >
        {title}
      </h2>
      {children}
    </div>
  );
}
