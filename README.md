# backuptool

A backup tool built around **BPFS**, a custom archive format. It includes a command-line tool and a desktop GUI.

## Features

- Deduplicates identical files so each unique blob is stored once.
- Groups similar files together before compressing them, which improves the compression ratio.
- Compresses data in 64 MB blocks on all CPU cores with Zstandard (default) or Brotli, so memory use stays bounded and restoring a file only decompresses the blocks it needs.
- Stores already-compressed or high-entropy data (images, video, archives) without compressing it again.
- Uses generational archives. Each generation has its own string table, and its SHA-256 hash links it to the previous one. Generations can optionally be signed with Ed25519.
- Verifies archives: by default it checks every section hash and the generation hash chain, and `--deep` also re-hashes every blob's content.

## Repository layout

| Path                  | Description                                                         |
| --------------------- | ------------------------------------------------------------------- |
| `crates/bpfs-core`    | Shared types, format constants, signing, sorting                    |
| `crates/bpfs-pack`    | Scans directories, deduplicates, writes archives                    |
| `crates/bpfs-read`    | Parses, verifies, and extracts archives                             |
| `crates/bpfs-cli`     | Command-line interface                                              |
| `crates/gui/frontend` | Desktop GUI (Tauri 2 + SvelteKit)                                   |
| `imhex.pattern`       | [ImHex](https://imhex.werwolv.net/) pattern for inspecting archives |

## Building

Requires a stable Rust toolchain (pinned via `rust-toolchain.toml`).

```sh
cargo build --release -p bpfs-cli
cargo test --workspace --exclude backuptool
```

### GUI

Requires Node.js, [pnpm](https://pnpm.io/), and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your platform.

```sh
cd crates/gui/frontend
pnpm install
pnpm tauri dev      # run in development
pnpm tauri build    # produce installers
```

## CLI usage

```sh
bpfs-cli pack <SRC> <OUT> [--final-hash] [--compression zstd|brotli|none]
bpfs-cli ls <ARCHIVE> [--tree]
bpfs-cli extract <ARCHIVE> <DEST> [--path <PATH>...] [--overwrite]
bpfs-cli verify <ARCHIVE> [--deep]
```

## License

Licensed under the [GNU General Public License v3.0 or later](LICENSE).
