import { Graphics } from "pixi.js";
import { useCallback } from "react";
import type { SettlementSnapshot, Route } from "../../types";
import { COLORS } from "./colors";

interface RoutesLayerProps {
  routes: Route[];
  settlements: SettlementSnapshot[];
}

export function RoutesLayer({ routes, settlements }: RoutesLayerProps) {
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
