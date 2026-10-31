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

## Ground rules

[SPEC.md](SPEC.md) describes the product and [CLAUDE.md](CLAUDE.md) the rules of the code. The ones that matter most:

- **The audio thread is sacred:** no allocations, locks, logging or I/O in the `cpal` callback.
- **Zero clicks:** every parameter change is smoothed.
- **State lives in Rust;** the panel only shows it.
- **No UI frameworks, no telemetry, no network requests.**
- Every colour, size and duration comes from `src/styles/tokens.css`.
- Code, names and commits in English, with [Conventional Commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:`…). UI text goes through `src/i18n/` in both Spanish and English.

For anything bigger than a fix, please open a Discussion first so we can agree on the design before you spend time on it.
