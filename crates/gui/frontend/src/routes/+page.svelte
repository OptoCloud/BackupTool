<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  type PackSummary = {
    generation_idx: number;
    file_count: number;
    dir_count: number;
    blob_count: number;
    new_blob_bytes: number;
    integrity_hash: string;
  };

  type FileEntry = { path: string; size: number };

  type VerifyResult = { generations: number; ok: boolean; message: string };

  let srcDir = $state("");
  let archivePath = $state("");
  let destDir = $state("");
  let noCompress = $state(false);
  let overwrite = $state(false);
  let deepVerify = $state(false);

  let busy = $state(false);
  let status = $state("");
  let files = $state<FileEntry[]>([]);

  function humanBytes(n: number): string {
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let size = n;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
      size /= 1024;
      unit += 1;
    }
    return `${size.toFixed(unit === 0 ? 0 : 2)} ${units[unit]}`;
  }

  async function choosePickFolder(target: "src" | "dest") {
    const picked = await invoke<string | null>("pick_folder");
    if (picked) {
      if (target === "src") srcDir = picked;
      else destDir = picked;
    }
  }

  async function chooseArchive(save: boolean) {
    const picked = await invoke<string | null>("pick_archive_file", { save });
    if (picked) archivePath = picked;
  }

  async function doPack() {
    if (!srcDir || !archivePath) {
      status = "Pick both a source folder and an archive path first.";
      return;
    }
    busy = true;
    status = "Packing…";
    try {
      const summary = await invoke<PackSummary>("pack_archive", {
        src: srcDir,
        out: archivePath,
        noCompress,
      });
      status =
        `Generation ${summary.generation_idx}: ${summary.file_count} files, ` +
        `${summary.dir_count} dirs, ${summary.blob_count} blobs ` +
        `(${humanBytes(summary.new_blob_bytes)} new). ` +
        `integrity_hash=${summary.integrity_hash.slice(0, 16)}…`;
    } catch (e) {
      status = `Pack failed: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function doList() {
    if (!archivePath) {
      status = "Pick an archive first.";
      return;
    }
    busy = true;
    status = "Listing…";
    try {
      files = await invoke<FileEntry[]>("list_archive", { path: archivePath });
      status = `${files.length} file(s) in the current snapshot.`;
    } catch (e) {
      status = `List failed: ${e}`;
      files = [];
    } finally {
      busy = false;
    }
  }

  async function doExtract() {
    if (!archivePath || !destDir) {
      status = "Pick an archive and a destination folder first.";
      return;
    }
    busy = true;
    status = "Extracting…";
    try {
      const count = await invoke<number>("extract_archive", {
        path: archivePath,
        dest: destDir,
        overwrite,
      });
      status = `Extracted ${count} file(s) to ${destDir}.`;
    } catch (e) {
      status = `Extract failed: ${e}`;
    } finally {
      busy = false;
    }
  }

  async function doVerify() {
    if (!archivePath) {
      status = "Pick an archive first.";
      return;
    }
    busy = true;
    status = "Verifying…";
    try {
      const result = await invoke<VerifyResult>("verify_archive", {
        path: archivePath,
        deep: deepVerify,
      });
      status = result.ok
        ? `OK: ${result.generations} generation(s) verified${deepVerify ? " (deep)" : ""}.`
        : `FAILED: ${result.message}`;
    } catch (e) {
      status = `Verify failed: ${e}`;
    } finally {
      busy = false;
    }
  }
</script>

<main class="container">
  <h1>BPFS Archive Manager</h1>

  <section class="panel">
    <h2>Source</h2>
    <div class="row">
      <input placeholder="Folder to back up" bind:value={srcDir} />
      <button onclick={() => choosePickFolder("src")}>Browse…</button>
    </div>
  </section>

  <section class="panel">
    <h2>Archive</h2>
    <div class="row">
      <input placeholder="Archive file (.bpfs)" bind:value={archivePath} />
      <button onclick={() => chooseArchive(false)}>Open…</button>
      <button onclick={() => chooseArchive(true)}>Save as…</button>
    </div>
  </section>

  <section class="panel actions">
    <h2>Pack</h2>
    <label><input type="checkbox" bind:checked={noCompress} /> Disable compression</label>
    <button disabled={busy} onclick={doPack}>Pack folder into archive</button>
  </section>

  <section class="panel actions">
    <h2>Inspect</h2>
    <button disabled={busy} onclick={doList}>List files</button>
    <label><input type="checkbox" bind:checked={deepVerify} /> Deep verify (recheck every blob hash)</label>
    <button disabled={busy} onclick={doVerify}>Verify archive</button>
  </section>

  <section class="panel">
    <h2>Extract</h2>
    <div class="row">
      <input placeholder="Destination folder" bind:value={destDir} />
      <button onclick={() => choosePickFolder("dest")}>Browse…</button>
    </div>
    <label><input type="checkbox" bind:checked={overwrite} /> Overwrite existing files</label>
    <button disabled={busy} onclick={doExtract}>Extract current snapshot</button>
  </section>

  <p class="status" class:busy>{status}</p>

  {#if files.length > 0}
    <section class="panel">
      <h2>Files ({files.length})</h2>
      <ul class="file-list">
        {#each files as f (f.path)}
          <li><span class="path">{f.path}</span><span class="size">{humanBytes(f.size)}</span></li>
        {/each}
      </ul>
    </section>
  {/if}
</main>

<style>
  :root {
    font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
    color: #0f0f0f;
    background-color: #f6f6f6;
  }

  .container {
    max-width: 720px;
    margin: 0 auto;
    padding: 2rem 1rem;
  }

  h1 {
    text-align: center;
  }

  .panel {
    margin-bottom: 1.25rem;
    padding: 1rem;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.03);
  }

  .panel h2 {
    margin: 0 0 0.5rem 0;
    font-size: 1rem;
  }

  .row {
    display: flex;
    gap: 0.5rem;
  }

  .row input {
    flex: 1;
  }

  .actions button {
    margin-right: 0.5rem;
  }

  input {
    border-radius: 8px;
    border: 1px solid #ccc;
    padding: 0.5em 0.8em;
  }

  button {
    border-radius: 8px;
    border: 1px solid transparent;
    padding: 0.5em 1em;
    cursor: pointer;
    background-color: #ffffff;
    box-shadow: 0 2px 2px rgba(0, 0, 0, 0.15);
  }

  button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .status {
    min-height: 1.5em;
    font-weight: 500;
  }

  .status.busy {
    color: #396cd8;
  }

  .file-list {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 300px;
    overflow-y: auto;
  }

  .file-list li {
    display: flex;
    justify-content: space-between;
    padding: 0.25em 0;
    border-bottom: 1px solid rgba(0, 0, 0, 0.06);
    font-family: monospace;
    font-size: 0.85rem;
  }

  .file-list .size {
    color: #666;
    margin-left: 1rem;
    white-space: nowrap;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      color: #f6f6f6;
      background-color: #2f2f2f;
    }
    .panel {
      background: rgba(255, 255, 255, 0.06);
    }
    button {
      color: #fff;
      background-color: #0f0f0f98;
    }
    input {
      background-color: #1e1e1e;
      color: #f6f6f6;
      border-color: #444;
    }
  }
</style>
