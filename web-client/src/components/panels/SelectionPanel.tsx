import type { StateSnapshot } from "../../types";
import type { Selection, SelectionItem } from "../../App";
import { SettlementPanel } from "./SettlementPanel";
import { ShipPanel } from "./ShipPanel";

interface SelectionPanelProps {
  state: StateSnapshot;
  selection: Selection;
  onToggle: (item: SelectionItem | null) => void;
  onRemove: (item: SelectionItem) => void;
  tick: number;
}

export function SelectionPanel({ state, selection, onToggle, onRemove, tick }: SelectionPanelProps) {
  if (selection.length === 0) return null;

  return (
    <div
      style={{
        position: "absolute",
        top: 16,
        right: 296, // sidebar width (280) + gap (16)
        width: 320,
        maxHeight: "calc(100vh - 32px)",
        overflowY: "auto",
        zIndex: 100,
        display: "flex",
        flexDirection: "column",
        gap: 8,
      }}
    >
      {selection.map((item) => {
        if (item.type === "settlement") {
          const settlement = state.settlements.find((s) => s.id === item.id);
          if (!settlement) return null;
          return (
            <SettlementPanel
              key={`settlement-${item.id}`}
              settlement={settlement}
              shipsAtPort={state.ships.filter(
                (s) => s.location === settlement.id && s.status === "InPort"
              )}
              onSelectShip={(id) => onToggle({ type: "ship", id })}
              onDeselect={() => onRemove(item)}
              tick={tick}
            />
          );
        } else {
          const ship = state.ships.find((s) => s.id === item.id);
          if (!ship) return null;
          return (
            <ShipPanel
              key={`ship-${item.id}`}
              ship={ship}
              location={state.settlements.find((s) => s.id === ship.location)}
              onSelectSettlement={(id) => onToggle({ type: "settlement", id })}
              onDeselect={() => onRemove(item)}
              tick={tick}
            />
          );
        }
      })}
    </div>
  );
}
