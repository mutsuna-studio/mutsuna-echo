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
      <div class="block-heading">
        <h3 id="summary-global-defaults">最初に使うAI</h3>
        <p>会議を開いたときに、はじめから選ばれているAIを設定します。あとから会議ごとに変更できます。</p>
      </div>
      <div class="defaults-grid">
        <label>
          <span>AIサービス</span>
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
        </label>
        <label>
          <span>使うAI</span>
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
        </label>
      </div>
    </section>

  {:else}
    <p class="empty">下の一覧からAIを追加すると、最初に使うAIを設定できます。</p>
  {/if}
</div>

<style>
  .summary-defaults { display: grid; gap: 20px; margin: 20px 0; }
  .defaults-block { display: grid; gap: 13px; }
  .block-heading { display: grid; gap: 4px; }
  .block-heading h3 { margin: 0; font-size: 0.84rem; }
  .block-heading p, .empty { margin: 0; color: var(--muted-foreground); font-size: 0.72rem; line-height: 1.55; }
  .defaults-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px; }
  label { display: grid; min-width: 0; gap: 6px; color: var(--muted-foreground); font-size: 0.72rem; font-weight: 650; }
  label :global([data-slot="select-trigger"]) { width: 100%; }
  :global(.settings-select) { width: 100%; min-width: 0; max-width: 100%; }
  .select-value { display: block; min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { padding: 14px 0 2px; }

  @media (max-width: 600px) {
    .defaults-grid { grid-template-columns: minmax(0, 1fr); }
    :global(.settings-select) { min-height: 44px; }
  }
</style>
