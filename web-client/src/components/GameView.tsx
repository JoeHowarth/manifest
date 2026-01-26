import { Application, extend } from "@pixi/react";
import { Container, Graphics, Text } from "pixi.js";
import { useRef, useState, useEffect } from "react";
import type { StateSnapshot } from "../types";
import type { Selection, SelectionItem } from "../App";
import { SettlementNode, ShipMarker, RoutesLayer, COLORS } from "./map";

// Register Pixi.js components with @pixi/react
extend({ Container, Graphics, Text });

interface GameViewProps {
  state: StateSnapshot;
  selection: Selection;
  onSelect: (item: SelectionItem | null) => void;
}

function isSelected(selection: Selection, type: SelectionItem["type"], id: number): boolean {
  return selection.some((s) => s.type === type && s.id === id);
}

export function GameView({ state, selection, onSelect }: GameViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
  }, []);

  return (
    <div ref={containerRef} style={{ flex: 1, height: "100%", minWidth: 0 }}>
      {mounted && containerRef.current && (
        <Application
          background={COLORS.background}
          resizeTo={containerRef.current}
          resolution={window.devicePixelRatio || 1}
          autoDensity={true}
        >
          {/* Background - no action on click for multi-select */}
          <pixiGraphics
            draw={(g) => {
              g.clear();
              g.rect(0, 0, 10000, 10000);
              g.fill({ color: 0x000000, alpha: 0 });
            }}
            eventMode="static"
          />
          <pixiContainer>
            {/* Routes layer */}
            <RoutesLayer
              routes={state.routes}
              settlements={state.settlements}
            />

            {/* Settlements layer */}
            {state.settlements.map((settlement) => (
              <SettlementNode
                key={settlement.id}
                settlement={settlement}
                isSelected={isSelected(selection, "settlement", settlement.id)}
                onSelect={() =>
                  onSelect({ type: "settlement", id: settlement.id })
                }
              />
            ))}

            {/* Ships layer */}
            {state.ships.map((ship, index) => {
              const settlement = state.settlements.find(
                (s) => s.id === ship.location
              );
              if (!settlement || ship.status !== "InPort") return null;
              return (
                <ShipMarker
                  key={ship.id}
                  x={settlement.position[0] + 30}
                  y={settlement.position[1] + 10 + index * 20}
                  name={ship.name}
                  isSelected={isSelected(selection, "ship", ship.id)}
                  onSelect={() => onSelect({ type: "ship", id: ship.id })}
                />
              );
            })}
          </pixiContainer>
        </Application>
      )}
    </div>
  );
}
