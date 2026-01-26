import { createRoot } from "react-dom/client";
import App from "./App";

// Note: StrictMode disabled because it double-invokes effects/callbacks
// which corrupts wasm_bindgen's mutable reference tracking
createRoot(document.getElementById("root")!).render(<App />);
