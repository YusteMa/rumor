# Contributing to Rumor

Thanks for helping. Rumor is small on purpose, so the most useful contributions right now are **bug reports from the beta**: Windows versions, headphones and sound cards we haven't tried.

## Reporting

- **Bugs:** open an issue with the bug template. Clicks, pops or any change in the sound are bugs: sound quality comes first.
- **Ideas and questions:** start a thread in [Discussions](https://github.com/YusteMa/rumor/discussions).
- English or Spanish, whichever you prefer.

## Building

You need [Rust](https://rustup.rs) (MSVC toolchain), Node.js LTS and the Visual Studio Build Tools with the "Desktop development with C++" workload.

```bash
npm install
npm run tauri dev
```

Before opening a pull request, run what CI runs:

```bash
npx tsc --noEmit
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

### Releasing

1. Bump the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`, and the MSI's numeric version in `tauri.conf.json` → `bundle.windows.wix.version` (it only takes numbers: `0.1.0.2` for `0.1.0-beta.2`, and it must go up).
2. Commit, then push a tag: `git tag v0.1.0-beta.2 && git push origin v0.1.0-beta.2`.
3. The release workflow builds and signs the installers, publishes the release and writes `latest.json` to the `updater` branch. Installed copies see it within 6 hours.

The installers are signed with the updater key (`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the repo secrets). If that key is lost, installed copies can't update any more and need a manual reinstall. Local `npm run tauri build` doesn't sign anything and needs no key.

## Ground rules

[SPEC.md](SPEC.md) describes the product and [CLAUDE.md](CLAUDE.md) the rules of the code. The ones that matter most:

- **The audio thread is sacred:** no allocations, locks, logging or I/O in the `cpal` callback.
- **Zero clicks:** every parameter change is smoothed.
- **State lives in Rust;** the panel only shows it.
- **No UI frameworks, no telemetry, no network requests** (the update check in `update.rs` is the only one).
- Every colour, size and duration comes from `src/styles/tokens.css`.
- Code, names and commits in English, with [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`…). UI text goes through `src/i18n/` in both Spanish and English.

For anything bigger than a fix, please open a Discussion first so we can agree on the design before you spend time on it.
