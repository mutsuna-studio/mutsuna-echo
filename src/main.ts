import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./app.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("App mount target was not found");
}
const mountTarget = target;

const isMeetingOverlay = getCurrentWebviewWindow().label === "meeting-overlay";
const isWindowsOverlay = isMeetingOverlay && navigator.userAgent.includes("Windows");
document.body.classList.toggle("overlay-window", isMeetingOverlay);
document.documentElement.classList.toggle("transparent-overlay", isWindowsOverlay);
document.body.classList.toggle("transparent-overlay", isWindowsOverlay);

async function bootstrap() {
  const component = isMeetingOverlay
    ? (await import("./lib/components/MeetingOverlay.svelte")).default
    : (await import("./App.svelte")).default;
  mount(component, { target: mountTarget });
}

void bootstrap().catch((error: unknown) => {
  console.error("Could not initialize the application window", error);
  mountTarget.textContent = "画面を読み込めませんでした。アプリを再起動してください。";
});
