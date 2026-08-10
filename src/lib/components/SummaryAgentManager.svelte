<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { Badge } from "@mutsuna/ui/badge";
  import { Button } from "@mutsuna/ui/button";
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import type { SummaryAgentInstallStatus, SummaryModelDefinition, SummaryProviderDefinition } from "../types/summary";

  type Props = {
    disabled: boolean;
    providers: readonly SummaryProviderDefinition[];
    providerModelDefaults: Readonly<Record<string, string>>;
    preview?: boolean;
    onChanged: () => Promise<void>;
    onLoadModels: (providerId: string) => Promise<void>;
    onProviderDefaultModelChange: (providerId: string, modelId: string) => void;
    onMessage: (message: string) => void;
    onError: (message: string) => void;
  };

  let {
    disabled,
    providers,
    providerModelDefaults,
    preview = false,
    onChanged,
    onLoadModels,
    onProviderDefaultModelChange,
    onMessage,
    onError
  }: Props = $props();
  let agents = $state.raw<SummaryAgentInstallStatus[]>([]);
  let loading = $state(true);
  let workingId = $state("");
  let loadingModelIds = $state<string[]>([]);
  const requestedModelIds = new Set<string>();

  function errorText(error: unknown): string {
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.message;
    return "AIの設定を変更できませんでした。";
  }

  async function refresh() {
    agents = await invoke<SummaryAgentInstallStatus[]>("list_summary_agent_install_status");
  }

  function fallbackModel(models: readonly SummaryModelDefinition[]): SummaryModelDefinition | undefined {
    return models.find((model) => model.isDefault) ?? models[0];
  }

  function selectedModel(provider: SummaryProviderDefinition): SummaryModelDefinition | undefined {
    const configured = providerModelDefaults[provider.id];
    return provider.models.find((model) => model.id === configured) ?? fallbackModel(provider.models);
  }

  function isModelLoading(providerId: string): boolean {
    return loadingModelIds.includes(providerId);
  }

  async function loadModels(providerId: string) {
    loadingModelIds = [...loadingModelIds, providerId];
    try {
      await onLoadModels(providerId);
    } finally {
      loadingModelIds = loadingModelIds.filter((id) => id !== providerId);
    }
  }

  $effect(() => {
    if (preview) {
      agents = providers.map((provider) => ({
        id: provider.id,
        label: provider.label,
        version: "preview",
        installed: true,
        external: false,
        installable: true,
        statusMessage: provider.statusMessage
      }));
      loading = false;
      return;
    }
    let cancelled = false;
    void refresh()
      .catch((error) => { if (!cancelled) onError(errorText(error)); })
      .finally(() => { if (!cancelled) loading = false; });
    return () => { cancelled = true; };
  });

  $effect(() => {
    for (const agent of agents) {
      if (!agent.installed || requestedModelIds.has(agent.id)) continue;
      const provider = providers.find((candidate) => candidate.id === agent.id && candidate.ready);
      if (!provider) continue;
      requestedModelIds.add(agent.id);
      void loadModels(agent.id);
    }
  });

  async function install(agent: SummaryAgentInstallStatus) {
    if (workingId) return;
    workingId = agent.id;
    try {
      await invoke("install_summary_agent", { providerId: agent.id });
      await refresh();
      await onChanged();
      onMessage(`${agent.label}を使えるようにしました。`);
    } catch (error) {
      onError(errorText(error));
    } finally {
      workingId = "";
    }
  }

  async function remove(agent: SummaryAgentInstallStatus) {
    if (workingId || agent.external || !window.confirm(`${agent.label}をこのアプリから削除しますか？`)) return;
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
    {@const provider = providers.find((candidate) => candidate.id === agent.id)}
    {@const currentModel = provider ? selectedModel(provider) : undefined}
    <div class="agent-row">
      <div class="agent-copy">
        <div class="agent-title">
          <strong>{agent.label}</strong>
          <Badge variant={agent.installed ? "default" : "secondary"}>
            {agent.installed ? "利用可能" : "未追加"}
          </Badge>
        </div>
        <small>{agent.installed ? "会議ノートの作成に使えます" : "追加すると会議ノートの作成に使えます"}{agent.external ? "・このアプリの外で管理されています" : ""}</small>
      </div>
      <div class="agent-actions">
        {#if agent.installed && provider}
          <Select
            type="single"
            value={currentModel?.id ?? ""}
            onValueChange={(modelId) => onProviderDefaultModelChange(provider.id, modelId)}
            disabled={disabled || isModelLoading(provider.id) || provider.models.length === 0}
          >
            <SelectTrigger aria-label={`${provider.label}で最初に使うAIモデル`} class="agent-model-select">
              <span class="select-value" title={currentModel?.label}>{isModelLoading(provider.id) ? "使えるAIを確認中…" : (currentModel?.label ?? "AIを選択")}</span>
            </SelectTrigger>
            <SelectContent>
              {#each provider.models as model (model.id)}
                <SelectItem value={model.id}>{model.label}</SelectItem>
              {/each}
            </SelectContent>
          </Select>
        {/if}
        {#if agent.installed && !agent.external}
          <Button variant="outline" type="button" onclick={() => remove(agent)} disabled={disabled || Boolean(workingId)} loading={workingId === agent.id}>削除</Button>
        {:else if !agent.installed}
          <Button type="button" onclick={() => install(agent)} disabled={disabled || loading || Boolean(workingId) || !agent.installable} loading={workingId === agent.id}>追加</Button>
        {/if}
      </div>
    </div>
  {/each}
  {#if !loading && agents.length === 0}<p>この端末で追加できるAIはありません。</p>{/if}
  <p class="note">AIを動かすために必要なものは、このアプリが自動で用意します。CodexやClaude Codeのログイン情報をこのアプリへコピーすることはありません。</p>
</div>

<style>
  .summary-agent-manager { display: grid; overflow: hidden; border: 1px solid var(--border); border-radius: 12px; background: color-mix(in oklch, var(--muted) 25%, var(--background)); }
  .agent-row { display: flex; align-items: center; justify-content: space-between; gap: 18px; padding: 15px 16px; border-bottom: 1px solid var(--border); }
  .agent-copy { display: grid; min-width: 0; gap: 5px; }
  .agent-title { display: flex; align-items: center; gap: 9px; }
  .agent-title strong { font-size: 0.84rem; }
  small, .note { color: var(--muted-foreground); font-size: 0.7rem; line-height: 1.5; }
  .agent-actions { display: grid; min-width: 0; flex: none; grid-template-columns: minmax(0, 230px) auto; align-items: center; gap: 8px; }
  :global(.agent-model-select) { width: 230px; min-width: 0; max-width: min(230px, 42vw); }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note { margin: 0; padding: 11px 16px; }
  @media (max-width: 680px) {
    .agent-row { align-items: stretch; flex-direction: column; }
    .agent-actions { width: 100%; }
    .agent-actions { grid-template-columns: minmax(0, 1fr) auto; }
    :global(.agent-model-select) { width: 100%; max-width: none; min-height: 44px; }
    .agent-actions :global([data-slot="button"]) { min-height: 44px; }
  }
</style>
