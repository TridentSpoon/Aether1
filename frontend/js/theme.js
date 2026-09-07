/**
 * Which colour theme should be on screen right now?
 *
 * Three windows ask that question -- the HUD, the desktop sprite and the avatar workbench --
 * and before this file they each answered it themselves with `localStorage.getItem(...) ||
 * 'halcy'`. That is fine until the answer stops being a single lookup, which is what these
 * two rules do:
 *
 *   1. Nothing chosen yet -> follow the operating system's light/dark setting. A companion
 *      that opens in blazing white on a machine set to dark mode looks broken before it has
 *      said a word, and the reverse is worse.
 *   2. Something chosen -> use it, and stop listening to the OS. An explicit choice is not a
 *      preference to be second-guessed the next time someone toggles Windows into night mode.
 *
 * Keeping both rules here means all three windows agree, and there is one place to look when
 * they don't.
 */
(function (global) {
    'use strict';

    const STORAGE_KEY = 'aether_color_theme';
    const LIGHT = 'solar';
    const DARK = 'eclipse';

    /* Solar and Eclipse shipped first as corporate-light and corporate-dark. Anyone who
       picked one before the rename still has the old name saved, so it is translated on the
       way out rather than being treated as an unknown theme -- which would silently drop them
       back to the default and look like the app forgot. */
    const RENAMED = {
        'corporate-light': LIGHT,
        'corporate-dark': DARK
    };

    function canonical(name) {
        if (!name) return null;
        return RENAMED[name] || name;
    }

    function systemPrefersDark() {
        return !!(global.matchMedia && global.matchMedia('(prefers-color-scheme: dark)').matches);
    }

    function systemTheme() {
        return systemPrefersDark() ? DARK : LIGHT;
    }

    /* localStorage throws rather than returning null in a few real situations (Safari's
       private mode, a browser set to block site data, an iframe with storage partitioned
       off). None of them should take the window down over a colour, so they read as
       "nothing chosen" and the OS setting decides. */
    function saved() {
        try {
            return canonical(global.localStorage.getItem(STORAGE_KEY));
        } catch (err) {
            return null;
        }
    }

    function resolve() {
        return saved() || systemTheme();
    }

    function save(name) {
        try {
            global.localStorage.setItem(STORAGE_KEY, name);
        } catch (err) {
            /* Nothing to do -- the theme still applies for this session, it just will not
               survive a restart. Better than a thrown error over a colour. */
        }
    }

    /* Calls onChange when the OS flips between light and dark, but only while the choice is
       still the OS's to make. The guard is re-read on every event rather than captured once,
       so the moment someone picks a theme this goes quiet without needing to be unsubscribed. */
    function followSystem(onChange) {
        if (!global.matchMedia) return;
        const query = global.matchMedia('(prefers-color-scheme: dark)');
        const handler = function () {
            if (!saved()) onChange(systemTheme());
        };
        if (query.addEventListener) query.addEventListener('change', handler);
        else if (query.addListener) query.addListener(handler); // older WebKit
    }

    global.Aether1Theme = {
        STORAGE_KEY: STORAGE_KEY,
        LIGHT: LIGHT,
        DARK: DARK,
        canonical: canonical,
        systemTheme: systemTheme,
        saved: saved,
        resolve: resolve,
        save: save,
        followSystem: followSystem
    };
})(window);
