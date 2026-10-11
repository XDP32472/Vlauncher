# Vlauncher

Vlauncher is an open-source Minecraft launcher in early development. The goal is a polished launcher that manages instances and mods, with a focus on measurable Minecraft performance improvements.

## Project status

The repository contains the MIT license, a v1 FPS comparison protocol, and the first Rust core foundation for user-owned paths and configuration. Installing or launching Minecraft, managing mods, and collecting in-game benchmark data are not implemented yet. Example values in the protocol are placeholders, not measurements.

## Direction

- Desktop shell: Tauri
- Launcher core: Rust, independent from the desktop shell where practical
- User data: stored under the current user's profile; no administrator rights required
- Mod discovery: Modrinth is the planned first provider
- Performance: compare repeatable vanilla and optimized profiles using the [FPS benchmark protocol v1](docs/benchmark-protocol-v1.md)

## Planned milestones

1. Build the user-data path and configuration foundation.
2. Install and launch a vanilla Minecraft instance using user-owned files.
3. Add Fabric profiles and Modrinth mod discovery, installation and inspection.
4. Implement benchmark data collection and reports against protocol v1.
5. Connect the desktop UI and improve accessibility, documentation and platform support.

## Development

Install the stable Rust toolchain, then from the repository root run:

```sh
cargo fmt --all -- --check
cargo check --workspace
```

Benchmark measurement is not implemented yet; reports must not display simulated FPS as real results.

## License

Vlauncher is licensed under the MIT License. See [LICENSE](LICENSE).
