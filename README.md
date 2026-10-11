# Vlauncher

Vlauncher is an open-source Minecraft launcher in early development. The goal is a polished launcher that manages instances and mods, with a focus on measurable Minecraft performance improvements.

## Project status

The repository currently contains the MIT license and the v1 FPS comparison protocol. There is no install, launch, mod-management or in-game benchmark implementation yet. Do not treat planned features or example numbers as working functionality.

## Direction

- Desktop shell: Tauri
- Launcher core: Rust, kept independent from the desktop shell where practical
- User data: stored under the current user's profile; no administrator rights required
- Mod discovery: Modrinth is the planned first provider
- Performance: compare repeatable vanilla and optimized profiles using the [FPS benchmark protocol v1](docs/benchmark-protocol-v1.md)

## Planned milestones

1. Create the Rust workspace and user-data path/configuration foundation.
2. Install and launch a vanilla Minecraft instance using user-owned files.
3. Add Fabric profiles and Modrinth mod discovery, installation and inspection.
4. Implement benchmark data collection and reports against protocol v1.
5. Connect the desktop UI and improve accessibility, documentation and platform support.

## Development

The Rust workspace and build instructions will be added with the first code milestone. Benchmark measurement is not implemented yet; reports must not display simulated FPS as real results.

## License

Vlauncher is licensed under the MIT License. See [LICENSE](LICENSE).
