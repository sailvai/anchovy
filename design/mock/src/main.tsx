import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { type ScreenId, type Theme, screens, windowSize } from "./catalog";
import { Gallery } from "./Gallery";
import { Screen } from "./screens/Screen";
import "./tokens.css";

// ?screen=<id>&theme=light|dark shows one screen at the app's window size.
// No parameters shows the list of screens.
const params = new URLSearchParams(location.search);
const id = params.get("screen") as ScreenId | null;
const theme: Theme = params.get("theme") === "dark" ? "dark" : "light";
document.documentElement.dataset.theme = theme;

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    {id && screens.some((screen) => screen.id === id) ? (
      <div className="overflow-hidden" style={windowSize}>
        <Screen id={id} />
      </div>
    ) : (
      <Gallery />
    )}
  </StrictMode>,
);
