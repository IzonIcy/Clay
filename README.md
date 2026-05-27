# Clay
Clay is a fast Homebrew-compatible package manager, built in Rust.

Status: early CLI and core plumbing are in place. Installation is wired to Homebrew's JSON API and bottle downloads, with linking for `bin`, `lib`, `include`, and `share`.

## Build
```sh
cargo build
```

## CLI
```sh
clay install <formula> [--platform <tag>] [--force] [--only-deps] [--skip-recommended] [--build-from-source] [--overwrite]
clay uninstall <formula>
clay list [--versions]
clay outdated
clay upgrade [<formula>]
clay fetch <formula> [<formula> ...]
clay link <formula> [--version <ver>] [--overwrite]
clay unlink <formula> [--version <ver>]
clay cleanup
clay update
clay search <query> [--limit N] [--desc]
clay info <formula> [--json]
clay tap add <user/repo>
clay tap list
clay tap update
clay tap remove <user/repo>
clay cache clean
clay doctor
clay pin <formula>
clay unpin <formula>
```

## Config
- `CLAY_PREFIX`: override the install prefix (default tries `/opt/homebrew` on Apple Silicon, otherwise `/usr/local`).
- `CLAY_PLATFORM`: override bottle platform tag (example: `arm64_sonoma`, `x86_64_linux`).
