# L2 Survivor

Lineage II rebuilt in Rust (Bevy) as an offline game, using the original
client's models, textures and animations, with gamepad support.

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
Movement as in the client: left click on the ground runs there (a ring marks
the spot); WASD / left stick also move (camera-relative), Shift walks.
Camera: right mouse drag, Q/E or right stick, wheel / triggers zoom.

World data comes from the L2J Mobius Classic 3.0 datapack (`L2_DATAPACK`,
default `G:/visery/l2-datapack/classic`): NPC stats and spawns, class
templates, experience, server skills and class skill trees, items, shops,
teleports and the quest pages. What is playable:

- Combat: Tab / click / RB target, F / X auto-attack; skills sit on the 1–9
  hotbar (gamepad: LB + A/B/X/Y = 1–4, LT + A/B/X/Y = 5–8).
- Skills: as in Classic, the skill window (K / right stick press) learns the
  class tree's next levels for SP (plus spellbooks or Adena where the tree
  asks); only `autoGet` skills come by themselves. Clicking a learned active
  skill puts it on the hotbar or takes it off. `L2_SKILL_WINDOW=1` opens it. Monsters and bosses use
  the offensive skills of their datapack `skillList`.
- Items: starting kit, equipment stats; monster drops fall on the ground with
  the client's drop models (click one or press Z / gamepad Y to pick up;
  `L2_AUTOLOOT=1` loots directly). Inventory on I / gamepad Select (Enter / A
  equips or uses; on soulshots / spiritshots it toggles auto-use, which
  doubles attack or spell damage per shot; the Classic universal shots fit
  any weapon grade).
- V sits down / stands up: 1.5x HP/MP regeneration.
- Formulas follow L2J (`game/src/formulas.rs`): stat bonuses from
  `stats/statBonus.xml` and the level modifier for P.Atk (STR), M.Atk (INT²),
  M.Def (MEN), HP / MP (CON / MEN), attack speed and critical rate (DEX),
  casting speed (WIT); armor replaces the bare slot's base defence;
  accuracy vs evasion decides misses; weapon random damage, x2 criticals.
  Monsters get the same functions from their datapack stats. Alt+T shows the
  status window (`L2_STATUS=1`).
- Map: M (gamepad left stick press) opens the world map from the client's
  `Minimap.worldmapN` art with the hero's position (a radar of the same map
  is shown only without the client interface). Equipped armor and weapons
  change the hero's model (client `armorgrp` / `weapongrp`), weapons switch
  the animation stance. Scrolls of escape / teleport and potions work.
- NPCs: F / X or a second click on a friendly NPC walks up and opens its
  dialog (datapack HTML). Links by mouse or Up/Down + Enter / A; Esc / B
  closes. Merchants buy and sell, multisell exchanges, gatekeepers teleport
  (free up to level 40).
- Quests: all 41 quests of the datapack are ported to Rust (`game/src/quests`),
  including the tutorial, the newbie chains of every race and Saga of Legend.
  Started quests are tracked on the left.
- Classes: transfer at 20, 40 and 76 (the third needs Saga of Legend) from a
  window that opens by itself (C / D-pad right reopens).
- Bosses: raid bosses with their escorts, grand bosses in their lairs (Baium
  wakes from his statue). Their HP and attack are scaled for one player:
  `L2_BOSS_HP` (default 0.05) and `L2_BOSS_ATK` (0.6).
- Saving: every minute, on F5 and on exit to `saves/hero.json` (`L2_SAVE`);
  C / gamepad Y on the character screen continues.
- Interface: the windows come from the client's own `system/interface.xdat`
  (layout, textures, Split9 frames, default positions) with labels from
  `sysstring-ru.dat` (`game/src/l2ui*.rs`): status, target, chat and shortcut
  bar; the inventory (I) with the classic equipment panel and item grid
  (click to equip / use); the character status (Alt+T); the skill window (K;
  hover shows a skill, click learns it or puts it on the bar); NPC dialogs in
  the NPCDialogWnd frame; the map (M) in MinimapWnd with wheel zoom; the
  restart menu on death; the system menu (Alt+X, "Quit" saves and exits);
  the quest window (Alt+U); the main menu bar (bottom right, the client's
  `Icon_DF_MenuWnd_*` buttons); class change in a window drawn with the
  client's frame art. Windows can be dragged and keep their place.
  Without `interface.xdat` the game falls back to plain panels.

Test helpers: `L2_INVENTORY=1`, `L2_AUTOFIGHT=1`, `L2_AUTOCAST=1`,
`L2_DEBUG_NPC=1`, `L2_START="x,y,z"`, `L2_LEVEL=<n>`, `L2_CLASS_PICK=<i>`,
`L2_TALK=<npc id>` with `L2_BYPASS="cmd|cmd"`, `L2_CONTINUE=1`,
`L2_NO_TUTORIAL=1`, `L2_AUTOSAVE_SECS=<s>`, `L2_MOVE_TO=dx,dz`, `L2_AUTOSHOTS=1`,
`L2_SIT=1`, `L2_GROUNDLOOT=1` (with `L2_AUTOFIGHT`), `L2_STATUS=1`, `L2_MAP=1`,
`L2_SYSMENU=1`, `L2_DIE=1` (dies once after 8 s).

Monsters behave like on the server: only those flagged aggressive in the
datapack attack on sight, the rest stroll around their spawn point until
hit. On death a window offers "To Village" (Enter / A), which restarts at
the nearest town from `mapregion/`.

Environment variables:

- `L2_CLIENT_DIR` — client root.
- `L2_GROUND=<package>:<texture>` — ground texture, e.g.
  `ertheia_v_t:ertheia_a_ground00` (default).
- `L2_WORLD=<tile>[,<tile>...]` — world viewer instead of the game, e.g.
  `20_21` (Dion / Cruma). WASD/left stick move, Q/E or triggers down/up,
  arrows/right stick look, Shift/A faster.
- `L2_CLASS=<prefix>` — skip the character screen, e.g. `FElf`, `MKamael`.
- `L2_LIST_SKILLS=<file>` — write the playable skill table (id, name,
  animations, effects) and continue.
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
