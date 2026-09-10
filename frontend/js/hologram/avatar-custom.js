/* The avatar you build yourself.
 *
 * One registered avatar, id "custom", assembled at build time from a *recipe*: a small
 * object saying which core, which inner ring, which outer ring, which effect, and how
 * it moves. The recipe lives in localStorage, written by the avatar workbench
 * (frontend/avatar-lab.html).
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
        size: 26,
        radius: 62,
        spin: 0.15,
        bob: 4,
    };

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

            const options = { size: recipe.size, radius: recipe.radius };
            const built = [
                parts.cores[recipe.core].build(api, options),
                parts.innerRings[recipe.innerRing].build(api, options),
                parts.outerRings[recipe.outerRing].build(api, options),
                parts.effects[recipe.effect].build(api, options),
            ];
            built.forEach((part) => group.add(part.object));

            return { group, parts: built, recipe };
        },

        applyPalette(model, palette) {
            model.parts.forEach((part) => part.applyPalette(palette));
        },

        animate(model, ctx) {
            model.parts.forEach((part) => part.animate(ctx));
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
