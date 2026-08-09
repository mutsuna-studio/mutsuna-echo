<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import type { SummaryAgentInstallStatus } from "../types/summary";

  type Props = {
    disabled: boolean;
    onChanged: () => Promise<void>;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  };

  let { disabled, onChanged, onMessage, onError }: Props = $props();
  let agents = $state.raw<SummaryAgentInstallStatus[]>([]);
  let loading = $state(true);
  let workingId = $state("");

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "要約エージェントの操作に失敗しました。";
  }

  async function refresh() {
    agents = await invoke<SummaryAgentInstallStatus[]>("list_summary_agent_install_status");
  }

  $effect(() => {
    let cancelled = false;
    void refresh()
      .catch((error) => { if (!cancelled) onError(errorText(error)); })
      .finally(() => { if (!cancelled) loading = false; });
    return () => { cancelled = true; };
  });

  async function install(agent: SummaryAgentInstallStatus) {
    if (workingId) return;
    workingId = agent.id;
    try {
      await invoke("install_summary_agent", { providerId: agent.id });
      await refresh();
      await onChanged();
      onMessage(`${agent.label}をEcho専用領域へインストールしました。`);
    } catch (error) {
      onError(errorText(error));
    } finally {
      workingId = "";
    }
  }

  async function remove(agent: SummaryAgentInstallStatus) {
    if (workingId || agent.external || !window.confirm(`${agent.label}のEcho管理版を削除しますか？`)) return;
    workingId = agent.id;
    try {
      await invoke("delete_summary_agent", { providerId: agent.id });
      await refresh();
      await onChanged();
      onMessage(`${agent.label}を削除しました。`);
    } catch (error) {
      onError(errorText(error));
    } finally {
      workingId = "";
    }
  }
</script>

<div class="summary-agent-manager" aria-busy={loading || Boolean(workingId)}>
  {#each agents as agent (agent.id)}
    <div class="agent-row">
      <div class="agent-copy">
        <div class="agent-title">
          <strong>{agent.label}</strong>
          <Badge variant={agent.installed ? "default" : "secondary"}>
            {agent.installed ? "利用可能" : "未インストール"}
          </Badge>
        </div>
        <small>{agent.statusMessage}{agent.external ? "・Echoの削除対象外" : `・固定版 ${agent.version}`}</small>
      </div>
      {#if agent.installed && !agent.external}
        <Button variant="outline" type="button" onclick={() => remove(agent)} disabled={disabled || Boolean(workingId)} loading={workingId === agent.id}>削除</Button>
      {:else if !agent.installed}
        <Button type="button" onclick={() => install(agent)} disabled={disabled || loading || Boolean(workingId) || !agent.installable} loading={workingId === agent.id}>インストール</Button>
      {/if}
    </div>
  {/each}
  {#if !loading && agents.length === 0}<p>このOSで利用できる要約エージェントはありません。</p>{/if}
  <p class="note">Node.jsがない端末では、公式Node.jsランタイムもEcho専用領域へ自動で追加します。各エージェントのログイン情報はEchoへコピーせず、エージェント自身が管理します。</p>
</div>

<style>
  .summary-agent-manager { display: grid; overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: color-mix(in oklch, var(--muted) 25%, var(--background)); }
  .agent-row { display: flex; align-items: center; justify-content: space-between; gap: 18px; padding: 15px 16px; border-bottom: 1px solid var(--border); }
  .agent-copy { display: grid; min-width: 0; gap: 5px; }
  .agent-title { display: flex; align-items: center; gap: 9px; }
  .agent-title strong { font-size: 0.84rem; }
  small, .note { color: var(--muted-foreground); font-size: 0.7rem; line-height: 1.5; }
  .note { margin: 0; padding: 11px 16px; }
  @media (max-width: 560px) { .agent-row { align-items: stretch; flex-direction: column; } }
</style>
