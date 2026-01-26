import { Graphics } from "pixi.js";
import { useMemo, useCallback } from "react";
import type { FacilitySnapshot } from "../../types";
import { FACILITY_COLORS } from "./colors";

interface FacilityBadgesProps {
  facilities: FacilitySnapshot[];
  settlementRadius: number;
}

export function FacilityBadges({
  facilities,
  settlementRadius,
}: FacilityBadgesProps) {
  // Memoize facility types to avoid recreating on every render
  const facilityTypes = useMemo(
    () => [...new Set(facilities.map((f) => f.kind))].sort(),
    [facilities]
  );

  const drawBadges = useCallback(
    (g: Graphics) => {
      g.clear();

      const badgeRadius = 4;
      const arcRadius = settlementRadius + 12;
      const totalBadges = facilityTypes.length;
      if (totalBadges === 0) return;

      // Center the badges below the settlement
      const startAngle = Math.PI / 2 - ((totalBadges - 1) * 0.3) / 2;
      const angleStep = 0.3; // ~17 degrees between badges

      facilityTypes.forEach((type, i) => {
        const angle = startAngle + i * angleStep;
        const x = Math.cos(angle) * arcRadius;
        const y = Math.sin(angle) * arcRadius;

        const color = FACILITY_COLORS[type] || 0x888888;
        g.circle(x, y, badgeRadius);
        g.fill(color);
        g.stroke({ width: 1, color: 0xffffff, alpha: 0.3 });
      });
    },
    [facilityTypes, settlementRadius]
  );

  if (facilityTypes.length === 0) return null;

  return <pixiGraphics draw={drawBadges} />;
}
