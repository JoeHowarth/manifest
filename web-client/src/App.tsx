import { useEffect, useState, useCallback } from "react";
import { GameView } from "./components/GameView";
import { Sidebar } from "./components/Sidebar";
import { SelectionPanel } from "./components/panels/SelectionPanel";
import type { StateSnapshot } from "./types";

export type SelectionItem =
  | { type: "settlement"; id: number }
  | { type: "ship"; id: number };

// Array of selections, ordered by when they were opened (newest last)
export type Selection = SelectionItem[];

// Dynamic import for WASM module
const initWasm = async () => {
  const wasm = await import("./wasm/sim_core");
  await wasm.default();
  return wasm;
};

type WasmModule = Awaited<ReturnType<typeof initWasm>>;
type SimulationInstance = InstanceType<WasmModule["Simulation"]>;

function App() {
  const [simInstance, setSimInstance] = useState<SimulationInstance | null>(
    null
  );
  const [state, setState] = useState<StateSnapshot | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selection, setSelection] = useState<Selection>([]);

  // Toggle a selection - add if not present, remove if present
  const toggleSelection = useCallback((item: SelectionItem | null) => {
    if (item === null) return;
    setSelection((prev) => {
      const exists = prev.some((s) => s.type === item.type && s.id === item.id);
      if (exists) {
        return prev.filter((s) => !(s.type === item.type && s.id === item.id));
      }
      return [...prev, item];
    });
  }, []);

  // Remove a specific selection by index or item
  const removeSelection = useCallback((item: SelectionItem) => {
    setSelection((prev) =>
      prev.filter((s) => !(s.type === item.type && s.id === item.id))
    );
  }, []);

  // Close the most recently opened selection (for ESC key)
  const closeLastSelection = useCallback(() => {
    setSelection((prev) => prev.slice(0, -1));
  }, []);

  // ESC key handler
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        closeLastSelection();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [closeLastSelection]);

  useEffect(() => {
    initWasm()
      .then((wasm) => {
        const sim = wasm.Simulation.with_test_scenario();
        setSimInstance(sim);
        setState(sim.get_state_snapshot());
        setIsLoading(false);
      })
      .catch((err) => {
        console.error("Failed to load WASM:", err);
        setError(err.message);
        setIsLoading(false);
      });
  }, []);

  const advanceTick = useCallback(() => {
    if (simInstance) {
      simInstance.advance_tick();
      setState(simInstance.get_state_snapshot());
    }
  }, [simInstance]);

  if (isLoading) {
    return (
      <div className="loading">
        <p>Loading simulation...</p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="error">
        <h2>Failed to load simulation</h2>
        <p>{error}</p>
        <p>Make sure to run: npm run wasm:build</p>
      </div>
    );
  }

  if (!state) {
    return null;
  }

  return (
    <div
      style={{
        display: "flex",
        width: "100%",
        height: "100%",
        position: "relative",
      }}
    >
      <GameView state={state} selection={selection} onSelect={toggleSelection} />
      <SelectionPanel
        state={state}
        selection={selection}
        onToggle={toggleSelection}
        onRemove={removeSelection}
        tick={state.tick}
      />
      <Sidebar
        state={state}
        selection={selection}
        onSelect={toggleSelection}
        onAdvanceTick={advanceTick}
      />
    </div>
  );
}

export default App;
