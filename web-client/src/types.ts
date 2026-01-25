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
  GoodState,
  SupplyLevel,
  PriceSnapshot,
  Population,
  FacilityType,
  NaturalResource,
} from "./wasm/sim_core";
