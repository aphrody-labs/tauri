---
"tauri-cli": "patch:enhance"
"@tauri-apps/cli": "patch:enhance"
---

Prefer bun: lockfile detection now ranks `bun` first (then deno, pnpm, yarn, npm) instead of following the directory listing order, `tauri init` falls back to bun when it is installed and no lockfile is found, the bun `beforeDevCommand` and `beforeBuildCommand` defaults are `bun run dev` and `bun run build` (`bun build` is the bundler, not the `build` script), and the plugin templates use bun scripts. Adds the `release-lto` workspace profile (fat LTO, `codegen-units = 1`, `panic = "abort"`, stripped).
