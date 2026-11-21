# OpenPax

An open-source studio for making Minecraft: Java Edition resource packs, written entirely in Rust.

OpenPax aims to be one desktop app for the whole job: managing the pack, drawing textures,
building models and animating them. It is early software; see the roadmap for what exists today.

## What works today

- **Packs**: create a new pack for a chosen game version, open a pack folder, import a `.zip`,
  edit `pack.mcmeta`, and export a `.zip` that Minecraft can load.
- **Explorer**: assets grouped by namespace and type, with search and texture thumbnails.
- **Pixel editor**: pencil, eraser, fill, colour picker, line, rectangle, ellipse, rectangular
  selection and move; brush size, X/Y mirroring, palette, layers with opacity, undo/redo,
  canvas resize and flip, pixel grid and tiling preview.
- **Texture animation**: edit the `.mcmeta` animation settings, add or remove frames, and watch
  a live preview at game speed.
- **Everything else** (models, blockstates, language files, ...) opens in a text editor with
  JSON validation.

OpenPax never rewrites files you did not change, and keeps JSON fields it does not understand.
Layers are stored in a `.openpax` folder inside the pack; the PNG Minecraft reads stays a normal
flattened image, and `.openpax` is left out of exported zips.

## Roadmap

1. **3D modelling**: cuboid model editor with a wgpu viewport, UV editing, painting on the model.
2. **Animation**: timeline, display-transform poser, keyframed rigs.
3. **Pack tooling**: validation, blockstate / item definition / language / sound / font editors,
   multi-version overlays.
4. Bedrock Edition support.

## Building

You need a recent stable [Rust toolchain](https://rustup.rs).

```bash
cargo run --release -p openpax-app
```

To open a pack (and optionally some of its files) straight away:

```bash
cargo run --release -p openpax-app -- path/to/pack assets/minecraft/textures/block/stone.png
```

Run the checks with:

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Linux

Install the usual windowing libraries first, for example on Debian/Ubuntu:
`sudo apt install libgtk-3-dev libxkbcommon-dev libwayland-dev`.

### Windows without Visual Studio

The default Windows toolchain needs the Visual Studio C++ build tools and a Windows SDK. If you
would rather not install those, rustup's self-contained LLVM toolchain works too:

```bash
rustup toolchain install stable-x86_64-pc-windows-gnullvm
```

```bash
rustup override set stable-x86_64-pc-windows-gnullvm
```

Then create `.cargo/config.toml` in the repository (it is git-ignored) with:

```toml
[target.x86_64-pc-windows-gnullvm]
linker = "rust-lld"
rustflags = ["-Clinker-flavor=ld.lld", "-Clink-self-contained=yes", "-Clink-arg=-LC:/Windows/System32"]
```

## Layout

| Crate | Purpose |
| --- | --- |
| `crates/openpax-core` | Pack model: `pack.mcmeta`, asset index, texture metadata, zip import/export. No UI. |
| `crates/openpax-paint` | Pixel-art engine: bitmaps, layers, tools, undo history, PNG I/O. No UI. |
| `crates/openpax-app` | The `openpax` desktop app (egui + wgpu). |

The library crates are headless on purpose, so editing logic can be unit tested without a window.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

OpenPax is not affiliated with Mojang or Microsoft. It ships no Minecraft assets.
