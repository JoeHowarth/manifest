import { Graphics, TextStyle, FederatedPointerEvent } from "pixi.js";
import { useCallback } from "react";
import { COLORS } from "./colors";

interface ShipMarkerProps {
  x: number;
  y: number;
  name: string;
  isSelected: boolean;
  onSelect: () => void;
}

export function ShipMarker({
  x,
  y,
  name,
  isSelected,
  onSelect,
}: ShipMarkerProps) {
  const drawShip = useCallback(
    (g: Graphics) => {
      g.clear();
      // Simple ship icon
      g.moveTo(-6, 4);
      g.lineTo(6, 4);
      g.lineTo(8, 0);
      g.lineTo(0, -6);
      g.lineTo(-8, 0);
      g.closePath();
      g.fill(isSelected ? 0xffffff : COLORS.ship);
      if (isSelected) {
        g.stroke({ width: 2, color: COLORS.ship });
      }
    },
    [isSelected]
  );

  return (
    <pixiContainer
      x={x}
      y={y}
      eventMode="static"
      cursor="pointer"
      onPointerDown={(e: FederatedPointerEvent) => {
        e.stopPropagation();
        onSelect();
      }}
    >
      <pixiGraphics draw={drawShip} />
      <pixiText
        text={name}
        x={12}
        y={0}
        anchor={{ x: 0, y: 0.5 }}
        style={
          new TextStyle({
            fontFamily: "system-ui, -apple-system, sans-serif",
            fontSize: 10,
            fill: isSelected ? 0xffffff : COLORS.ship,
            fontWeight: isSelected ? "bold" : "normal",
          })
        }
      />
    </pixiContainer>
  );
}
