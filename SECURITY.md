# Security

Rumor updates itself, so a security bug can reach every installed copy. Thank you for reporting it privately.

## Reporting a vulnerability

Use **[Report a vulnerability](https://github.com/YusteMa/rumor/security/advisories/new)** (Security tab → Advisories). Please don't open a public issue. English or Spanish.

You'll get an answer within 7 days. Once a fix is released, the advisory is published with credit to you, if you want it.

## Supported versions

Only the latest release gets fixes. Installed copies update themselves unless updates were turned off in Settings → About.

## What Rumor does on the network

- Checks `raw.githubusercontent.com/YusteMa/rumor/updater/latest.json` for updates (can be turned off). Updates are only installed if their signature matches the public key built into the app.
- Downloads an ambient sound from this repository's `ambients-v1` release, only when you click it. The file is only used if its size and SHA-256 match the ones built into the app.

Nothing about you is ever sent.
