<script lang="ts">
  import BookOpen from "@lucide/svelte/icons/book-open";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Search from "@lucide/svelte/icons/search";

  type LicenseDocument = {
    id: string;
    name: string;
    text: string;
  };

  type LicenseComponent = {
    ecosystem: string;
    name: string;
    version: string;
    license: string;
    sourceUrl: string;
    documentIds: string[];
  };

  type LicenseCatalog = {
    schemaVersion: number;
    project: {
      name: string;
      license: string;
      sourceUrl: string;
      licenseDocumentId: string;
    };
    components: LicenseComponent[];
    documents: LicenseDocument[];
  };

  let expanded = $state(false);
  let loading = $state(false);
  let error = $state("");
  let catalog = $state.raw<LicenseCatalog | null>(null);
  let query = $state("");
  let ecosystem = $state("all");
  let visibleLimit = $state(100);
  let selectedId = $state("project");

  const ecosystems = $derived.by(() =>
    catalog ? [...new Set(catalog.components.map((component) => component.ecosystem))].sort() : []
  );
  const filteredComponents = $derived.by(() => {
    if (!catalog) return [];
    const normalizedQuery = query.trim().toLocaleLowerCase("ja");
    return catalog.components.filter((component) => {
      if (ecosystem !== "all" && component.ecosystem !== ecosystem) return false;
      if (!normalizedQuery) return true;
      return [component.name, component.version, component.license, component.ecosystem]
        .some((value) => value.toLocaleLowerCase("ja").includes(normalizedQuery));
    });
  });
  const visibleComponents = $derived(filteredComponents.slice(0, visibleLimit));
  const selectedComponent = $derived.by(() => {
    if (!catalog || selectedId === "project") return null;
    return catalog.components.find((component) => componentId(component) === selectedId) ?? null;
  });
  const selectedDocuments = $derived.by(() => {
    if (!catalog) return [];
    const ids = selectedId === "project"
      ? [catalog.project.licenseDocumentId]
      : selectedComponent?.documentIds ?? [];
    const byId = new Map(catalog.documents.map((document) => [document.id, document]));
    return ids.flatMap((id) => byId.get(id) ?? []);
  });

  function componentId(component: LicenseComponent): string {
    return `${component.ecosystem}\0${component.name}\0${component.version}`;
  }

  async function toggleExpanded() {
    expanded = !expanded;
    if (!expanded || catalog || loading) return;
    loading = true;
    error = "";
    try {
      const response = await fetch(new URL("third-party-licenses.json", document.baseURI));
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const value = await response.json() as LicenseCatalog;
      if (value.schemaVersion !== 1 || !Array.isArray(value.components) || !Array.isArray(value.documents)) {
        throw new Error("unsupported catalog");
      }
      catalog = value;
    } catch {
      error = "ライセンス情報を読み込めませんでした。アプリを再起動してもう一度お試しください。";
    } finally {
      loading = false;
    }
  }

  function updateQuery(event: Event) {
    query = (event.currentTarget as HTMLInputElement).value;
    visibleLimit = 100;
  }

  function updateEcosystem(event: Event) {
    ecosystem = (event.currentTarget as HTMLSelectElement).value;
    visibleLimit = 100;
  }
</script>

<section class="license-settings" aria-labelledby="license-settings-title">
  <div class="license-summary">
    <div class="license-heading">
      <span class="license-icon" aria-hidden="true"><BookOpen /></span>
      <div>
        <div class="license-title">
          <h2 id="license-settings-title">オープンソースライセンス</h2>
          {#if catalog}<span>{catalog.components.length}件</span>{/if}
        </div>
        <p>このアプリとローカルAIで使用しているソフトウェア、フォント、モデルのライセンスを確認できます。</p>
      </div>
    </div>
    <button class="toggle-button" type="button" aria-expanded={expanded} aria-controls="license-details" onclick={toggleExpanded}>
      {expanded ? "閉じる" : "表示する"}
      <ChevronDown class={expanded ? "expanded" : undefined} aria-hidden="true" />
    </button>
  </div>

  {#if expanded}
    <div id="license-details" class="license-details">
      {#if loading}
        <p class="loading" aria-live="polite">ライセンス情報を読み込んでいます…</p>
      {:else if error}
        <p class="error" role="alert">{error}</p>
      {:else if catalog}
        <div class="license-controls">
          <label class="search-field">
            <Search aria-hidden="true" />
            <span class="sr-only">ライセンスを検索</span>
            <input type="search" value={query} placeholder="名前またはライセンスで検索" oninput={updateQuery} />
          </label>
          <label class="ecosystem-field">
            <span class="sr-only">種類で絞り込む</span>
            <select value={ecosystem} onchange={updateEcosystem}>
              <option value="all">すべての種類</option>
              {#each ecosystems as value}
                <option value={value}>{value}</option>
              {/each}
            </select>
          </label>
        </div>

        <div class="license-browser">
          <div class="component-list" aria-label="ライセンス対象一覧">
            {#if ecosystem === "all" && (!query || catalog.project.name.toLocaleLowerCase("ja").includes(query.toLocaleLowerCase("ja")))}
              <button class:selected={selectedId === "project"} type="button" onclick={() => selectedId = "project"}>
                <strong>{catalog.project.name}</strong>
                <span>{catalog.project.license} · このアプリ</span>
              </button>
            {/if}
            {#each visibleComponents as component (componentId(component))}
              <button class:selected={selectedId === componentId(component)} type="button" onclick={() => selectedId = componentId(component)}>
                <strong>{component.name}</strong>
                <span>{component.version} · {component.license} · {component.ecosystem}</span>
              </button>
            {/each}
            {#if filteredComponents.length === 0}
              <p class="empty">該当するライセンスはありません。</p>
            {:else if visibleComponents.length < filteredComponents.length}
              <button class="show-more" type="button" onclick={() => visibleLimit += 100}>
                さらに表示（残り{filteredComponents.length - visibleComponents.length}件）
              </button>
            {/if}
          </div>

          <article class="license-document" aria-live="polite">
            <header>
              <div>
                <h3>{selectedComponent?.name ?? catalog.project.name}</h3>
                <p>{selectedComponent?.license ?? catalog.project.license}</p>
              </div>
              <span>{selectedComponent?.version ?? "現在のバージョン"}</span>
            </header>
            <dl>
              <div><dt>種類</dt><dd>{selectedComponent?.ecosystem ?? "Application"}</dd></div>
              <div><dt>ソース</dt><dd><code>{selectedComponent?.sourceUrl ?? catalog.project.sourceUrl}</code></dd></div>
            </dl>
            {#if selectedDocuments.length > 0}
              {#each selectedDocuments as document, index (document.id)}
                <details open={selectedDocuments.length === 1 || index === 0}>
                  <summary>{document.name}</summary>
                  <pre>{document.text}</pre>
                </details>
              {/each}
            {:else}
              <p class="empty">ライセンス条件は上記の配布元で確認できます。</p>
            {/if}
          </article>
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .license-settings { display: grid; border-top: 1px solid var(--border); }
  .license-summary { display: flex; min-height: 76px; align-items: center; justify-content: space-between; gap: 24px; padding: 14px; }
  .license-heading { display: flex; min-width: 0; align-items: flex-start; gap: 12px; }
  .license-icon { display: grid; width: 34px; height: 34px; flex: none; place-items: center; border-radius: 9px; color: var(--primary); background: color-mix(in oklch, var(--primary) 10%, var(--background)); }
  .license-icon :global(svg) { width: 18px; height: 18px; stroke-width: 1.8; }
  .license-title { display: flex; flex-wrap: wrap; align-items: baseline; gap: 8px; }
  .license-title h2 { margin: 0; font-size: 0.9rem; font-weight: 680; }
  .license-title span { color: var(--muted-foreground); font-size: 0.7rem; }
  .license-heading p { max-width: 620px; margin: 4px 0 0; color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.55; }
  .toggle-button { display: inline-flex; min-height: 34px; flex: none; align-items: center; gap: 6px; padding: 0 10px; border: 1px solid var(--border); border-radius: 8px; color: var(--foreground); background: var(--background); cursor: pointer; font: inherit; font-size: 0.74rem; font-weight: 650; }
  .toggle-button:hover { background: var(--muted); }
  .toggle-button:focus-visible, .component-list button:focus-visible, select:focus-visible, input:focus-visible, summary:focus-visible { outline: 2px solid var(--ring); outline-offset: 2px; }
  .toggle-button :global(svg) { width: 15px; height: 15px; transition: transform 160ms ease; }
  .toggle-button :global(svg.expanded) { transform: rotate(180deg); }
  .license-details { display: grid; gap: 12px; padding: 16px 14px 14px; border-top: 1px solid var(--border); background: color-mix(in oklch, var(--muted) 28%, var(--background)); }
  .license-controls { display: grid; grid-template-columns: minmax(220px, 1fr) minmax(150px, 220px); gap: 8px; }
  .search-field { position: relative; display: flex; align-items: center; }
  .search-field :global(svg) { position: absolute; left: 10px; width: 15px; height: 15px; color: var(--muted-foreground); pointer-events: none; }
  input, select { width: 100%; height: 36px; border: 1px solid var(--border); border-radius: 8px; color: var(--foreground); background: var(--background); font: inherit; font-size: 0.76rem; }
  input { padding: 0 10px 0 33px; }
  select { padding: 0 28px 0 10px; }
  .license-browser { display: grid; min-height: 430px; grid-template-columns: minmax(220px, 0.8fr) minmax(0, 1.4fr); overflow: hidden; border: 1px solid var(--border); border-radius: 10px; background: var(--background); }
  .component-list { max-height: 560px; overflow-y: auto; border-right: 1px solid var(--border); }
  .component-list button { display: grid; width: 100%; gap: 3px; padding: 10px 12px; border: 0; border-bottom: 1px solid color-mix(in oklch, var(--border) 72%, transparent); color: var(--foreground); background: transparent; cursor: pointer; font: inherit; text-align: left; }
  .component-list button:hover { background: var(--muted); }
  .component-list button.selected { background: color-mix(in oklch, var(--primary) 10%, var(--background)); }
  .component-list strong { overflow: hidden; font-size: 0.77rem; font-weight: 670; text-overflow: ellipsis; white-space: nowrap; }
  .component-list span { overflow: hidden; color: var(--muted-foreground); font-size: 0.66rem; text-overflow: ellipsis; white-space: nowrap; }
  .component-list .show-more { color: var(--primary); font-size: 0.72rem; font-weight: 650; text-align: center; }
  .license-document { min-width: 0; max-height: 560px; overflow: auto; padding: 16px; }
  .license-document > header { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; }
  .license-document h3 { margin: 0; font-size: 0.98rem; font-weight: 680; overflow-wrap: anywhere; }
  .license-document header p, .license-document header > span { margin: 4px 0 0; color: var(--muted-foreground); font-size: 0.7rem; }
  dl { display: grid; gap: 5px; margin: 14px 0; padding: 10px 0; border-top: 1px solid var(--border); border-bottom: 1px solid var(--border); }
  dl div { display: grid; grid-template-columns: 52px minmax(0, 1fr); gap: 8px; font-size: 0.68rem; }
  dt { color: var(--muted-foreground); }
  dd { min-width: 0; margin: 0; }
  code { font: inherit; overflow-wrap: anywhere; }
  details { border-bottom: 1px solid var(--border); }
  summary { padding: 9px 2px; cursor: pointer; font-size: 0.72rem; font-weight: 650; }
  pre { margin: 0 0 14px; color: var(--muted-foreground); font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.66rem; line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .loading, .error, .empty { margin: 0; padding: 18px 12px; color: var(--muted-foreground); font-size: 0.74rem; line-height: 1.55; }
  .error { color: var(--destructive); }
  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }

  @media (max-width: 760px) {
    .license-summary { align-items: flex-start; }
    .license-controls { grid-template-columns: 1fr; }
    .license-browser { min-height: 0; grid-template-columns: 1fr; }
    .component-list { max-height: 300px; border-right: 0; border-bottom: 1px solid var(--border); }
    .license-document { max-height: 520px; }
  }

  @media (max-width: 520px) {
    .license-summary { display: grid; gap: 12px; padding-right: 2px; padding-left: 2px; }
    .toggle-button { width: 100%; justify-content: center; }
    .license-details { margin: 0 -2px; padding-right: 2px; padding-left: 2px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle-button :global(svg) { transition: none; }
  }
</style>
