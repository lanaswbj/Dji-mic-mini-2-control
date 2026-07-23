import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.svelte";
import PieMenu from "./PieMenu.svelte";
import "./app.css";

// The pie-menu overlay is a separate always-on-top window sharing this same
// bundle; it mounts its own tiny root component instead of the main app UI.
const isPieMenu = getCurrentWindow().label === "pie-menu";
const app = mount(isPieMenu ? PieMenu : App, { target: document.getElementById("app") });

export default app;
