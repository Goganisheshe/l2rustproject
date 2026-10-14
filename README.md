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

The default mode is the offline Lineage II (see `docs/OFFLINE_PLAN.md`):
create a character, start in your race's village, walk the streamed world
(tiles load in the background, 3×3 around you) with building collision.
Movement: WASD / left stick (camera-relative), Shift walks; camera: Q/E,
right stick or right mouse drag, wheel / triggers zoom. `L2_SURVIVOR=1`
runs the survivors prototype described below.

World data comes from the L2J Mobius Classic 3.0 datapack (`L2_DATAPACK`,
default `G:/visery/l2-datapack/classic`): NPC stats and spawns, class
templates, experience, server skills and class skill trees. Combat: Tab /
click / RB target, F / X auto-attack; skills are learned automatically as you
level (SP is spent) and sit on the 1–9 hotbar (gamepad: LB + A/B/X/Y = 1–4,
LT + A/B/X/Y = 5–8). Test helpers: `L2_AUTOFIGHT=1`, `L2_AUTOCAST=1`,
`L2_DEBUG_NPC=1`, `L2_START="x,y,z"`.

A run starts on the character screen: 16 race/sex/archetype combinations
(human fighter/mystic, elf, dark elf, orc fighter/mystic, dwarf, kamael),
each assembled from the client's face, hair and starter-armor parts and
holding a class weapon (sword, rod, club, tomahawk or knife) on
`Weapon_R_Bone`. Standing still, the hero turns and swings at each volley.

Skills come from the client's tables: 1 673 offensive skills (661 magic for
mystics, 1 012 physical for fighters) can be learned on level-up, each cast
with the character's own combo animation (e.g. Wind Strike: CastMid ->
CastEnd -> MagicShot) and its client effects (cast, flight, impact). Waves
rotate through 683 monster kinds from `npcgrp.dat` with their textures.

Controls: WASD / arrows or the left stick / D-pad to move or pick, Enter /
Space or gamepad A to confirm, Esc or Start to pause. Attacks are automatic.

Environment variables:

- `L2_CLIENT_DIR` — client root.
- `L2_GROUND=<package>:<texture>` — ground texture, e.g.
  `ertheia_v_t:ertheia_a_ground00` (default).
- `L2_WORLD=<tile>[,<tile>...]` — world viewer instead of the game, e.g.
  `20_21` (Dion / Cruma). WASD/left stick move, Q/E or triggers down/up,
  arrows/right stick look, Shift/A faster.
- `L2_CLASS=<prefix>` — skip the character screen, e.g. `FElf`, `MKamael`.
- `L2_SKILLS=<id,id,...>` — starting skills, e.g. `1177,1239`.
- `L2_LIST_SKILLS=<file>` — write the playable skill table (id, name,
  animations, effects) and continue.
- `L2_ZOOM=<k>` — camera distance factor (e.g. `0.35` for a close-up).
- `L2_FONT=<ttf>` — UI font (default: Segoe UI / Arial for Cyrillic names).
- `L2_MODEL=<package>:<mesh>[:<clip>]` — model viewer instead of the game,
  e.g. `lineagemonsters:orc_m00:atk01`; `class:<prefix>[:clip]` shows an
  assembled player character. Several meshes can be joined with `+`, and
  `mesh@package/material` forces a material. `L2_VIEW_ANGLE`, `L2_VIEW_DIST`,
  `L2_VIEW_HEIGHT` fix the camera.
- `L2_EFFECT=<effect class>` — with `L2_MODEL`, replay an effect every 1.5 s
  sweeping along +X, e.g. `el_wind_strike_fl`.
- `L2_VIEW=<package>:<static mesh>` — place one static mesh next to the start.
- `L2_SCREENSHOT=<file.png>` — save a frame after `L2_SCREENSHOT_AT` seconds
  (default 8) and quit; `L2_SCREENSHOT_COUNT` / `L2_SCREENSHOT_STEP` take a
  burst (`shot_0.png`, ...). `L2_AUTOPILOT=1` walks the player in a circle.

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
cargo run --release -p l2tool -- dat <client>                # decode skillgrp/npcgrp/... and summarize
cargo run --release -p l2tool -- props <package> <export>     # tagged properties
cargo run --release -p l2tool -- defaults <package> <class>   # class default properties
cargo run --release -p l2tool -- effect <package> <class>     # effect emitters
cargo run --release -p l2tool -- map <client> <tile>           # map actors and terrain layers
cargo run --release -p l2tool -- hex <package> <export> [n]
```

## Client format notes

- `.ukx` (animations) are mostly plain UE packages; `.utx/.usx/.unr/.u/.uax`
  start with the UTF-16 header `Lineage2Ver111` or `Lineage2Ver121` followed by
  the package XORed with one byte (0xAC for 111, name-derived for 121). The key
  is recovered from the package magic `0x9E2A83C1`.
- `.dat` files in `system` use `Lineage2Ver413`: 128-byte RSA blocks (public
  exponent 0x1d, modulus in `ue2::dat`), each holding a length byte and
  payload, then zlib. Strings are indices into `L2GameDataName.dat`. Layouts
  match the L2ClientDat definitions: skillgrp "Salvation", npcgrp
  "GrandCrusade", skillname/animationcombo "Helios" (each file ends with a
  `SafePackage` marker). `tools/datparse.py` probes layouts against those XML
  definitions.
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
- Skill effects are `Emitter` subclasses in `system/lineageeffect*.u` and
  `lineageskilleffect.u` (e.g. `el_wind_strike_ca/fl/ta` = cast/flight/target).
  Their SpriteEmitter/MeshEmitter subobjects hold tagged properties; non-native
  structs (`Range`, `RangeVector`, array elements) are nested tagged streams,
  native ones (`Vector`, `Color`) are raw. `game/src/effects.rs` plays sprite and
  mesh emitters (beams/trails are skipped).
- Skill visuals: `skillgrp` gives an animation combo code (`E`, `S`, `SMB01`;
  some single letters share a string slot shown as `L2Day - E`).
  `animationcombo` maps it to pawn variables (`CastMidAnimName`...), whose clip
  is `<base>[_<weapon>]_<race>` on the character. `lineageskilleffect.u`
  class `l2_<id>_skill` holds `CastingAction`/`ShotAction`/`ExplosionAction`
  arrays of `L2EffectEmitter` (EffectClass, AttachOn, offset, bSpawnOnTarget)
  and `FlyingTime`; defaults are found by scanning for a clean property stream.
- Effect draw styles: Darken = dst * (1 - src) is emitted as black with alpha =
  source brightness; Modulated approximated with multiply.
- Maps: actors with `RF_HasStack` start with a script state frame (node,
  state node, probe mask, latent action, offset if node) before properties.
  StaticMeshActor gives StaticMesh/Location/Rotation/DrawScale(3D).
  TerrainInfo: G16 heightmap, vertex = Location + ((i - w/2) * Sx,
  (j - h/2) * Sy, (h - 32768) * Sz / 256), layers = texture + alpha map +
  UScale/VScale. Tiles are 256 x 128 UU. The viewer bakes layers into one
  2048² texture per tile.
