import type { SettlementSnapshot, ShipSnapshot } from "../../types";
import { CollapsibleSection } from "../ui/CollapsibleSection";
import { FixedValue, ValueWithDiff } from "../ui/ValueWithDiff";

interface SettlementPanelProps {
  settlement: SettlementSnapshot;
  shipsAtPort: ShipSnapshot[];
  onSelectShip: (id: number) => void;
  onDeselect: () => void;
  tick: number;
}

export function SettlementPanel({
  settlement,
  shipsAtPort,
  onSelectShip,
  onDeselect,
  tick,
}: SettlementPanelProps) {
  return (
    <div
      style={{
        padding: 12,
        backgroundColor: "#1a1a2e",
        borderRadius: 4,
        border: "1px solid #4a90a4",
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
        <span style={{ fontWeight: 600, fontSize: 15, color: "#4a90a4" }}>{settlement.name}</span>
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
        <DiffRow label="Population" value={settlement.population} tick={tick} format={(n) => n.toLocaleString()} />
        <DiffRow label="Wealth" value={settlement.wealth} tick={tick} format={(n) => n.toFixed(1)} />
        <DiffRow label="Satisfaction" value={settlement.provision_satisfaction * 100} tick={tick} format={(n) => `${n.toFixed(0)}%`} />
      </div>

      {/* Ships in port */}
      {shipsAtPort.length > 0 && (
        <CollapsibleSection title={`Ships in Port (${shipsAtPort.length})`} defaultOpen>
          {shipsAtPort.map((ship) => (
            <div
              key={ship.id}
              onClick={() => onSelectShip(ship.id)}
              style={{
                fontSize: 12,
                color: "#f4d03f",
                cursor: "pointer",
                padding: "2px 0",
              }}
            >
              {ship.name}
            </div>
          ))}
        </CollapsibleSection>
      )}

      {/* Market */}
      <CollapsibleSection title="Market">
        <MarketContent settlement={settlement} tick={tick} />
      </CollapsibleSection>

      {/* Labor */}
      <CollapsibleSection title="Labor">
        <LaborContent settlement={settlement} tick={tick} />
      </CollapsibleSection>

      {/* Facilities */}
      {settlement.facilities.length > 0 && (
        <CollapsibleSection title={`Facilities (${settlement.facilities.length})`}>
          <FacilitiesContent settlement={settlement} />
        </CollapsibleSection>
      )}

      {/* Inventory */}
      {settlement.total_inventory.length > 0 && (
        <CollapsibleSection title="Warehouse">
          <InventoryContent settlement={settlement} tick={tick} />
        </CollapsibleSection>
      )}
    </div>
  );
}

function DiffRow({ label, value, tick, format }: { label: string; value: number; tick: number; format?: (n: number) => string }) {
  return (
    <div
      style={{
        display: "flex",
        justifyContent: "space-between",
        marginBottom: 2,
      }}
    >
      <span style={{ color: "#888" }}>{label}</span>
      <ValueWithDiff value={value} tick={tick} format={format} />
    </div>
  );
}

function MarketContent({ settlement, tick }: { settlement: SettlementSnapshot; tick: number }) {
  if (settlement.prices.length === 0) {
    return <div style={{ color: "#666", fontSize: 12 }}>No market data</div>;
  }

  const sortedPrices = [...settlement.prices].sort((a, b) =>
    a.good.localeCompare(b.good)
  );

  return (
    <table style={{ width: "100%", fontSize: 11, color: "#ccc" }}>
      <thead>
        <tr style={{ textAlign: "left", color: "#666" }}>
          <th style={{ paddingBottom: 4, fontWeight: 500 }}>Good</th>
          <th style={{ paddingBottom: 4, fontWeight: 500, width: 60, textAlign: "right" }}>Price</th>
          <th style={{ paddingBottom: 4, fontWeight: 500, width: 60, textAlign: "right" }}>Stock</th>
          <th style={{ paddingBottom: 4, fontWeight: 500, width: 45, textAlign: "right" }}>Sold</th>
        </tr>
      </thead>
      <tbody>
        {sortedPrices.map((p) => (
          <tr key={p.good}>
            <td style={{ paddingBottom: 2 }}>{p.good}</td>
            <td style={{ textAlign: "right" }}>
              <ValueWithDiff value={p.price} tick={tick} format={(n) => n.toFixed(1)} width={55} />
            </td>
            <td style={{ textAlign: "right" }}>
              <ValueWithDiff value={p.available} tick={tick} width={55} />
            </td>
            <td style={{ textAlign: "right" }}>
              <FixedValue value={p.last_traded} width={40} />
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function LaborContent({ settlement, tick }: { settlement: SettlementSnapshot; tick: number }) {
  const employmentRate =
    settlement.labor_supply > 0
      ? Math.min(100, (settlement.labor_demand / settlement.labor_supply) * 100)
      : 0;

  return (
    <div style={{ fontSize: 12, color: "#ccc" }}>
      <DiffRow label="Wage" value={settlement.wage} tick={tick} format={(n) => `${n.toFixed(1)}/day`} />
      <DiffRow label="Supply" value={settlement.labor_supply} tick={tick} format={(n) => n.toFixed(0)} />
      <DiffRow label="Demand" value={settlement.labor_demand} tick={tick} format={(n) => n.toFixed(0)} />
      <DiffRow label="Employment" value={employmentRate} tick={tick} format={(n) => `${n.toFixed(0)}%`} />
    </div>
  );
}

function FacilitiesContent({ settlement }: { settlement: SettlementSnapshot }) {
  return (
    <div style={{ fontSize: 11, color: "#ccc" }}>
      {settlement.facilities.map((f) => (
        <div
          key={f.id}
          style={{
            display: "flex",
            justifyContent: "space-between",
            marginBottom: 2,
          }}
        >
          <span>{f.kind}</span>
          <span style={{ color: "#888" }}>
            {f.workers}/{f.optimal_workers} ({(f.efficiency * 100).toFixed(0)}%)
          </span>
        </div>
      ))}
    </div>
  );
}

function InventoryContent({ settlement, tick }: { settlement: SettlementSnapshot; tick: number }) {
  const sortedInventory = [...settlement.total_inventory].sort((a, b) =>
    a[0].localeCompare(b[0])
  );

  return (
    <div style={{ fontSize: 11, color: "#ccc" }}>
      {sortedInventory.map(([good, qty]) => (
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
  );
}
