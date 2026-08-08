<script lang="ts">
  import { Button } from "@mutsuna/ui/button";
  import type { PendingAction } from "../types/pending-action";

  type Props = {
    action: PendingAction | null;
    message: string;
    busy: boolean;
    onRetry: () => void;
    onDiscard: () => void;
  };

  let { action, message, busy, onRetry, onDiscard }: Props = $props();
</script>

<section class="pending-action-notice" aria-live="polite">
  <div>
    <strong>録音の引き渡しを完了できませんでした</strong>
    <p>{message}</p>
    {#if action}
      <small>録音は保存済みです。再試行するか、履歴から選択できます。</small>
    {:else}
      <small>保存済みの引き渡し情報を解除しても、録音ファイルは削除されません。</small>
    {/if}
  </div>
  <div class="pending-action-buttons">
    <Button size="sm" type="button" onclick={onRetry} loading={busy} disabled={busy}>再試行</Button>
    <Button variant="outline" size="sm" type="button" onclick={onDiscard} disabled={busy}>解除</Button>
  </div>
</section>

<style>
  .pending-action-notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 1rem;
    padding: 0.9rem 1rem;
    border: 1px solid color-mix(in oklch, var(--destructive) 38%, transparent);
    border-radius: 0.85rem;
    background: color-mix(in oklch, var(--destructive) 7%, var(--background));
  }

  strong,
  p,
  small {
    display: block;
  }

  p {
    margin: 0.25rem 0;
  }

  small {
    color: var(--muted-foreground);
  }

  .pending-action-buttons {
    display: flex;
    flex: 0 0 auto;
    gap: 0.5rem;
  }

  @media (max-width: 620px) {
    .pending-action-notice {
      align-items: stretch;
      flex-direction: column;
    }

    .pending-action-buttons {
      justify-content: flex-end;
    }
  }
</style>
