# L2 Survivor

Lineage II client rebuilt in Rust (Bevy), using the original client's models,
textures and animations, with gamepad support and a Vampire Survivors style
game loop.

The original client is read-only input. By default it is expected at
`G:/visery/l2-client`; override with `L2_CLIENT_DIR`.

## Layout

| Path | What |
|------|------|
| `crates/ue2` | Reader for the client's UE2 packages: `Lineage2Ver111/121` XOR decryption, name/import/export tables, tagged properties, textures (DXT1/3/5, P8, RGBA8, L8, G16), static meshes, skeletal meshes (incl. Lineage wedges), mesh animations, cross-package material resolution (`library`). |
| `crates/l2tool` | CLI for inspecting packages and exporting assets. |
| `game` | The Bevy game (`l2survivor`). |

## Running

```
cargo run -p l2survivor
```

A run starts on the character screen: 16 race/sex/archetype combinations
(human fighter/mystic, elf, dark elf, orc fighter/mystic, dwarf, kamael),
each assembled from the client's face, hair and starter-armor parts and
holding a class weapon (sword, rod, club, tomahawk or knife) on
`Weapon_R_Bone`. Standing still, the hero turns and swings at each volley.

Controls: WASD / arrows or the left stick / D-pad to move or pick, Enter /
Space or gamepad A to confirm, Esc or Start to pause. Attacks are automatic.

Environment variables:

- `L2_CLIENT_DIR` — client root.
- `L2_GROUND=<package>:<texture>` — ground texture, e.g.
  `ertheia_v_t:ertheia_a_ground00` (default).
- `L2_CLASS=<prefix>` — skip the character screen, e.g. `FElf`, `MKamael`.
- `L2_MODEL=<package>:<mesh>[:<clip>]` — model viewer instead of the game,
  e.g. `lineagemonsters:orc_m00:atk01`; `class:<prefix>[:clip]` shows an
  assembled player character. Several meshes can be joined with `+`, and
  `mesh@package/material` forces a material. `L2_VIEW_ANGLE`, `L2_VIEW_DIST`,
  `L2_VIEW_HEIGHT` fix the camera.
- `L2_VIEW=<package>:<static mesh>` — place one static mesh next to the start.
- `L2_SCREENSHOT=<file.png>` — save a frame after `L2_SCREENSHOT_AT` seconds
  (default 8) and quit. `L2_AUTOPILOT=1` walks the player in a circle.

## l2tool

```
cargo run --release -p l2tool -- info <package>...         # export table
cargo run --release -p l2tool -- names <package>           # name table
cargo run --release -p l2tool -- textures <package> <dir>  # all textures to PNG
cargo run --release -p l2tool -- scan <dir>...             # open every package, decode every texture
cargo run --release -p l2tool -- obj <package> <mesh> <out.obj> # static mesh to OBJ
cargo run --release -p l2tool -- materials <client> <package> <mesh>  # static or skeletal
cargo run --release -p l2tool -- bones <client> <package> <skeletal mesh>
cargo run --release -p l2tool -- alpha <client> <package> <material>
cargo run --release -p l2tool -- hex <package> <export> [n]
```

## Client format notes

- `.ukx` (animations) are mostly plain UE packages; `.utx/.usx/.unr/.u/.uax`
  start with the UTF-16 header `Lineage2Ver111` or `Lineage2Ver121` followed by
  the package XORed with one byte (0xAC for 111, name-derived for 121). The key
  is recovered from the package magic `0x9E2A83C1`.
- `.dat`/`.ini` in `system` use `Lineage2Ver413` (RSA + zlib), not handled yet.
- Package versions range from 117 to 132 (licensee 0 to 40).
- Newer textures carry extra Lineage data (padding, material path, HLSL
  snippet) between properties and mips; the reader finds mip 0 by its
  self-describing lazy-array header instead of hard-coding each layout.
- `systextures/l2ui_ct2.utx` uses an unknown wrapper (`FF ED FA CE`).
- Coverage over the whole client: 1441/1442 packages, 75 434/75 434 textures,
  15 276/15 276 static meshes, 11 489/11 517 skeletal meshes, 1 863/1 863
  animations. The 28 skeletal meshes left keep soft-skinned geometry only in
  `SkinningData`, which is not decoded yet.
- Bone math: stored child-bone quaternions are conjugated relative to the
  root (ActorX convention). Unreal→Bevy axes: (x, y, z) → (x, z, y),
  quaternions (x, y, z, w) → (−x, −z, −y, w). 1 UU = 1/27 m.
- Player characters: `<Prefix>_m000_f` face, `<Prefix>_m000_m00_ah/bh` hair
  pieces, `<Prefix>_<set>_u/l/g/b` armor in `animations/<race>.ukx`. Faces and
  hair have no material in the mesh; textures are
  `lineagefacetex.<Prefix>_m000_t00_f` and
  `<prefix>.<Prefix>_m000_t00_m00_ah/bh`. Armor uses `<Prefix>_<set>_tNN_<slot>`
  variants from `systextures/<prefix>.utx`. Player clips carry a race suffix
  (`run_1HS_MFighter`).
- Some skeletons repeat a bone name (the right hand has a second
  `Bip01_L_Finger01`); bones are matched by name and occurrence.
- Under a Shader the diffuse alpha is a specular mask; only `Opacity` maps,
  `AlphaTest` (with `AlphaRef`) or flags on bare textures make cutouts.
- Weapons are single-bone skeletal meshes in `animations/lineageweapons.ukx`
  without a material; the texture is `lineageweaponstex.<name>_t00_wp`. They
  hang rigidly on the character bone `Weapon_R_Bone` (`Shield_L_Bone` exists
  for shields).
