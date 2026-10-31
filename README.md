<div align="center">
<img src="docs/logo.png" width="128" height="128" alt="Asahiro logo">
<h1>Asahiro Waves</h1>
<p>A macOS launcher for Wuthering Waves on Apple Silicon, with its anti-cheat running as normal</p>
</div>

Asahiro Waves is a fork of [Asahiro](https://github.com/chwair/asahiro) by chwair, reworked for Wuthering Waves.
It runs the Windows Steam release of the game under a patched Wine. Nothing in the game or its anti-cheat
(ACE) is bypassed or disabled. The patches fill in the parts of Windows that Rosetta 2 and Wine lack.

## Status

- Works: installing, updating and playing the Steam release, with ACE's kernel driver running as normal
- Experimental: WWMI mods. WWMI loads into the game, but mods don't apply to characters yet, and ACE stops
  many modded launches. Mods may go against the game's terms of service, so only try them on an account you
  can afford to lose.

## Features

- Sets up its own Wine bottle with Steam inside it, with the fixes Steam needs to draw its window
- Patched Wine runner (CrossOver 26.1 source plus the patches in [wine/patches](wine/patches)), downloaded
  automatically
- DXMT for the game. Steam starts on Wine's own Direct3D and the app switches the runner to DXMT once Steam
  is up.
- Install, update and verify go through Steam. Play starts the game through the running Steam.
- WWMI install from its signed GitHub releases, and a mod manager (experimental, see above)
- Frame cap and the Metal HUD
- Installs next to upstream Asahiro without clashing: own name, bundle id and data folder

## Using Asahiro Waves

Requirements: an Apple Silicon Mac on macOS 26 or later with Rosetta 2, and a Steam account that owns
Wuthering Waves (it's free).

1. Build the app (see below). There are no prebuilt releases yet.
2. Open Asahiro Waves and click Install. It downloads the Wine runner (about 450 MB), creates the bottle and
   installs Steam, then opens Steam on the game's page.
3. Sign in to Steam in its window and install Wuthering Waves there.
4. Click Start. The app starts Steam if needed and has it launch the game.

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
</invoke>
