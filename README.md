# Clay

Clay is a fast Homebrew-compatible package manager built in Rust.

Clay is early-stage software. The core CLI is in place, including formula lookup through Homebrew's JSON API, bottle downloads, checksum verification, dependency planning, registry tracking, and linking for `bin`, `lib`, `include`, and `share`.

## Quickstart

```sh
cargo build
CLAY_PREFIX="$HOME/.clay" cargo run -- install wget --dry-run
```

Clay defaults to Homebrew-style prefixes: `/opt/homebrew` on Apple Silicon macOS and `/usr/local` elsewhere. During development or testing, set `CLAY_PREFIX` to a disposable directory so installs do not touch your system Homebrew prefix.

## CLI

```sh
clay install <formula> [--platform <tag>] [--force] [--only-deps] [--skip-recommended] [--build-from-source] [--overwrite] [--dry-run]
clay uninstall <formula> [--ignore-dependencies]
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
clay leaves
clay autoremove
clay pin <formula>
clay unpin <formula>
```

## Examples

Preview an install plan:

```sh
clay install ripgrep --dry-run
```

Install a formula and its dependencies:

```sh
clay install ripgrep
```

List installed formulae with versions:

```sh
clay list --versions
```

Remove automatically installed dependency leaves:

```sh
clay autoremove
```

Check environment health:

```sh
clay doctor
```

## Configuration

- `CLAY_PREFIX`: override the install prefix.
- `CLAY_PLATFORM`: override the bottle platform tag, for example `arm64_sonoma`, `arm64_sequoia`, or `x86_64_linux`.

## Safety and reliability

Clay is designed to be conservative around package-manager operations:

- bottle downloads use an HTTP timeout, a User-Agent, retries, and atomic cache writes;
- cached tarballs are verified with SHA-256 before extraction;
- corrupt cached tarballs are removed after checksum failure;
- bottle extraction rejects archive entries that escape the destination;
- registry writes are atomic via temporary file and rename;
- install operations use a prefix-scoped lock;
- uninstall refuses to remove packages required by other registry entries unless `--ignore-dependencies` is passed.

## Development

Useful local checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo check
```

The GitHub Actions workflow in `.github/workflows/ci.yml` runs the same core checks on pushes and pull requests.

## Current limitations

Clay is still early. Source builds currently delegate to `brew`, dependency metadata is based on Clay's registry, and Homebrew compatibility is not complete. Use a disposable `CLAY_PREFIX` while experimenting.
