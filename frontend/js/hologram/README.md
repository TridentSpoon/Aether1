# The avatar

Everything that draws the holographic avatar lives in this folder. Nothing else in
Aether1 knows how it works, and it knows nothing about chat, memory, tools or model
providers. That separation is the point: the avatar can be worked on, replaced or
rewritten without touching the rest of the app.

## The contract

The rest of Aether1 talks to the avatar through **six things, and only these six**:

```js
const hologram = new HologramAvatar('hologram-viewport'); // build it into a container
hologram.setAvatar('halcy');        // which avatar is on screen
hologram.setColorTheme('nexus');    // which colour palette tints it
hologram.setState('THINKING');      // IDLE | LISTENING | THINKING | SPEAKING
hologram.updateAudioData(bins);     // 64 frequency bins from the voice, 0-255
hologram.setZoom(1.4);              // manual framing, 0.5-2.5x, independent of auto-fit
```

Two files use them: `js/app.js` (the HUD) and `js/sprite.js` (the desktop floater).

**This list changes only deliberately.** It is small enough to keep in your head, and
that is what makes the avatar a module rather than a tangle. Adding a seventh method is a
decision, not a convenience — if something new is needed, ask first whether it belongs
inside the avatar instead.

`dispose()` exists as a seventh method for one narrow case: something that builds more
than one engine in a page. The HUD builds exactly one and keeps it; the workbench
rebuilds on every change, and without `dispose()` the old render loops pile up.

`setZoom(scale)` is a deliberate manual override, not an auto-fit setting: auto-fit (see
`applyContentFit` in `core.js`) exists purely to keep an avatar's own resting geometry from
clipping the frame, and widens the camera's FOV to whatever that takes; `setZoom` is
someone picking a size by hand on top of that, and is never overridden by it. It scales
every avatar uniformly except the custom one's tier-4 effect/background layer, which stays
outside its reach on purpose -- see `avatar-custom.js`.

## The files

| File | What it is |
| --- | --- |
| `core.js` | The engine: the class, the scene, the camera, avatar and theme selection, the plug-in registry. |
| `animate.js` | The per-frame loop, and one `animateX()` per built-in avatar. |
| `geometry-helpers.js` | Shared texture and shape makers (glows, rings, hexagons). |
| `palettes.js` | The six colour themes. |
| `avatar-*.js` | One built-in avatar each, hand-modelled. |
| `parts.js` | A kit of cores, bodies and equalisers to assemble an avatar from. |
| `avatar-custom.js` | The avatar built from a saved recipe — what the workbench produces. |
| `avatar-template.js` | Not loaded. A working example to copy when writing your own. |
| `avatar-loader.js` | Which avatar lives in which file, and fetching it when it is picked. |

Load order matters and is fixed in `index.html`: `core.js` defines the class, everything
else attaches to it, and `app.js` runs last.

The `avatar-*.js` files are the exception. They attach to the class the same way, but they
are not in the page at launch: `avatar-loader.js` names them, and each one is fetched the
first time its avatar is actually asked for. Loading all nineteen to show one cost about
195 KB and eighteen file loads at every startup, for avatars most people never pick. The
workbench is the one page that still asks for all of them at once, because listing every
avatar is its whole job.

## Two ways to make an avatar

### 1. Build one from parts, no code

From the HUD: pick **✨ Your own** in the avatar row and press **✎ Customise** next to
it, or open Settings and use **Design your own avatar**. Either opens the workbench in
its own window. (You can also open `frontend/avatar-lab.html` directly in a browser --
it needs nothing else running.)

Choose four tiers -- a core (the thing at the middle), an inner ring (wrapping close
around it), an outer ring (further out, toward the edge) and an effect (an ambient layer
or background) -- set the sizes and the motion, and press **Use this in Aether1**. The
HUD picks it up immediately: the two windows share the saved recipe, so an open HUD
updates the moment you save, with no reload.

pieces of the hand-built avatars. The rule is opt-out, not opt-in: every avatar's
components are available to the kit -- and so to Genesis and the custom-avatar builder
-- by default, including avatars still behind a Trace Protocol unlock. They stay out
only when a reason is written down for it. The current exceptions are the Nexus /
Nexus Sent, A1, and White Rabbit, whose designs stay theirs alone. (`avatar-senti.js` is
the Nexus Sent avatar under a different filename -- id `senti`, label "Nexus Sent" -- so
it falls under this same exception, not a separate avatar left out by mistake. White
Rabbit's pieces were in the kit until they were withdrawn at the owner's request on
2026-09-20; the avatar itself still ships.) See
`js/hologram/parts.js` for the full catalogue.

What is saved is a *recipe* — a few lines of settings, not code. A recipe can be pasted
to someone else safely, because settings cannot do anything.

### 2. Write one

Copy `avatar-template.js` and change it. An avatar is an object with an id and three
functions, registered before the engine starts:

```js
HologramAvatar.registerAvatar({
    id: 'my-avatar',
    label: 'My avatar',

    // Optional, and unrelated to build()'s own `group` below -- this one is a family
    // name for the picker to cluster related avatars under, e.g. 'The Umbrals' for the
    // A.R.X. line (see index.html's avatar-menu and js/avatar-lab.js's BUILT_IN, which
    // group the same way). Leave it out for an avatar that stands on its own.
    group: 'My family',

    // Runs once. Return an object with a `group`; anything else you put on it comes
    // back to you as `model` below.
    build(api) { … return { group, myMaterial }; },

    // Runs about sixty times a second. Change things; do not build things.
    animate(model, ctx) { … },

    // Runs when the colour theme changes, and once at startup.
    applyPalette(model, palette) { … },
});
```

`api` carries `THREE`, the current `palette`, and `helpers` (the same texture makers the
built-in avatars use). `ctx` carries `time`, `audio` (0–1 loudness), `audioData` (the 64
bins), `click` (a brief pulse when the avatar is clicked), `state`, `palette`, `zoom` (the
current manual zoom scale, see `setZoom`) and `viewRotationY` (the scene's current drag-yaw).
Most avatars can ignore the last two -- their group is scaled by `setZoom` for free, and
spinning along with a drag is the normal, wanted behaviour. They're there for the rare
avatar that wants to opt out of one: Operator counters `viewRotationY` every frame so its
depth tunnel stays camera-locked instead of spinning with the hologram, and the custom
avatar reads `zoom` directly because it must scale its own tiers 1-3 but never its tier-4
effect layer.

A broken avatar is reported to the console and skipped — a mistake in one avatar file
does not take the HUD down with it.

### Loading yours

- **While working on it:** the workbench's *Load someone's avatar file* button. Reload
  as often as you like; re-registering the same id replaces it.
- **To install it:** put the file in this folder, add a line for it to the `AVATARS` list
  in `avatar-loader.js` (`'your-id': { file: 'avatar-your-id.js', v: 1 }`), and add a picker
  button with `data-avatar-val="your-id"`. Bump that `v` whenever you change the file, so
  browsers fetch the new one rather than the copy they already have.
- **Add it to the kit too.** Per the opt-out rule above, adapt the new avatar's cores,
  inner rings, outer rings and effects into `parts.js` as part of the same change --
  a fresh, simplified build using `api.helpers`, not the original file's code (see the
  note at the top of `parts.js`) -- so it's available to Genesis and the custom-avatar
  builder from day one. Skip this only if the avatar is meant to stay exclusive, and
  say so in `parts.js`'s exception list.

**An avatar file is a program.** It runs with the same access as the rest of the page.
Load files you wrote or trust — the same care you would give any script. A recipe is
different: it is settings, and it cannot run.

## Conventions worth keeping

These are not enforced, but every built-in avatar follows them and it is why six very
different shapes still look like one family:

- **The core stays obsidian.** Every avatar's centre is a fixed dark material, not
  theme-tinted. It gives the avatar something solid at its heart in every theme.
- **Hot accents stay fixed too** — R.E.D. 9000's lens, the Nexus's eyes. An avatar that
  retints everything stops looking like itself.
- **`MeshBasicMaterial` unless you mean it.** Basic ignores lights, which is right for
  something reading as drawn light. The obsidian cores use Phong precisely because they
  are meant to look lit.
- **Drive motion from `ctx.time`, never a frame count.** Frames are not evenly spaced,
  and an avatar that counts them runs at a different speed on a different machine.
- **Build in `build()`, not in `animate()`.** Making geometry every frame is what makes
  an avatar stutter.

## Working on the avatar without the rest of the app

`frontend/avatar-lab.html` loads the avatar engine and nothing else — no Rust build, no
backend, no model server, no network. It fakes everything the HUD would supply:

- every avatar and colour theme in a dropdown,
- the four states as buttons,
- a **voice simulator** producing the same 64 frequency bins a real voice would, in
  speech, music or swept-tone shapes, so an equaliser can be built in silence.

Open it directly from disk, or at `/avatar-lab.html` when Aether1 is running with
`--serve`.
