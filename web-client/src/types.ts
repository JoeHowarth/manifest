// Re-export types from the generated WASM module
export type {
  StateSnapshot,
  SettlementSnapshot,
  ShipSnapshot,
  OrgSnapshot,
  Route,
  ShipStatus,
  TransportMode,
  Good,
  GoodFamily,
  Population,
  FacilityType,
  NaturalResource,
  // New types for visualization
  FacilitySnapshot,
  MarketPriceSnapshot,
  LaborMarket,
  GoodMarket,
} from "./wasm/sim_core";
