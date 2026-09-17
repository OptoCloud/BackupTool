<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import TaskPanel from "$lib/TaskPanel.svelte";
  import { humanBytes, task, type TaskKind } from "$lib/task.svelte";

  type PackSummary = {
    generation_idx: number;
    file_count: number;
    dir_count: number;
    blob_count: number;
    new_blob_bytes: number;
    bytes_written: number;
    integrity_hash: string;
  };
  type FileEntry = { path: string; size: number };
  type VerifyResult = { generations: number; ok: boolean; message: string };
  type View = "backup" | "browse" | "restore";
  type Toast = { id: number; kind: "success" | "error"; text: string };
  type PathAction = { label: string; onclick: () => void };
  type Codec = "zstd" | "brotli" | "none";

  const views: { id: View; label: string; icon: IconName; hint: string; task: TaskKind }[] = [
    { id: "backup", label: "Back up", icon: "backup", hint: "Save a folder as a new snapshot", task: "backup" },
    { id: "browse", label: "Browse", icon: "browse", hint: "Inspect and verify an archive", task: "verify" },
    { id: "restore", label: "Restore", icon: "restore", hint: "Extract the latest snapshot", task: "restore" },
  ];

  const codecs: { id: Codec; label: string; hint: string }[] = [
    { id: "zstd", label: "Zstandard", hint: "Fast, good compression" },
    { id: "brotli", label: "Brotli", hint: "Much slower" },
    { id: "none", label: "None", hint: "Store everything as-is" },
  ];

  const MAX_ROWS = 2000;

  let view = $state<View>("backup");
  let archivePath = $state("");
  let archiveIsNew = $state(false);
  let srcDir = $state("");
  let destDir = $state("");
  let codec = $state<Codec>("zstd");
  let overwrite = $state(false);
  let deepVerify = $state(false);

  let loadingFiles = $state(false);
  let packResult = $state<PackSummary | null>(null);
  let restoreCount = $state<number | null>(null);
  let verifyResult = $state<VerifyResult | null>(null);
  let files = $state<FileEntry[] | null>(null);
  let query = $state("");
  let toasts = $state<Toast[]>([]);
  let toastId = 0;

  const busy = $derived(task.running || loadingFiles);
  const current = $derived(views.find((v) => v.id === view)!);
  const archiveName = $derived(baseName(archivePath));
  const filtered = $derived.by(() => {
    if (!files) return [];
    const q = query.trim().toLowerCase();
    return q ? files.filter((f) => f.path.toLowerCase().includes(q)) : files;
  });
  const totalSize = $derived(files?.reduce((sum, f) => sum + f.size, 0) ?? 0);

  function baseName(p: string): string {
    return p.split(/[\\/]/).filter(Boolean).pop() ?? "";
  }

  function toast(kind: Toast["kind"], text: string) {
    const id = ++toastId;
    toasts.push({ id, kind, text });
    setTimeout(() => dismiss(id), kind === "error" ? 8000 : 4000);
  }

  function dismiss(id: number) {
    toasts = toasts.filter((t) => t.id !== id);
  }

  function setArchive(path: string, isNew: boolean) {
    archiveIsNew = isNew;
    if (path === archivePath) return;
    archivePath = path;
    files = null;
    verifyResult = null;
    packResult = null;
    restoreCount = null;
    if (view === "browse" && !isNew) loadFiles();
  }

  async function openArchive() {
    const picked = await invoke<string | null>("pick_archive_file", { save: false });
    if (picked) setArchive(picked, false);
  }

  async function newArchive() {
    const picked = await invoke<string | null>("pick_archive_file", { save: true });
    if (picked) setArchive(picked.endsWith(".bpfs") ? picked : `${picked}.bpfs`, true);
  }

  async function pickFolder(target: "src" | "dest") {
    const picked = await invoke<string | null>("pick_folder");
    if (!picked) return;
    if (target === "src") srcDir = picked;
    else destDir = picked;
  }

  function selectView(v: View) {
    view = v;
    if (v === "browse" && archivePath && !archiveIsNew && !files) loadFiles();
  }

  async function doPack() {
    packResult = null;
    const summary = await task.run<PackSummary>("backup", "pack_archive", {
      src: srcDir,
      out: archivePath,
      compression: codec,
    });
    if (!summary) {
      if (task.status === "failed") toast("error", "Backup failed. See the log for details.");
      return;
    }
    packResult = summary;
    archiveIsNew = false;
    files = null;
    verifyResult = null;
    toast("success", `Snapshot #${summary.generation_idx + 1} saved`);
  }

  async function loadFiles() {
    loadingFiles = true;
    try {
      files = await invoke<FileEntry[]>("list_archive", { path: archivePath });
    } catch (e) {
      files = null;
      toast("error", `Could not open archive: ${e}`);
    } finally {
      loadingFiles = false;
    }
  }

  async function doVerify() {
    const deep = deepVerify;
    verifyResult = null;
    const result = await task.run<VerifyResult>("verify", "verify_archive", { path: archivePath, deep });
    if (!result) return;
    verifyResult = result;
    if (result.ok) toast("success", `Archive verified${deep ? " (including contents)" : ""}`);
    else toast("error", "Verification found a problem");
  }

  async function doRestore() {
    restoreCount = null;
    const count = await task.run<number>("restore", "extract_archive", {
      path: archivePath,
      dest: destDir,
      overwrite,
    });
    if (count === undefined) {
      if (task.status === "failed") toast("error", "Restore failed. See the log for details.");
      return;
    }
    restoreCount = count;
    toast("success", `Restored ${count.toLocaleString()} files`);
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast("success", "Copied to clipboard");
    } catch {
      toast("error", "Could not access the clipboard");
    }
  }
</script>

{#snippet pathField(label: string, value: string, placeholder: string, icon: IconName, actions: PathAction[])}
  <div class="field">
    <span class="field-label">{label}</span>
    <div class="path-picker" class:empty={!value}>
      <button class="path-main" onclick={actions[0].onclick} disabled={busy}>
        <span class="path-icon"><Icon name={icon} /></span>
        <span class="path-text">
          {#if value}
            <span class="path-name">{baseName(value) || value}</span>
            <span class="path-full"><bdi>{value}</bdi></span>
          {:else}
            <span class="path-name">{placeholder}</span>
          {/if}
        </span>
      </button>
      <span class="path-actions">
        {#each actions as action (action.label)}
          <button class="path-action" onclick={action.onclick} disabled={busy}>{action.label}</button>
        {/each}
      </span>
    </div>
  </div>
{/snippet}

{#snippet toggle(label: string, description: string, checked: boolean, onchange: (v: boolean) => void)}
  <label class="toggle">
    <span class="toggle-text">
      <span class="toggle-label">{label}</span>
      <span class="toggle-desc">{description}</span>
    </span>
    <input
      type="checkbox"
      role="switch"
      {checked}
      disabled={busy}
      onchange={(e) => onchange(e.currentTarget.checked)}
    />
    <span class="switch" aria-hidden="true"></span>
  </label>
{/snippet}

{#snippet stat(label: string, value: string)}
  <div class="stat">
    <span class="stat-value tabular">{value}</span>
    <span class="stat-label">{label}</span>
  </div>
{/snippet}

<div class="app">
  <aside class="sidebar">
    <div class="brand">
      <img src="/favicon.png" alt="" width="28" height="28" />
      <span class="label">backuptool</span>
    </div>

    <nav>
      {#each views as v (v.id)}
        <button class="nav-item" class:active={view === v.id} title={v.label} onclick={() => selectView(v.id)}>
          <Icon name={v.icon} />
          <span class="label">{v.label}</span>
          {#if task.running && task.kind === v.task}
            <span class="nav-busy spinner" aria-label="Running"></span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="archive-card">
      <span class="archive-card-label label">Archive</span>
      {#if archivePath}
        <div class="archive-current" title={archivePath}>
          <Icon name="archive" size={16} />
          <span class="label">{archiveName}</span>
        </div>
      {:else}
        <p class="archive-none label">No archive selected</p>
      {/if}
      <div class="archive-actions">
        <button class="btn btn-sm" title="Open archive" onclick={openArchive} disabled={busy}>
          <Icon name="archive" size={14} /><span class="label">Open</span>
        </button>
        <button class="btn btn-sm" title="New archive" onclick={newArchive} disabled={busy}>
          <Icon name="plus" size={14} /><span class="label">New</span>
        </button>
      </div>
    </div>
  </aside>

  <main class="content">
    {#if busy}<div class="top-progress" aria-hidden="true"></div>{/if}

    <div class="page">
      <header class="page-header">
        <h1>{current.label}</h1>
        <p>{current.hint}</p>
      </header>

      {#if view === "backup"}
        <div class="columns">
          <div class="col">
            <section class="card">
              {@render pathField("Source folder", srcDir, "Choose a folder to back up", "folder", [
                { label: srcDir ? "Change" : "Choose", onclick: () => pickFolder("src") },
              ])}
              {@render pathField("Archive", archivePath, "Open an existing archive or create a new one", "archive", [
                { label: archivePath ? "Open…" : "Open existing", onclick: openArchive },
                { label: archivePath ? "New…" : "Create new", onclick: newArchive },
              ])}
              {#if archivePath}
                <p class="note">
                  {#if archiveIsNew}
                    A new archive will be created here on the first backup.
                  {:else}
                    New snapshots are appended to the archive. Files already stored in earlier snapshots are not stored again.
                  {/if}
                </p>
              {/if}

              <div class="divider"></div>

              <span class="field-label">Compression</span>
              <div class="segmented" role="radiogroup" aria-label="Compression">
                {#each codecs as c (c.id)}
                  <button
                    role="radio"
                    aria-checked={codec === c.id}
                    class:selected={codec === c.id}
                    disabled={busy}
                    onclick={() => (codec = c.id)}
                  >
                    <span class="segment-label">{c.label}</span>
                    <span class="segment-hint">{c.hint}</span>
                  </button>
                {/each}
              </div>
              <p class="note">Photos, videos and archives are always stored as-is.</p>

              <div class="card-footer">
                <button class="btn btn-primary" onclick={doPack} disabled={busy || !srcDir || !archivePath}>
                  {#if task.running && task.kind === "backup"}
                    <span class="spinner"></span> Backing up…
                  {:else}
                    <Icon name="backup" size={16} /> Back up now
                  {/if}
                </button>
              </div>
            </section>

            {#if packResult}
              <section class="card">
                <div class="result-head">
                  <span class="badge badge-success">
                    <Icon name="check" size={14} /> Snapshot #{packResult.generation_idx + 1}
                  </span>
                </div>
                <div class="stats">
                  {@render stat("Files", packResult.file_count.toLocaleString())}
                  {@render stat("Folders", packResult.dir_count.toLocaleString())}
                  {@render stat("New data", humanBytes(packResult.new_blob_bytes))}
                  {@render stat("Written", humanBytes(packResult.bytes_written))}
                </div>
                <div class="hash">
                  <span class="field-label">Integrity hash</span>
                  <div class="hash-row">
                    <code>{packResult.integrity_hash}</code>
                    <button class="btn btn-icon" title="Copy" onclick={() => copy(packResult!.integrity_hash)}>
                      <Icon name="copy" size={16} />
                    </button>
                  </div>
                </div>
              </section>
            {/if}
          </div>

          <div class="col">
            <TaskPanel kind="backup" idleText="Progress and a detailed log appear here while a backup runs." />
          </div>
        </div>
      {:else if view === "browse"}
        {#if !archivePath || archiveIsNew}
          <section class="card empty-state">
            <Icon name="archive" size={36} />
            <h2>No archive open</h2>
            <p>Open an archive to see its files and check its integrity.</p>
            <button class="btn btn-primary" onclick={openArchive} disabled={busy}>Open archive…</button>
          </section>
        {:else}
          <div class="columns">
            <div class="col">
              <section class="card files">
                <div class="files-toolbar">
                  <div class="search">
                    <Icon name="search" size={16} />
                    <input type="search" placeholder="Filter files" bind:value={query} />
                  </div>
                  {#if files}
                    <span class="muted tabular">
                      {filtered.length.toLocaleString()} of {files.length.toLocaleString()} · {humanBytes(totalSize)}
                    </span>
                  {/if}
                  <button class="btn btn-icon" title="Reload" onclick={loadFiles} disabled={busy}>
                    <Icon name="refresh" size={16} />
                  </button>
                </div>

                {#if loadingFiles}
                  <div class="files-empty"><span class="spinner"></span> Loading files…</div>
                {:else if !files}
                  <div class="files-empty">
                    <button class="btn" onclick={loadFiles} disabled={busy}>Load files</button>
                  </div>
                {:else if filtered.length === 0}
                  <div class="files-empty">
                    {files.length ? "No files match your filter." : "This snapshot is empty."}
                  </div>
                {:else}
                  <div class="table">
                    <div class="row head">
                      <span>Name</span>
                      <span class="size">Size</span>
                    </div>
                    {#each filtered.slice(0, MAX_ROWS) as f (f.path)}
                      <div class="row">
                        <span class="name" title={f.path}>
                          <Icon name="file" size={15} />
                          <span class="dir">{f.path.slice(0, f.path.length - baseName(f.path).length)}</span>
                          <span class="base">{baseName(f.path)}</span>
                        </span>
                        <span class="size tabular">{humanBytes(f.size)}</span>
                      </div>
                    {/each}
                    {#if filtered.length > MAX_ROWS}
                      <div class="files-empty">
                        Showing the first {MAX_ROWS.toLocaleString()} files. Filter to narrow down.
                      </div>
                    {/if}
                  </div>
                {/if}
              </section>
            </div>

            <div class="col">
              <section class="card verify">
                <div class="verify-main">
                  <span class="verify-icon" class:ok={verifyResult?.ok} class:bad={verifyResult && !verifyResult.ok}>
                    <Icon name={verifyResult && !verifyResult.ok ? "alert" : "shield"} size={22} />
                  </span>
                  <div class="verify-text">
                    {#if !verifyResult}
                      <strong>Integrity not checked</strong>
                      <span>Re-hashes all archive data on disk.</span>
                    {:else if verifyResult.ok}
                      <strong>Archive is intact</strong>
                      <span>
                        {verifyResult.generations} snapshot{verifyResult.generations === 1 ? "" : "s"} verified
                      </span>
                    {:else}
                      <strong>Verification failed</strong>
                      <span class="error-text">{verifyResult.message}</span>
                    {/if}
                  </div>
                </div>
                <div class="verify-actions">
                  <label class="check" title="Also decompress every file and check its hash">
                    <input type="checkbox" bind:checked={deepVerify} disabled={busy} />
                    Check file contents
                  </label>
                  <button class="btn" onclick={doVerify} disabled={busy}>
                    {#if task.running && task.kind === "verify"}
                      <span class="spinner"></span> Verifying…
                    {:else}
                      Verify
                    {/if}
                  </button>
                </div>
              </section>
              <TaskPanel
                kind="verify"
                skipPhases={deepVerify ? [] : ["verifyingContent"]}
                idleText="Verification progress and log appear here."
              />
            </div>
          </div>
        {/if}
      {:else}
        <div class="columns">
          <div class="col">
            <section class="card">
              {@render pathField("Archive", archivePath, "Open an archive", "archive", [
                { label: archivePath ? "Change" : "Choose", onclick: openArchive },
              ])}
              {@render pathField("Destination", destDir, "Choose where to restore files", "folder", [
                { label: destDir ? "Change" : "Choose", onclick: () => pickFolder("dest") },
              ])}
              <div class="divider"></div>
              {@render toggle(
                "Overwrite existing files",
                "Replace files that already exist in the destination.",
                overwrite,
                (v) => (overwrite = v),
              )}
              <div class="card-footer">
                <button
                  class="btn btn-primary"
                  onclick={doRestore}
                  disabled={busy || !archivePath || archiveIsNew || !destDir}
                >
                  {#if task.running && task.kind === "restore"}
                    <span class="spinner"></span> Restoring…
                  {:else}
                    <Icon name="restore" size={16} /> Restore latest snapshot
                  {/if}
                </button>
              </div>
            </section>

            {#if restoreCount !== null}
              <section class="card">
                <div class="result-head">
                  <span class="badge badge-success"><Icon name="check" size={14} /> Restore complete</span>
                </div>
                <p class="note">{restoreCount.toLocaleString()} files written to <code>{destDir}</code></p>
              </section>
            {/if}
          </div>

          <div class="col">
            <TaskPanel kind="restore" idleText="Restore progress and log appear here." />
          </div>
        </div>
      {/if}
    </div>
  </main>
</div>

<div class="toasts" aria-live="polite">
  {#each toasts as t (t.id)}
    <div class="toast {t.kind}">
      <Icon name={t.kind === "success" ? "check" : "alert"} size={16} />
      <span>{t.text}</span>
      <button class="btn btn-icon" title="Dismiss" onclick={() => dismiss(t.id)}>
        <Icon name="close" size={14} />
      </button>
    </div>
  {/each}
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    height: 100vh;
  }

  /* Sidebar */
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 20px;
    width: 224px;
    padding: 18px 12px;
    overflow-y: auto;
    background: var(--sidebar);
    border-right: 1px solid var(--border);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .brand img {
    flex: none;
    border-radius: 7px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-2);
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }

  .nav-item :global(svg) {
    flex: none;
  }

  .nav-item:hover {
    background: var(--surface-2);
    color: var(--text);
  }

  .nav-item.active {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .nav-busy {
    width: 12px;
    height: 12px;
    margin-left: auto;
  }

  .archive-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: auto;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .archive-card-label {
    color: var(--text-3);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  .archive-current {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-weight: 500;
  }

  .archive-current :global(svg) {
    flex: none;
  }

  .archive-current span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .archive-none {
    margin: 0;
    color: var(--text-3);
  }

  .archive-actions {
    display: flex;
    gap: 6px;
  }

  .archive-actions .btn {
    flex: 1;
  }

  /* Content */
  .content {
    position: relative;
    height: 100vh;
    overflow: auto;
  }

  .top-progress {
    position: sticky;
    top: 0;
    z-index: 5;
    height: 2px;
    margin-bottom: -2px;
    overflow: hidden;
    background: var(--accent-soft);
  }

  .top-progress::after {
    content: "";
    position: absolute;
    inset: 0;
    width: 35%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(300%);
    }
  }

  .page {
    display: flex;
    flex-direction: column;
    height: 100%;
    max-width: 1440px;
    margin: 0 auto;
    padding: clamp(16px, 3vw, 32px) clamp(16px, 3vw, 40px);
  }

  .page-header {
    flex: none;
    margin-bottom: 20px;
  }

  .page-header h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }

  .page-header p {
    margin: 2px 0 0;
    color: var(--text-2);
  }

  .columns {
    display: grid;
    flex: 1;
    grid-template-columns: minmax(0, 1.15fr) minmax(0, 1fr);
    gap: 16px;
    min-height: 0;
  }

  .col {
    display: flex;
    flex-direction: column;
    gap: 16px;
    min-height: 0;
    margin: -4px;
    padding: 4px;
    overflow-y: auto;
  }

  .card-footer {
    display: flex;
    justify-content: flex-end;
    margin-top: 20px;
  }

  .divider {
    height: 1px;
    margin: 18px 0;
    background: var(--border);
  }

  .note {
    margin: 6px 0 0;
    color: var(--text-2);
    font-size: 13px;
  }

  /* Path picker */
  .field + .field {
    margin-top: 14px;
  }

  .path-picker {
    display: flex;
    align-items: center;
    gap: 4px;
    padding-right: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface-2);
    transition: border-color 0.12s;
  }

  .path-picker:hover:has(button:not(:disabled)) {
    border-color: var(--accent);
  }

  .path-picker.empty {
    border-style: dashed;
    background: none;
  }

  .path-main {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 10px 12px;
    border: none;
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .path-main:disabled {
    cursor: not-allowed;
  }

  .path-icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: 8px;
    background: var(--accent-soft);
    color: var(--accent);
  }

  .path-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .path-name {
    overflow: hidden;
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty .path-name {
    color: var(--text-2);
    font-weight: 400;
    white-space: normal;
  }

  .path-full {
    overflow: hidden;
    color: var(--text-3);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .path-actions {
    display: flex;
    flex: none;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .path-action {
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--accent);
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
  }

  .path-action:hover:not(:disabled) {
    background: var(--accent-soft);
  }

  .path-action:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Compression choice */
  .segmented {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(130px, 1fr));
    gap: 8px;
  }

  .segmented button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    text-align: left;
    cursor: pointer;
    transition: border-color 0.12s, background 0.12s;
  }

  .segmented button:hover:not(:disabled) {
    border-color: var(--border-strong);
  }

  .segmented button.selected {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .segmented button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .segment-label {
    font-weight: 600;
  }

  .selected .segment-label {
    color: var(--accent);
  }

  .segment-hint {
    color: var(--text-2);
    font-size: 12px;
  }

  /* Toggle */
  .toggle {
    position: relative;
    display: flex;
    align-items: center;
    gap: 16px;
    cursor: pointer;
  }

  .toggle-text {
    display: flex;
    flex: 1;
    flex-direction: column;
  }

  .toggle-label {
    font-weight: 500;
  }

  .toggle-desc {
    color: var(--text-2);
    font-size: 13px;
  }

  .toggle input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }

  .switch {
    position: relative;
    flex: none;
    width: 36px;
    height: 20px;
    border-radius: 20px;
    background: var(--border-strong);
    transition: background 0.15s;
  }

  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 0.15s;
  }

  .toggle input:checked + .switch {
    background: var(--accent);
  }

  .toggle input:checked + .switch::after {
    transform: translateX(16px);
  }

  .toggle input:focus-visible + .switch {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .toggle input:disabled + .switch {
    opacity: 0.5;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    white-space: nowrap;
    cursor: pointer;
  }

  .check input {
    accent-color: var(--accent);
  }

  /* Results */
  .result-head {
    margin-bottom: 16px;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px;
    border-radius: 20px;
    font-size: 12.5px;
    font-weight: 600;
  }

  .badge-success {
    background: var(--success-soft);
    color: var(--success);
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
    gap: 12px;
  }

  .stat {
    display: flex;
    flex-direction: column;
    padding: 12px 14px;
    border-radius: var(--radius);
    background: var(--surface-2);
  }

  .stat-value {
    font-size: 20px;
    font-weight: 650;
    letter-spacing: -0.02em;
  }

  .stat-label {
    color: var(--text-2);
    font-size: 12px;
  }

  .hash {
    margin-top: 16px;
  }

  .hash-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 6px 6px 12px;
    border-radius: 8px;
    background: var(--surface-2);
  }

  .hash-row code {
    flex: 1;
    overflow: hidden;
    color: var(--text-2);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Empty state */
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 48px 20px;
    color: var(--text-3);
    text-align: center;
  }

  .empty-state h2 {
    margin: 12px 0 4px;
    color: var(--text);
    font-size: 16px;
  }

  .empty-state p {
    margin: 0 0 18px;
    color: var(--text-2);
  }

  /* Verify */
  .verify {
    display: flex;
    flex: none;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .verify-main {
    display: flex;
    flex: 1 1 220px;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .verify-icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text-3);
  }

  .verify-icon.ok {
    background: var(--success-soft);
    color: var(--success);
  }

  .verify-icon.bad {
    background: var(--danger-soft);
    color: var(--danger);
  }

  .verify-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .verify-text span {
    color: var(--text-2);
    font-size: 13px;
  }

  .error-text {
    color: var(--danger) !important;
    overflow-wrap: anywhere;
  }

  .verify-actions {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-left: auto;
  }

  /* Files */
  .files {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 320px;
    padding: 0;
    overflow: hidden;
  }

  .files-toolbar {
    display: flex;
    flex: none;
    align-items: center;
    gap: 12px;
    padding: 12px 12px 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  .search {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 8px;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--text-3);
  }

  .search:focus-within {
    border-color: var(--accent);
  }

  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: none;
    color: var(--text);
  }

  .files-empty {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 28px;
    color: var(--text-3);
  }

  .table {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 7px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }

  .row:not(.head):hover {
    background: var(--surface-2);
  }

  .row.head {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
    color: var(--text-3);
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
  }

  .name :global(svg) {
    flex: none;
    color: var(--text-3);
  }

  .name .dir {
    flex-shrink: 1;
    overflow: hidden;
    color: var(--text-3);
    text-overflow: ellipsis;
  }

  .name .base {
    flex-shrink: 0;
    max-width: 75%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .size {
    flex: none;
    color: var(--text-2);
  }

  /* Toasts */
  .toasts {
    position: fixed;
    right: 20px;
    bottom: 20px;
    z-index: 10;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 40px));
  }

  .toast {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 6px 10px 14px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow-md);
    animation: pop 0.18s ease-out;
  }

  .toast > :global(svg) {
    flex: none;
    margin-top: 2px;
  }

  .toast span {
    flex: 1;
    overflow-wrap: anywhere;
  }

  .toast .btn-icon {
    width: 22px;
    height: 22px;
  }

  .toast.success > :global(svg) {
    color: var(--success);
  }

  .toast.error > :global(svg) {
    color: var(--danger);
  }

  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }

  /* Narrower windows: one column, the page scrolls as a whole. */
  @media (max-width: 1080px) {
    .page {
      height: auto;
      min-height: 100%;
    }

    .columns {
      grid-template-columns: minmax(0, 1fr);
    }

    .col {
      overflow: visible;
    }

    .col :global(.log-card) {
      flex: none;
      height: 320px;
    }

    .files {
      flex: none;
      height: 60vh;
    }
  }

  /* Narrow windows: collapse the sidebar into an icon rail. */
  @media (max-width: 760px) {
    .sidebar {
      align-items: center;
      width: 64px;
      padding: 16px 8px;
    }

    .sidebar .label {
      display: none;
    }

    .brand {
      padding: 0;
    }

    .nav-item {
      justify-content: center;
      width: 44px;
      height: 40px;
      padding: 0;
    }

    .nav-busy {
      display: none;
    }

    .archive-card {
      padding: 8px 4px;
      border: none;
      background: none;
    }

    .archive-current {
      justify-content: center;
      color: var(--accent);
    }

    .archive-actions {
      flex-direction: column;
    }

    .archive-actions .btn {
      width: 36px;
      height: 32px;
      padding: 0;
    }
  }
</style>
