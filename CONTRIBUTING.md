# Contributing

Thanks for helping. Omega Zap is part of the Omega family of open-source apps, and anyone is welcome
to report a problem, suggest something, or send code.

## Reporting a bug

[Open an issue](https://github.com/joaomdsferro/omega-zap/issues/new/choose) with the bug template:
what you did, what you expected, what happened, the Omega Zap version and your distribution.

**Never share message contents, phone numbers, QR codes, or anything from
`~/.local/share/rocks.omegazap.desktop/`.** Describe the problem instead, or redact screenshots.

Security problems go privately, as [SECURITY.md](SECURITY.md) explains.

## Setting up

Requirements: Rust (stable, 1.94+), Node.js 22+, [pnpm](https://pnpm.io), and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for Linux.

```sh
pnpm install
pnpm tauri dev
```

GIF and sticker search need a [KLIPY](https://klipy.com) API key (Partner Panel → API Keys). Put it in a `.env` file
at the repository root (it is gitignored):

```sh
KLIPY_API_KEY=your-key
```

A debug build reads it when you open the GIFs tab, so no rebuild is needed; a release build takes it
in at compile time. Without one, everything else works and the search boxes say search is off.

A debug build is called "Omega Zap (Dev)" and has its own icon. It uses the same data folder as an
installed copy, so quit one before starting the other.

Before sending changes:

```sh
pnpm build                                   # type-checks and builds the UI
cargo test --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path signal-sidecar/Cargo.toml
```

## Pull requests

- Branch from `main` and keep each pull request to one change.
- Commits follow [Conventional Commits](https://www.conventionalcommits.org): `feat:`, `fix:`, `perf:`,
  `build:`, `docs:`, `chore:`.
- Match the code around you: its naming, comments and structure.
- Interface text is in plain English, short, and written for someone who doesn't code.
- Every clickable element shows a pointer cursor; disabled ones show `not-allowed`.
- Performance matters: the app should stay light in the background. Mention memory or CPU effects in the
  pull request when there are any.

## Dependencies

npm packages are installed with pnpm, which only accepts versions published at least two days ago
(`pnpm-workspace.yaml`). Rust crates from git are pinned to a revision.

## Releases

A release goes out when a merge to `main` bumps the version. Bump it in `package.json`,
`src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `signal-sidecar/Cargo.toml` (and their
`Cargo.lock`), and the release workflow builds the installers and publishes them as `v<version>`.

## License

Contributions are under the [MIT license](LICENSE), except in `signal-sidecar/`, which is AGPL-3.0-only.
