# Rumor

**Real-time pink, brown and white noise for the Windows tray.**

> 🚧 Early development: there is no release yet. See [ROADMAP.md](ROADMAP.md).

The pink noise you find on YouTube or Spotify is compressed audio. Noise is random and can't be compressed, so the codec mangles it (it sounds bubbly or metallic), and it is usually a loop your brain eventually notices. Rumor **computes the noise sample by sample** on your PC, like the iPhone's Background Sounds do. It is one click or one shortcut away, with no ads, no account and **zero telemetry**.

- Windows 10 and 11
- Tauri v2 + a Rust audio engine (`cpal`, WASAPI) + a tiny TypeScript panel
- MIT licensed

## Build from source

You need [Rust](https://rustup.rs) (MSVC toolchain), Node.js LTS and the Visual Studio Build Tools with the "Desktop development with C++" workload.

```bash
npm install
npm run tauri dev
```

Checks run by CI:

```bash
npx tsc --noEmit
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## En español

**Rumor** genera ruido rosa, marrón y blanco **en tiempo real** desde la bandeja del sistema de Windows. A diferencia del ruido de YouTube o Spotify, no es un audio comprimido ni un bucle: se calcula en el momento, igual que los Sonidos de fondo del iPhone. Sin anuncios, sin cuenta y sin telemetría.

Todavía está en desarrollo; la primera beta llegará como *pre-release* en GitHub Releases.

## License

[MIT](LICENSE)
