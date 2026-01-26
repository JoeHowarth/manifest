export const COLORS = {
  background: 0x1a1a2e,
  settlement: 0x4a90a4,
  settlementHover: 0x6ab0c4,
  routeSea: 0x3a7ca5,
  routeRiver: 0x5dade2,
  routeLand: 0x8b7355,
  text: 0xffffff,
  ship: 0xf4d03f,
};

// Settlement health colors based on provision satisfaction
export const HEALTH_COLORS = {
  thriving: 0x4caf50, // green - satisfaction > 0.9
  stable: 0xffeb3b, // yellow - 0.5 < satisfaction <= 0.9
  struggling: 0xff9800, // orange - 0.3 < satisfaction <= 0.5
  crisis: 0xf44336, // red - satisfaction <= 0.3
};

// Facility type colors for badges
export const FACILITY_COLORS: Record<string, number> = {
  Farm: 0xffd700, // gold (grain)
  Fishery: 0x00ced1, // dark cyan
  LumberCamp: 0x8b4513, // saddle brown
  Mine: 0x708090, // slate gray
  Pasture: 0xf5deb3, // wheat
  Mill: 0xdeb887, // burlywood
  Foundry: 0xb22222, // firebrick
  Weaver: 0x9370db, // medium purple
  Bakery: 0xd2691e, // chocolate
  Toolsmith: 0x4682b4, // steel blue
  Shipyard: 0x000080, // navy
};

// Utility to get health color from satisfaction
export function getHealthColor(satisfaction: number): number {
  if (satisfaction > 0.9) return HEALTH_COLORS.thriving;
  if (satisfaction > 0.5) return HEALTH_COLORS.stable;
  if (satisfaction > 0.3) return HEALTH_COLORS.struggling;
  return HEALTH_COLORS.crisis;
}

// Utility to lighten a color for hover/selected state
export function lightenColor(color: number, amount = 0.2): number {
  const r = ((color >> 16) & 0xff) / 255;
  const g = ((color >> 8) & 0xff) / 255;
  const b = (color & 0xff) / 255;

  const newR = Math.min(1, r + amount);
  const newG = Math.min(1, g + amount);
  const newB = Math.min(1, b + amount);

  return (
    (Math.round(newR * 255) << 16) |
    (Math.round(newG * 255) << 8) |
    Math.round(newB * 255)
  );
}
