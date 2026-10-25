/* The avatar you build yourself.
 *
 * One registered avatar, id "custom", assembled at build time from a *recipe*: a small
 * object saying which core, which inner ring, which outer ring, which effect, which face
 * -- eyes, brows and a mouth, each picked separately -- and how it moves. The recipe
 * lives in localStorage, written by the avatar workbench (frontend/avatar-lab.html).
 *
 * The recipe is the thing that gets shared, not the code. That is deliberate -- a
 * recipe is a few lines of JSON that cannot do anything, so passing one to someone is
 * safe in a way that passing a .js file is not. The workbench can also export a recipe
 * as a standalone avatar file for people who want one; see its Export button.
 *
 * Loaded after parts.js and before app.js. If no recipe has been saved, the defaults
 * below are what you get -- an avatar that exists on first look rather than an empty
 * viewport that leaves you wondering whether it is broken.
 */
(function () {
    'use strict';

    const RECIPE_KEY = 'aether_custom_avatar';

    /* The recipe the avatar currently on screen was built from. */
    let builtRecipe = null;

    const DEFAULT_RECIPE = {
        core: 'crystal',
        innerRing: 'equalizerRing',
        outerRing: 'rings',
        effect: 'none',
        eyes: 'none',
        brows: 'none',
        mouth: 'none',
        size: 26,
        radius: 62,
        spin: 0.15,
        bob: 4,
    };

    /* The three face tiers, and which collection in the kit each recipe field names. The
       field is singular where the kit's collection is plural, which is the convention the
       first four tiers already set (innerRing names innerRings). Stated once here rather
       than spelled out in normalise, build and the workbench, so adding a fourth face
       tier later is one line in one place.

       All three default to 'none'. A face is something you choose; an avatar that grew
       eyes because its owner upgraded would be a fright. */
    const FACE_TIERS = [
        { field: 'eyes', kinds: 'eyes', label: 'Eyes' },
        { field: 'brows', kinds: 'brows', label: 'Brows' },
        { field: 'mouth', kinds: 'mouths', label: 'Mouth' },
    ];

    /* Anything missing, misspelled or of the wrong type falls back to the default for
       that one field, so a recipe written by hand -- or by an older version of the
       workbench -- still produces an avatar rather than an error. A recipe saved before
       the four-tier split (core/body/equaliser) still lands somewhere sane: its body and
       equaliser choices carry over to innerRing/outerRing where the ids still exist,
       instead of silently reverting to the defaults. */
    function normalise(raw) {
        const recipe = Object.assign({}, DEFAULT_RECIPE, raw && typeof raw === 'object' ? raw : {});
        const parts = window.AvatarParts;
        if (raw && typeof raw === 'object') {
            if (recipe.innerRing === undefined && raw.body !== undefined) recipe.innerRing = raw.body;
            if (recipe.outerRing === undefined && raw.equaliser !== undefined) recipe.outerRing = raw.equaliser;
        }
        if (!parts.cores[recipe.core]) recipe.core = DEFAULT_RECIPE.core;
        if (!parts.innerRings[recipe.innerRing]) recipe.innerRing = DEFAULT_RECIPE.innerRing;
        if (!parts.outerRings[recipe.outerRing]) recipe.outerRing = DEFAULT_RECIPE.outerRing;
        if (!parts.effects[recipe.effect]) recipe.effect = DEFAULT_RECIPE.effect;
        FACE_TIERS.forEach(({ field, kinds }) => {
            if (!parts[kinds][recipe[field]]) recipe[field] = DEFAULT_RECIPE[field];
        });
        const number = (value, fallback, min, max) =>
            (Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback);
        recipe.size = number(recipe.size, DEFAULT_RECIPE.size, 4, 70);
        recipe.radius = number(recipe.radius, DEFAULT_RECIPE.radius, 20, 110);
        recipe.spin = number(recipe.spin, DEFAULT_RECIPE.spin, -1.5, 1.5);
        recipe.bob = number(recipe.bob, DEFAULT_RECIPE.bob, 0, 25);
        return recipe;
    }

    function loadRecipe() {
        /* The workbench sets this directly so it can preview a recipe without saving
           it -- you should be able to try something on before committing to it. */
        if (window.AETHER_CUSTOM_AVATAR_RECIPE) return normalise(window.AETHER_CUSTOM_AVATAR_RECIPE);
        try {
            return normalise(JSON.parse(localStorage.getItem(RECIPE_KEY) || 'null'));
        } catch (err) {
            console.warn('The saved custom avatar could not be read; using the default:', err);
            return normalise(null);
        }
    }

    HologramAvatar.registerAvatar({
        id: 'custom',
        label: 'Your own avatar',

        build(api) {
            const recipe = loadRecipe();
            /* Remembered so anything holding this engine can tell whether what is on
               screen still matches what is saved -- see isStale() below. */
            builtRecipe = recipe;
            const parts = window.AvatarParts;
            const group = new THREE.Group();

            /* A core that draws its own face needs telling, or the one it drew and the
               one picked here both appear. Only facetedHeadCore does at the moment, and
               it reads this; see the comment on its faceGroup. */
            const faceSupplied = FACE_TIERS.some(({ field }) => recipe[field] !== 'none');
            const options = { size: recipe.size, radius: recipe.radius, faceSupplied };
            const built = [
                parts.cores[recipe.core].build(api, options),
                parts.innerRings[recipe.innerRing].build(api, options),
                parts.outerRings[recipe.outerRing].build(api, options),
                parts.effects[recipe.effect].build(api, options),
            ];
            // Tiers 1-3 (core, inner ring, outer ring) each get their own wrapper so the
            // manual zoom slider (ctx.zoom, see setZoom in core.js) can scale them without
            // touching the part's own animate() -- which may already be setting a scale on
            // its object directly (an audio-reactive pulse, say) and would otherwise fight
            // a scale written to that same object from outside. Tier 4 (the effect/
            // background) is added straight to group, unwrapped, so it never zooms with the
            // avatar -- exactly the split the workbench's four tiers already describe.
            const zoomWraps = built.slice(0, 3).map((part) => {
                const wrap = new THREE.Group();
                wrap.add(part.object);
                group.add(wrap);
                return wrap;
            });
            group.add(built[3].object);

            /* The face goes inside the core's wrapper, so it zooms and leans with the head
               it belongs to and draws after it. The parts themselves only know x and y:
               one group carries all three forward to where the front of this particular
               core is, which is what a head's faceZ says. A core that does not say -- every
               core written before the face tiers existed -- gets a plane just outside a
               sphere of its size, which is the right guess for most of them and is why the
               older cores can wear a face at all. */
            const face = new THREE.Group();
            face.position.z = Number.isFinite(built[0].faceZ) ? built[0].faceZ : recipe.size * 0.9;
            zoomWraps[0].add(face);
            FACE_TIERS.forEach(({ field, kinds }) => {
                const part = parts[kinds][recipe[field]].build(api, options);
                face.add(part.object);
                /* Appended, so built[0..3] stay the four tiers the zoom wrapping above
                   indexes by position, while applyPalette and animate reach all of them. */
                built.push(part);
            });

            return { group, parts: built, zoomWraps, face, recipe };
        },

        applyPalette(model, palette) {
            model.parts.forEach((part) => part.applyPalette(palette));
        },

        animate(model, ctx) {
            model.parts.forEach((part) => part.animate(ctx));
            /* Tier 2 is the listening tier.

               The other three states carry themselves. Speaking is the equaliser opening up,
               thinking has its own treatments, and idle is the core's resting animation --
               all of which the parts handle, and most of which are driven by ctx.audio. That
               is why listening was the one that went missing: it is the only state with no
               sound to react to. The microphone being open makes no noise, so a vocabulary
               built on reacting to noise has nothing to say about it, and nothing in the
               seventeen cores or the ten inner rings says anything.

               So it is said here, once, on the wrapper rather than inside a part. Each tier
               1-3 already has its own wrapper so the zoom slider can scale it without
               fighting a scale the part's own animate() may be writing (see build above) --
               which makes the wrapper the one place a tier-wide behaviour can live without
               touching ten parts or overriding any of them. Every inner ring gains this, a
               hand-written one included, and a part that later wants its own richer
               listening look can still have it: this scales the wrapper, that scales itself,
               and the two compose rather than collide.

               A slow breath rather than a flourish. It has to be legible across the room on
               the fullscreen face and bearable for the minute someone spends deciding what
               to say, and anything sharper reads as the avatar being agitated at you. The
               rhythm is self-driven because there is nothing to drive it with -- which is
               the honest signal here: it is not reacting to you yet, it is waiting. */
            const LISTENING_BREATH_HZ = 0.55;   // one breath every ~1.8s
            const LISTENING_BREATH_DEPTH = 0.16; // +/- this much of the tier's size
            const LISTENING_LEAN = 14;           // world units toward the viewer
            /* Two channels rather than one, and the reason is measured: a breath alone, at
               the depth that still looked calm, did not rise above the amount this avatar
               changes just sitting there -- 9.0% of pixels against a 9.2% idle floor. One
               subtle thing on top of a busy thing is not a signal. The lean is what makes
               it one: scale shares an axis with the equaliser's own radial motion and gets
               lost in it, where moving toward the viewer does not, and "leaning in" is what
               listening looks like anyway.

               Both numbers are named so they can be argued with. Depth and lean are the two
               knobs; the rate is slow on purpose. */
            const breathing = ctx.state === 'LISTENING'
                ? Math.sin(ctx.time * LISTENING_BREATH_HZ * Math.PI * 2)
                : null;
            model.zoomWraps.forEach((wrap, tier) => {
                const isListeningTier = tier === 1 && breathing != null;
                wrap.scale.setScalar(ctx.zoom * (isListeningTier ? 1 + breathing * LISTENING_BREATH_DEPTH : 1));
                wrap.position.z = isListeningTier ? LISTENING_LEAN * (0.5 + breathing * 0.5) : 0;
            });
            const { spin, bob } = model.recipe;
            model.group.rotation.y += spin * 0.01;
            model.group.position.y = Math.sin(ctx.time * 0.8) * bob;
        },
    });

    /* Exposed so the workbench and anything else can read and write the recipe without
       knowing where it is kept. */
    window.CustomAvatarRecipe = {
        key: RECIPE_KEY,
        defaults: DEFAULT_RECIPE,
        faceTiers: FACE_TIERS,
        normalise,
        load: loadRecipe,
        save(recipe) {
            localStorage.setItem(RECIPE_KEY, JSON.stringify(normalise(recipe)));
        },
        clear() {
            localStorage.removeItem(RECIPE_KEY);
        },
        /* True when the saved recipe has moved on from the one that was built -- which is
           what happens the moment someone presses Save in the workbench while the HUD is
           open. The HUD watches for this; without it, saving appears to do nothing until
           the whole app is restarted. */
        isStale() {
            if (!builtRecipe) return true;
            return JSON.stringify(loadRecipe()) !== JSON.stringify(builtRecipe);
        },
    };
})();
