import { Graphics, TextStyle, FederatedPointerEvent } from "pixi.js";
import { useCallback } from "react";
import type { SettlementSnapshot } from "../../types";
import { COLORS, getHealthColor, lightenColor } from "./colors";
import { FacilityBadges } from "./FacilityBadges";

interface SettlementNodeProps {
  settlement: SettlementSnapshot;
  isSelected: boolean;
  onSelect: () => void;
}

export function SettlementNode({
  settlement,
  isSelected,
  onSelect,
}: SettlementNodeProps) {
  const radius = Math.max(15, Math.min(30, settlement.population / 300));
  const healthColor = getHealthColor(settlement.provision_satisfaction);

  const drawNode = useCallback(
    (g: Graphics) => {
      g.clear();
      g.circle(0, 0, radius);
      g.fill(isSelected ? lightenColor(healthColor) : healthColor);
      if (isSelected) {
        g.stroke({ width: 3, color: 0xffffff, alpha: 0.9 });
      } else {
        g.stroke({ width: 2, color: 0xffffff, alpha: 0.3 });
      }
    },
    [radius, isSelected, healthColor]
  );

  const textStyle = new TextStyle({
    fontFamily: "system-ui, -apple-system, sans-serif",
    fontSize: 13,
    fill: COLORS.text,
    fontWeight: "600",
  });

  return (
    <pixiContainer
      x={settlement.position[0]}
      y={settlement.position[1]}
      eventMode="static"
      cursor="pointer"
      onPointerDown={(e: FederatedPointerEvent) => {
        e.stopPropagation();
        onSelect();
      }}
    >
      <pixiGraphics draw={drawNode} />
      <FacilityBadges
        facilities={settlement.facilities}
        settlementRadius={radius}
      />
      <pixiText
        text={settlement.name}
        x={0}
        y={radius + 8}
        anchor={0.5}
        style={textStyle}
      />
    </pixiContainer>
  );
}
