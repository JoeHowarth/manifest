import { Application, extend } from "@pixi/react";
import { Container, Graphics, Text, TextStyle } from "pixi.js";
import { useCallback, useState } from "react";
import type { StateSnapshot, SettlementSnapshot, Route } from "../types";

// Register Pixi.js components with @pixi/react
extend({ Container, Graphics, Text });

// Colors
const COLORS = {
  background: 0x1a1a2e,
  settlement: 0x4a90a4,
  settlementHover: 0x6ab0c4,
  routeSea: 0x3a7ca5,
  routeRiver: 0x5dade2,
  routeLand: 0x8b7355,
  text: 0xffffff,
  ship: 0xf4d03f,
};

interface GameViewProps {
  state: StateSnapshot;
}

export function GameView({ state }: GameViewProps) {
  const [hoveredSettlement, setHoveredSettlement] = useState<number | null>(
    null
  );

  return (
    <div style={{ flex: 1, height: "100%" }}>
      <Application background={COLORS.background} resizeTo={window}>
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
              isHovered={hoveredSettlement === settlement.id}
              onHover={() => setHoveredSettlement(settlement.id)}
              onHoverEnd={() => setHoveredSettlement(null)}
            />
          ))}

          {/* Ships layer */}
          {state.ships.map((ship) => {
            const settlement = state.settlements.find(
              (s) => s.id === ship.location
            );
            if (!settlement || ship.status !== "InPort") return null;
            return (
              <ShipMarker
                key={ship.id}
                x={settlement.position[0] + 30}
                y={settlement.position[1] + 10}
                name={ship.name}
              />
            );
          })}
        </pixiContainer>
      </Application>
    </div>
  );
}

interface RoutesLayerProps {
  routes: Route[];
  settlements: SettlementSnapshot[];
}

function RoutesLayer({ routes, settlements }: RoutesLayerProps) {
  const drawRoutes = useCallback(
    (g: Graphics) => {
      g.clear();

      for (const route of routes) {
        const from = settlements.find((s) => s.id === route.from);
        const to = settlements.find((s) => s.id === route.to);

        if (!from || !to) continue;

        const color =
          route.mode === "Sea"
            ? COLORS.routeSea
            : route.mode === "River"
              ? COLORS.routeRiver
              : COLORS.routeLand;

        g.moveTo(from.position[0], from.position[1]);
        g.lineTo(to.position[0], to.position[1]);
        g.stroke({ width: 2, color, alpha: 0.6 });
      }
    },
    [routes, settlements]
  );

  return <pixiGraphics draw={drawRoutes} />;
}

interface SettlementNodeProps {
  settlement: SettlementSnapshot;
  isHovered: boolean;
  onHover: () => void;
  onHoverEnd: () => void;
}

function SettlementNode({
  settlement,
  isHovered,
  onHover,
  onHoverEnd,
}: SettlementNodeProps) {
  const radius = Math.max(15, Math.min(30, settlement.population / 300));

  const drawNode = useCallback(
    (g: Graphics) => {
      g.clear();
      g.circle(0, 0, radius);
      g.fill(isHovered ? COLORS.settlementHover : COLORS.settlement);
      g.stroke({ width: 2, color: 0xffffff, alpha: 0.3 });
    },
    [radius, isHovered]
  );

  const textStyle = new TextStyle({
    fontFamily: "Arial",
    fontSize: 12,
    fill: COLORS.text,
    fontWeight: "bold",
  });

  return (
    <pixiContainer
      x={settlement.position[0]}
      y={settlement.position[1]}
      eventMode="static"
      cursor="pointer"
      onPointerEnter={onHover}
      onPointerLeave={onHoverEnd}
    >
      <pixiGraphics draw={drawNode} />
      <pixiText
        text={settlement.name}
        x={0}
        y={radius + 8}
        anchor={0.5}
        style={textStyle}
      />
      {isHovered && (
        <pixiText
          text={`Pop: ${settlement.population.toLocaleString()}`}
          x={0}
          y={radius + 22}
          anchor={0.5}
          style={
            new TextStyle({
              fontFamily: "Arial",
              fontSize: 10,
              fill: 0xaaaaaa,
            })
          }
        />
      )}
    </pixiContainer>
  );
}

interface ShipMarkerProps {
  x: number;
  y: number;
  name: string;
}

function ShipMarker({ x, y, name }: ShipMarkerProps) {
  const drawShip = useCallback((g: Graphics) => {
    g.clear();
    // Simple ship icon
    g.moveTo(-6, 4);
    g.lineTo(6, 4);
    g.lineTo(8, 0);
    g.lineTo(0, -6);
    g.lineTo(-8, 0);
    g.closePath();
    g.fill(COLORS.ship);
  }, []);

  return (
    <pixiContainer x={x} y={y} eventMode="static" cursor="pointer">
      <pixiGraphics draw={drawShip} />
      <pixiText
        text={name}
        x={12}
        y={0}
        anchor={{ x: 0, y: 0.5 }}
        style={
          new TextStyle({
            fontFamily: "Arial",
            fontSize: 9,
            fill: COLORS.ship,
          })
        }
      />
    </pixiContainer>
  );
}
