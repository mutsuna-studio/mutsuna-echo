<script lang="ts">
  import { Select, SelectContent, SelectItem, SelectTrigger } from "@mutsuna/ui/select";
  import type { SummaryModelDefinition, SummaryProviderDefinition } from "../types/summary";

  type Props = {
    providers: readonly SummaryProviderDefinition[];
    defaultProviderId: string;
    defaultModelId: string;
    disabled: boolean;
    onLoadModels: (providerId: string) => Promise<void>;
    onDefaultProviderChange: (providerId: string) => void;
    onDefaultModelChange: (modelId: string) => void;
  };

  let {
    providers,
    defaultProviderId,
    defaultModelId,
    disabled,
    onLoadModels,
    onDefaultProviderChange,
    onDefaultModelChange
  }: Props = $props();

  let loadingProviderIds = $state<string[]>([]);
  const requestedProviderIds = new Set<string>();
  const availableProviders = $derived(providers.filter((provider) => provider.ready));
  const defaultProvider = $derived(availableProviders.find((provider) => provider.id === defaultProviderId) ?? availableProviders[0]);
  const defaultModel = $derived(defaultProvider ? selectedModel(defaultProvider, defaultModelId) : undefined);

  function fallbackModel(models: readonly SummaryModelDefinition[]): SummaryModelDefinition | undefined {
    return models.find((model) => model.isDefault) ?? models[0];
  }

  function selectedModel(provider: SummaryProviderDefinition, modelId: string): SummaryModelDefinition | undefined {
    return provider.models.find((model) => model.id === modelId) ?? fallbackModel(provider.models);
  }

  function isLoading(providerId: string): boolean {
    return loadingProviderIds.includes(providerId);
  }

  async function loadModels(providerId: string) {
    loadingProviderIds = [...loadingProviderIds, providerId];
    try {
      await onLoadModels(providerId);
    } finally {
      loadingProviderIds = loadingProviderIds.filter((id) => id !== providerId);
    }
  }

  $effect(() => {
    if (!defaultProvider || requestedProviderIds.has(defaultProvider.id)) return;
    requestedProviderIds.add(defaultProvider.id);
    void loadModels(defaultProvider.id);
  });
</script>

<div class="summary-defaults" aria-busy={loadingProviderIds.length > 0}>
  {#if availableProviders.length > 0 && defaultProvider}
    <section class="defaults-block" aria-labelledby="summary-global-defaults">
      <h3 id="summary-global-defaults" class="sr-only">最初に使うAI</h3>
      <label class="setting-row">
        <span class="setting-copy"><strong>AIサービス</strong><small>会議ノートの作成に使うサービス</small></span>
        <span class="setting-control">
          <Select type="single" value={defaultProviderId} onValueChange={onDefaultProviderChange} {disabled}>
            <SelectTrigger aria-label="最初に使うAIサービス" class="settings-select">
              <span class="select-value" title={defaultProvider.label}>{defaultProvider.label}</span>
            </SelectTrigger>
            <SelectContent>
              {#each availableProviders as provider (provider.id)}
                <SelectItem value={provider.id}>{provider.label}</SelectItem>
              {/each}
            </SelectContent>
          </Select>
        </span>
      </label>
      <label class="setting-row">
        <span class="setting-copy"><strong>モデル</strong><small>最初に選ぶモデル</small></span>
        <span class="setting-control">
          <Select type="single" value={defaultModel?.id ?? ""} onValueChange={onDefaultModelChange} disabled={disabled || isLoading(defaultProvider.id) || defaultProvider.models.length === 0}>
            <SelectTrigger aria-label="最初に使うAIモデル" class="settings-select">
              <span class="select-value" title={defaultModel?.label}>{isLoading(defaultProvider.id) ? "使えるAIを確認中…" : (defaultModel?.label ?? "AIを選択")}</span>
            </SelectTrigger>
            <SelectContent>
              {#each defaultProvider.models as model (model.id)}
                <SelectItem value={model.id}>{model.label}</SelectItem>
              {/each}
            </SelectContent>
          </Select>
        </span>
      </label>
    </section>

  {:else}
    <p class="empty">下の一覧からAIを追加すると、最初に使うAIを設定できます。</p>
  {/if}
</div>

<style>
  .summary-defaults, .defaults-block { display: grid; }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }
  .setting-row { display: flex; min-width: 0; min-height: 64px; align-items: center; justify-content: space-between; gap: 24px; padding: 10px 14px; border-bottom: 1px solid var(--border); }
  .setting-row:last-child { border-bottom: 0; }
  .setting-copy { display: grid; min-width: 0; gap: 3px; }
  .setting-copy strong { color: var(--foreground); font-size: 0.8rem; font-weight: 650; }
  .setting-copy small, .empty { color: var(--muted-foreground); font-size: 0.7rem; font-weight: 400; line-height: 1.45; }
  .setting-control { width: min(310px, 46%); min-width: 180px; }
  .setting-control :global([data-slot="select-trigger"]) { width: 100%; }
  :global(.settings-select) { width: 100%; min-width: 0; max-width: 100%; }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { margin: 0; padding: 14px; }

  @media (max-width: 600px) {
    .setting-row { align-items: stretch; flex-direction: column; gap: 9px; padding: 13px 14px; }
    .setting-control { width: 100%; min-width: 0; }
    :global(.settings-select) { min-height: 44px; }
  }
</style>
