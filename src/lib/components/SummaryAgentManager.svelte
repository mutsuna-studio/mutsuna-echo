<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import CircleCheck from "@lucide/svelte/icons/circle-check";
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
        installing: false,
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
    if (preview || !agents.some((agent) => agent.installing)) return;
    const timer = window.setInterval(() => {
      void refresh()
        .then(() => {
          if (!agents.some((agent) => agent.installing)) return onChanged();
        })
        .catch((error) => onError(errorText(error)));
    }, 1_000);
    return () => window.clearInterval(timer);
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
          <span class:ready={agent.installed} class="agent-status">{#if agent.installed}<CircleCheck aria-hidden="true" />{/if}{agent.installed ? "利用可能" : agent.installing ? "追加中" : "未追加"}</span>
        </div>
        {#if agent.external}<small>このアプリの外で管理されています</small>{/if}
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
          <Button type="button" onclick={() => install(agent)} disabled={disabled || loading || Boolean(workingId) || !agent.installable} loading={workingId === agent.id || agent.installing}>追加</Button>
        {/if}
      </div>
    </div>
  {/each}
  {#if !loading && agents.length === 0}<p>この端末で追加できるAIはありません。</p>{/if}
  <p class="note">CodexやClaude Codeのログイン情報は、このアプリにコピーされません。</p>
</div>

<style>
  .summary-agent-manager { display: grid; overflow: hidden; }
  .agent-row { display: flex; min-height: 64px; align-items: center; justify-content: space-between; gap: 24px; padding: 10px 14px; border-bottom: 1px solid var(--border); }
  .agent-copy { display: grid; min-width: 0; gap: 5px; }
  .agent-title { display: flex; align-items: center; gap: 9px; }
  .agent-title strong { font-size: 0.8rem; }
  .agent-status { display: inline-flex; align-items: center; gap: 4px; color: var(--muted-foreground); font-size: 0.66rem; font-weight: 600; }
  .agent-status.ready { color: var(--primary); }
  .agent-status :global(svg) { width: 13px; height: 13px; }
  small, .note { color: var(--muted-foreground); font-size: 0.7rem; line-height: 1.5; }
  .agent-actions { display: grid; min-width: 0; flex: none; grid-template-columns: minmax(0, 230px) auto; align-items: center; gap: 8px; }
  :global(.agent-model-select) { width: 230px; min-width: 0; max-width: min(230px, 42vw); }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note { margin: 0; padding: 10px 14px; background: color-mix(in oklch, var(--muted) 45%, var(--background)); }
  @media (max-width: 680px) {
    .agent-row { align-items: stretch; flex-direction: column; }
    .agent-actions { width: 100%; }
    .agent-actions { grid-template-columns: minmax(0, 1fr) auto; }
    :global(.agent-model-select) { width: 100%; max-width: none; min-height: 44px; }
    .agent-actions :global([data-slot="button"]) { min-height: 44px; }
  }
</style>
