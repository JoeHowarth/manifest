import { useEffect, useState, useCallback } from "react";
import { GameView } from "./components/GameView";
import { Sidebar } from "./components/Sidebar";
import type { StateSnapshot } from "./types";

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
      }}
    >
      <GameView state={state} />
      <Sidebar state={state} onAdvanceTick={advanceTick} />
    </div>
  );
}

export default App;
