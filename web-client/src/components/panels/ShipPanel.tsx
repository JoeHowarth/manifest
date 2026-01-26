import type { SettlementSnapshot, ShipSnapshot } from "../../types";
import { CollapsibleSection } from "../ui/CollapsibleSection";
import { ValueWithDiff } from "../ui/ValueWithDiff";

interface ShipPanelProps {
  ship: ShipSnapshot;
  location: SettlementSnapshot | undefined;
  onSelectSettlement: (id: number) => void;
  onDeselect: () => void;
  tick: number;
}

export function ShipPanel({
  ship,
  location,
  onSelectSettlement,
  onDeselect,
  tick,
}: ShipPanelProps) {
  const sortedCargo = [...ship.cargo].sort((a, b) => a[0].localeCompare(b[0]));
  const totalCargo = ship.cargo.reduce((sum, [, qty]) => sum + qty, 0);

  return (
    <div
      style={{
        padding: 12,
        backgroundColor: "#1a1a2e",
        borderRadius: 4,
        border: "1px solid #f4d03f",
      }}
    >
      {/* Header */}
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 12,
        }}
      >
        <span style={{ fontWeight: 600, fontSize: 15, color: "#f4d03f" }}>
          {ship.name}
        </span>
        <button
          onClick={onDeselect}
          style={{
            background: "none",
            border: "none",
            color: "#888",
            cursor: "pointer",
            fontSize: 16,
            padding: "0 4px",
          }}
        >
          x
        </button>
      </div>

      {/* Overview - always visible */}
      <div style={{ marginBottom: 12, fontSize: 13, color: "#ccc" }}>
        <Row
          label="Status"
          value={ship.status === "InPort" ? "In Port" : "En Route"}
        />
        {location && (
          <div
            style={{
              display: "flex",
              justifyContent: "space-between",
              marginBottom: 2,
            }}
          >
            <span style={{ color: "#888" }}>Location</span>
            <span
              onClick={() => onSelectSettlement(location.id)}
              style={{ color: "#4a90a4", cursor: "pointer" }}
            >
              {location.name}
            </span>
          </div>
        )}
        {ship.status === "EnRoute" && ship.days_remaining > 0 && (
          <Row label="Arrives in" value={`${ship.days_remaining} days`} />
        )}
      </div>

      {/* Cargo */}
      <CollapsibleSection
        title={`Cargo (${totalCargo.toFixed(0)}/${ship.capacity.toFixed(0)})`}
        defaultOpen
      >
        {sortedCargo.length > 0 ? (
          <div style={{ fontSize: 11, color: "#ccc" }}>
            {sortedCargo.map(([good, qty]) => (
              <div
                key={good}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  marginBottom: 2,
                }}
              >
                <span>{good}</span>
                <ValueWithDiff value={qty} tick={tick} width={60} />
              </div>
            ))}
          </div>
        ) : (
          <div style={{ fontSize: 12, color: "#666" }}>Empty</div>
        )}
      </CollapsibleSection>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        marginBottom: 2,
      }}
    >
      <span style={{ color: "#888" }}>{label}</span>
      <span>{value}</span>
    </div>
  );
}
