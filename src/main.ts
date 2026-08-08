import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App.svelte";
import MeetingOverlay from "./lib/components/MeetingOverlay.svelte";
import "./app.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}

const component = getCurrentWebviewWindow().label === "meeting-overlay" ? MeetingOverlay : App;
mount(component, { target });
