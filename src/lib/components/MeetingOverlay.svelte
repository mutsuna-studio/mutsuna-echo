<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { ThemeProvider, createTheme } from "@mutsuna/ui/theme";
  import type { MeetingDetection } from "../types/meeting";

  const echoTheme = createTheme("custom", "oklch(0.49 0.12 154)");
  let detection = $state<MeetingDetection | null>(null);
  let loading = $state(true);
  let recording = $state(false);
  let error = $state("");

  function errorText(value: unknown): string {
    if (typeof value === "string") return value;
    if (value instanceof Error) return value.message;
    return "録音を開始できませんでした。";
  }

  async function closeOverlay() {
    await getCurrentWebviewWindow().destroy();
  }

  async function dismiss() {
    try {
      await invoke("dismiss_meeting_overlay");
    } finally {
      await closeOverlay();
    }
  }

  async function startRecording() {
    if (recording) return;
    recording = true;
    error = "";
    try {
      await invoke("start_recording", {
        request: {
          microphone: true,
          systemAudio: true,
          microphoneDeviceId: null,
          systemDeviceId: null
        }
      });
      await invoke("dismiss_meeting_overlay");
      await closeOverlay();
    } catch (cause) {
      error = errorText(cause);
      recording = false;
    }
  }

  $effect(() => {
    void (async () => {
      try {
        detection = await invoke<MeetingDetection | null>("get_meeting_detection");
        if (!detection) await closeOverlay();
      } catch (cause) {
        error = errorText(cause);
      } finally {
        loading = false;
      }
    })();
  });
</script>

<ThemeProvider theme={echoTheme}>
  <main class="meeting-overlay" aria-busy={loading || recording}>
    {#if detection}
      <header>
        <div class="meeting-mark" aria-hidden="true">●</div>
        <div>
          <Badge variant="secondary">{detection.providerLabel}</Badge>
          <h1>会議を開始しましたか？</h1>
        </div>
      </header>
      <p class="meeting-title" title={detection.windowTitle}>{detection.windowTitle}</p>
      <p class="meeting-consent">録音はまだ開始されていません。参加者の同意を確認してから開始してください。</p>
      {#if error}<p class="meeting-error" role="alert">{error}</p>{/if}
      <div class="meeting-actions">
        <Button variant="ghost" type="button" onclick={dismiss} disabled={recording}>今は録音しない</Button>
        <Button type="button" onclick={startRecording} loading={recording} disabled={loading}>
          {recording ? "録音を準備中…" : "録音を開始"}
        </Button>
      </div>
    {:else if loading}
      <p class="meeting-loading">会議の状態を確認しています…</p>
    {/if}
  </main>
</ThemeProvider>
