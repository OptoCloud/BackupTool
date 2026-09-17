<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import {
    formatClock,
    formatDuration,
    humanBytes,
    phaseTitles,
    task,
    taskSteps,
    type Phase,
    type TaskKind,
  } from "$lib/task.svelte";

  let {
    kind,
    skipPhases = [],
    idleText,
  }: { kind: TaskKind; skipPhases?: Phase[]; idleText: string } = $props();

  const active = $derived(task.kind === kind);
  const steps = $derived(taskSteps[kind].filter((s) => !s.phases.every((p) => skipPhases.includes(p))));
  const p = $derived(active ? task.progress : null);
  const stepIndex = $derived(p ? steps.findIndex((s) => s.phases.includes(p.phase)) : -1);
  const percent = $derived(
    task.status === "done" ? 100 : p && p.total > 0 ? Math.min(100, (p.done / p.total) * 100) : null,
  );
  const itemPercent = $derived(p && p.itemTotal > 0 ? Math.min(100, (p.itemDone / p.itemTotal) * 100) : null);

  const heading = $derived.by(() => {
    switch (task.status) {
      case "done":
        return "Finished";
      case "cancelled":
        return "Cancelled";
      case "failed":
        return "Failed";
      default:
        return p ? phaseTitles[p.phase] : "Starting…";
    }
  });

  const detail = $derived.by(() => {
    if (!p || task.status !== "running") return "";
    if (p.phase === "scanning") return `${p.done.toLocaleString()} items found`;
    if (p.phase === "opening") return "Reading snapshot metadata";
    if (p.total === 0) return "Nothing to process";
    const parts = [`${humanBytes(p.done)} of ${humanBytes(p.total)}`];
    if (task.rate) parts.push(`${humanBytes(task.rate)}/s`);
    if (task.eta !== null) parts.push(`${formatDuration(task.eta * 1000)} left`);
    return parts.join(" · ");
  });

  let logEl = $state<HTMLDivElement>();
  let stickToBottom = true;
  let copied = $state(false);

  $effect(() => {
    void task.log.length;
    if (logEl && stickToBottom) logEl.scrollTop = logEl.scrollHeight;
  });

  function onLogScroll() {
    if (!logEl) return;
    stickToBottom = logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 24;
  }

  async function copyLog() {
    try {
      await navigator.clipboard.writeText(task.logText());
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // Clipboard unavailable; nothing else to do.
    }
  }
</script>

{#if !active}
  <section class="card panel-idle">
    <Icon name="log" size={28} />
    <p>{idleText}</p>
  </section>
{:else}
  <section class="card progress-card" class:failed={task.status === "failed" || task.status === "cancelled"}>
    <div class="progress-head">
      <div class="progress-title">
        {#if task.status === "running"}
          <span class="spinner" aria-hidden="true"></span>
        {:else if task.status === "done"}
          <span class="status-icon ok"><Icon name="check" size={16} /></span>
        {:else}
          <span class="status-icon bad"><Icon name="alert" size={16} /></span>
        {/if}
        <strong>{heading}</strong>
      </div>
      <span class="muted tabular">{formatDuration(task.elapsed)}</span>
      {#if task.status === "running"}
        <button class="btn btn-sm btn-danger" onclick={() => task.cancel()} disabled={task.cancelling}>
          <Icon name="stop" size={14} />
          {task.cancelling ? "Cancelling…" : "Cancel"}
        </button>
      {/if}
    </div>

    <div class="bar" class:indeterminate={percent === null && task.status === "running"}>
      <div class="bar-fill" style:width={percent === null ? undefined : `${percent}%`}></div>
    </div>
    {#if task.status === "running"}
      <div class="progress-detail tabular">
        <span>{detail}</span>
        {#if percent !== null}<span>{percent.toFixed(1)}%</span>{/if}
      </div>
    {/if}

    {#if p?.item && task.status === "running"}
      <div class="item">
        <div class="item-row">
          <Icon name="file" size={14} />
          <span class="item-path" title={p.item}><bdi>{p.item}</bdi></span>
          {#if p.itemTotal > 0}
            <span class="muted tabular">{humanBytes(p.itemDone)} / {humanBytes(p.itemTotal)}</span>
          {/if}
        </div>
        {#if itemPercent !== null}
          <div class="bar bar-thin"><div class="bar-fill" style:width={`${itemPercent}%`}></div></div>
        {/if}
      </div>
    {/if}

    <ol class="steps">
      {#each steps as step, i (step.label)}
        {@const done = task.status === "done" || i < stepIndex}
        <li class:done class:active={!done && i === stepIndex && task.status === "running"}>
          <span class="step-dot">
            {#if done}<Icon name="check" size={12} />{:else}{i + 1}{/if}
          </span>
          {step.label}
        </li>
      {/each}
    </ol>
  </section>

  <section class="card log-card">
    <div class="log-head">
      <strong>Log</strong>
      <span class="muted">{task.log.length} {task.log.length === 1 ? "entry" : "entries"}</span>
      <button class="btn btn-sm" onclick={copyLog} disabled={task.log.length === 0}>
        <Icon name={copied ? "check" : "copy"} size={14} />
        {copied ? "Copied" : "Copy"}
      </button>
    </div>
    <div class="log" bind:this={logEl} onscroll={onLogScroll}>
      {#each task.log as entry (entry.id)}
        <div class="log-line {entry.level}">
          <time>{formatClock(entry.time)}</time>
          <span>{entry.text}</span>
        </div>
      {/each}
    </div>
  </section>
{/if}

<style>
  .panel-idle {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 160px;
    color: var(--text-3);
    text-align: center;
  }

  .panel-idle p {
    margin: 0;
    max-width: 280px;
    color: var(--text-2);
  }

  .progress-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }

  .progress-title {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
  }

  .progress-title strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .status-icon {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
  }

  .status-icon.ok {
    background: var(--success-soft);
    color: var(--success);
  }

  .status-icon.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .bar {
    position: relative;
    height: 8px;
    overflow: hidden;
    border-radius: 8px;
    background: var(--surface-2);
  }

  .bar-thin {
    height: 4px;
    margin-top: 6px;
  }

  .bar-fill {
    width: 0;
    height: 100%;
    border-radius: 8px;
    background: var(--accent);
    transition: width 0.2s ease-out;
  }

  .failed .bar-fill {
    background: var(--danger);
  }

  .bar.indeterminate .bar-fill {
    position: absolute;
    width: 30%;
    animation: slide 1.1s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  .progress-detail {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    min-height: 20px;
    margin-top: 8px;
    color: var(--text-2);
    font-size: 13px;
  }

  .item {
    margin-top: 12px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .item-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--text-2);
  }

  .item-path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text);
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 20px;
    margin: 16px 0 0;
    padding: 0;
    list-style: none;
  }

  .steps li {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    font-size: 12.5px;
    font-weight: 500;
  }

  .step-dot {
    display: grid;
    place-items: center;
    flex: none;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--surface-2);
    font-size: 11px;
  }

  .steps li.active {
    color: var(--text);
  }

  .steps li.active .step-dot {
    background: var(--accent);
    color: var(--accent-text);
  }

  .steps li.done {
    color: var(--text-2);
  }

  .steps li.done .step-dot {
    background: var(--success-soft);
    color: var(--success);
  }

  .log-card {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 220px;
    padding: 0;
    overflow: hidden;
  }

  .log-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px 10px 16px;
    border-bottom: 1px solid var(--border);
  }

  .log-head .muted {
    flex: 1;
  }

  .log {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 8px 0;
    font-family: var(--mono);
    font-size: 12px;
    line-height: 1.6;
    user-select: text;
  }

  .log-line {
    display: flex;
    gap: 12px;
    padding: 0 16px;
  }

  .log-line time {
    flex: none;
    color: var(--text-3);
  }

  .log-line span {
    overflow-wrap: anywhere;
  }

  .log-line.success span {
    color: var(--success);
  }

  .log-line.error span {
    color: var(--danger);
  }
</style>
