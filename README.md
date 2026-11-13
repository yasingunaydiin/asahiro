<div align="center">
<img src="docs/logo.png" width="128" height="128" alt="Asahiro logo">
<h1>Asahiro Waves</h1>
<p>A macOS launcher for a Certain Wavy Anime Game on Apple Silicon, with its anti-cheat running as normal</p>
</div>

Asahiro Waves is a fork of [Asahiro](https://github.com/chwair/asahiro) by chwair, reworked for a different game.
It runs the Windows Steam release of the game under a patched Wine. Nothing in the game or its anti-cheat
(ACE) is bypassed or disabled. The patches fill in the parts of Windows that Rosetta 2 and Wine lack.

## Status

- Works: installing, updating and playing the Steam-channel release, with the anti-cheat (ACE) kernel driver
  running as normal
- Works: WWMI mods, on an account you can afford to lose. Mods may go against the game's terms of service.
  See the notes below for what a mod needs to show up.

## Features

- Downloads the game straight from the publisher's servers, with checksums and resume. Steam only ships the
  publisher's launcher now, and its window stays blank under Wine.
- Sets up its own Wine bottle, with Steam inside it for sign-in
- Patched Wine runner (CrossOver 26.1 source plus the patches in [wine/patches](wine/patches)), downloaded
  automatically
- DXMT for the game
- Starts the game directly. With mods on, Steam is closed first: with Steam running next to 3DMigoto the game
  was closed within a minute.
- WWMI install from its signed GitHub releases, and a mod manager
- Custom resolution (experimental), frame cap and the Metal HUD
- Installs next to upstream Asahiro without clashing: own name, bundle id and data folder

## Using Asahiro Waves

Requirements: an Apple Silicon Mac on macOS 26 or later with Rosetta 2, and about 90 GB of free space (the HD
version). An external SSD works well, but plug it straight into the Mac: through a USB 2.0 hub or adapter,
loading takes minutes.

1. Build the app (see below). There are no prebuilt releases yet.
2. Open Asahiro Waves and click Install. It downloads the Wine runner (about 450 MB), creates the bottle and
   installs Steam, then downloads the game.
3. Click Start.

## Mods

- Mods must be made for the current game version. After a game update, a character's model can change and
  older mods stop matching until their author updates them.
- In the game's graphics settings, set **LOD Bias** to **Ultra High**. Without it mod textures don't apply.
- Don't write the game's config files from outside the game: the anti-cheat closes the game when it sees them
  change.

## Building

You need:

- [Rust](https://rustup.rs), stable, and the Xcode command line tools. Full Xcode is optional: without it the
  app gets a flat icon instead of the Liquid Glass one.
- Optional: `mingw-w64` (`brew install mingw-w64`) to rebuild the injector and the Steam web helper wrapper.
  Prebuilt copies are in the repo.
- Optional: `create-dmg` (`brew install create-dmg`) to build the dmg image

Build the app into `dist/Asahiro Waves.app`:

```bash
./scripts/build.sh
```

Build the dmg image into `dist/Asahiro-Waves-<version>.dmg`:

```bash
./scripts/build-dmg.sh
```

The patched Wine runner is built separately with `scripts/wuwa-build-runner.sh` and packaged with
`scripts/wuwa-package-runner.sh`. [wine/README.md](wine/README.md) explains the patches and the build.

## Special thanks

- [Asahiro](https://github.com/chwair/asahiro) by chwair, which this is built on
- [dwproton](https://dawn.wine/dawn-winery/dwproton) and [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom)
  for the ACE kernel patches. Their authors are credited in each patch.
- [YAAGL](https://github.com/yaagl/yet-another-anime-game-launcher) for the wine runner and DXMT builds
- [DXMT](https://github.com/3Shain/dxmt)
- [CodeWeavers](https://www.codeweavers.com) for CrossOver and its sources, and the [Wine](https://www.winehq.org) project
- [WWMI](https://github.com/SpectrumQT/WWMI-Package), [XXMI](https://github.com/SpectrumQT/XXMI-Launcher) and
  [3DMigoto](https://github.com/bo3b/3Dmigoto)
- [Chakra Petch](https://fonts.google.com/specimen/Chakra+Petch) by Cadson Demak

Asahiro Waves is not affiliated with Kuro Games, Valve, CodeWeavers or Apple.

## License

MIT, see [LICENSE](LICENSE). The original Asahiro copyright notice stays in place.

PS to chwair: thank you for making Asahiro for ZZZ. Modding broke on YAAGL exactly two days before your
first commit, so I'm very thankful for it.
