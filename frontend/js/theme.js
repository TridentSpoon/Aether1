/**
 * The theme engine.
 *
 * A theme used to be one word -- "halcy", "night-city" -- naming a block of hand-written CSS.
 * Eight words, eight blocks, and no way to have a ninth without writing a ninth block. It is
 * now two independent choices instead:
 *
 *   mode    solar | eclipse | cyberpunk
 *           Decides the chrome. Solar and Eclipse are flat window shells modelled on Windows
 *           11; Cyberpunk keeps the scanlines, corner brackets, grid backdrop and neon glow.
 *           Chosen in CSS, via the data-theme attribute on <html>.
 *
 *   colours background, main, highlight
 *           Three values, set as custom properties on <html>, from which every other colour
 *           the stylesheet reads is derived here in one place. The presets in palettes.js are
 *           just named bundles of them, so anything a preset can do the three colour pickers
 *           in Settings can do too -- including to Solar and Eclipse.
 *
 * Each mode remembers its own colours, so switching to Solar to read something in daylight and
 * back to Cyberpunk afterwards does not cost you the accent you had picked.
 *
 * Two rules about where the first answer comes from:
 *
 *   1. Nothing chosen yet -> follow the operating system's light/dark setting (Solar or
 *      Eclipse). A companion that opens in blazing white on a machine set to dark looks broken
 *      before it has said a word, and the reverse is worse.
 *   2. Something chosen -> use it, and stop listening to the OS. An explicit choice is not a
 *      preference to be second-guessed the next time someone toggles Windows into night mode.
 *
 * The HUD, the desktop sprite and the avatar workbench all ask this file rather than each
 * keeping their own copy of the answer.
 */
(function (global) {
    'use strict';

    const STORAGE_KEY = 'aether_theme';
    const LEGACY_KEY = 'aether_color_theme';   // the single-word themes this replaced
    const MODES = ['solar', 'eclipse', 'cyberpunk'];

    const MODE_LABELS = { solar: 'Solar', eclipse: 'Eclipse', cyberpunk: 'Cyberpunk' };
    const DEFAULT_PRESET = { solar: 'solar', eclipse: 'eclipse', cyberpunk: 'halcy' };

    // ---- colour arithmetic --------------------------------------------------------------
    // Everything below works in plain sRGB. Not because sRGB is the right space for mixing
    // colours -- it is not -- but because these are decorative tints and borders, and the
    // difference is invisible at the alphas involved.

    function toRgb(hex) {
        const clean = String(hex).trim().replace(/^#/, '');
        const full = clean.length === 3 ? clean.split('').map((c) => c + c).join('') : clean;
        const n = parseInt(full, 16);
        if (full.length !== 6 || isNaN(n)) return null;
        return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
    }

    function toHex(rgb) {
        const part = (v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0');
        return '#' + part(rgb.r) + part(rgb.g) + part(rgb.b);
    }

    /* Is this hex a colour we can use? Anything that fails becomes a fallback rather than an
       exception: a bad value in localStorage, or typed into a colour field, should not be able
       to take the window down. */
    function isColour(hex) {
        return toRgb(hex) !== null;
    }

    function mix(a, b, amount) {
        const x = toRgb(a);
        const y = toRgb(b);
        return toHex({ r: x.r + (y.r - x.r) * amount, g: x.g + (y.g - x.g) * amount, b: x.b + (y.b - x.b) * amount });
    }

    function rgba(hex, alpha) {
        const c = toRgb(hex);
        return `rgba(${c.r}, ${c.g}, ${c.b}, ${alpha})`;
    }

    /* Perceived brightness, 0..1 (Rec. 601 coefficients -- the eye is far more sensitive to
       green than to blue). Every light-or-dark decision below asks this rather than asking
       which mode is active, so a Solar with a dark background still gets light text. */
    function luminance(hex) {
        const c = toRgb(hex);
        return (c.r * 0.299 + c.g * 0.587 + c.b * 0.114) / 255;
    }

    function isLight(hex) {
        return luminance(hex) > 0.5;
    }

    /* Cyberpunk's panels are the ground scaled up rather than mixed toward grey, which is what
       keeps a near-black with a blue cast reading as blue rather than drifting to slate. The
       added constant is a floor, so a background of pure black still produces panels you can
       see the edges of. */
    function raise(hex, factor, floor) {
        const c = toRgb(hex);
        return toHex({ r: c.r * factor + floor, g: c.g * factor + floor, b: c.b * factor + floor });
    }

    // ---- presets ------------------------------------------------------------------------

    function presets() {
        return (global.THEME_PRESETS || []).slice();
    }

    function preset(id) {
        return (global.THEME_PRESETS_BY_ID || {})[id] || null;
    }

    function presetColours(id) {
        const p = preset(id);
        if (!p) return null;
        return { background: p.background, main: p.main, highlight: p.highlight, preset: p.id };
    }

    function defaultColours(mode) {
        return presetColours(DEFAULT_PRESET[mode] || 'halcy');
    }

    /* The tone between main and highlight. Presets spell theirs out because they were tuned by
       eye; a pair of hand-picked colours gets the midpoint, which is what "between" means when
       nobody has an opinion. */
    function midToneFor(colours) {
        const p = preset(colours.preset);
        if (p && p.main === colours.main && p.highlight === colours.highlight) return p.mid;
        return mix(colours.main, colours.highlight, 0.5);
    }

    /* The avatar renderer's palette shape -- see themePaletteFrom in palettes.js, which builds
       the same thing for the presets. Third-party avatar files read hex/hex2/hex3/r/g/b, so
       colours picked by hand have to arrive in exactly that shape. */
    function paletteFor(colours) {
        return global.themePaletteFrom(colours.main, midToneFor(colours), colours.highlight);
    }

    // ---- deriving the stylesheet's variables ---------------------------------------------

    /* One background, one main, one highlight in; every colour the stylesheet reads out. This
       is the whole reason the eight CSS blocks could go: they were eight hand-written answers
       to this function. */
    function variablesFor(mode, colours) {
        const { background, main, highlight } = colours;
        const mid = midToneFor(colours);
        const light = isLight(background);
        const ink = light ? mix('#000000', background, 0.10) : mix('#ffffff', main, 0.14);

        const vars = {
            '--bg-core': background,
            '--border-neon-bright': main,
            '--neon-cyan': main,
            '--neon-blue': mid,
            '--neon-purple': highlight,
            '--text-main': ink,
            '--text-dim': mix(ink, background, light ? 0.62 : 0.45)
        };

        if (mode === 'cyberpunk') {
            /* Lit chrome: panels glow faintly with the ground's own hue, borders are the accent
               at low alpha, and the glow variables are real neon -- a wide soft halo, not a
               drop shadow. */
            Object.assign(vars, {
                '--bg-panel': rgba(raise(background, 2.0, 4), 0.8),
                '--bg-panel-hover': rgba(raise(background, 3.3, 10), 0.9),
                '--border-neon': rgba(main, 0.4),
                '--neon-green': '#00ffaa',
                '--neon-amber': '#ffaa00',
                '--neon-red': '#ff3366',
                '--glow-cyan': `0 0 15px ${rgba(main, 0.55)}, 0 0 30px ${rgba(main, 0.22)}`,
                '--glow-purple': `0 0 15px ${rgba(highlight, 0.55)}, 0 0 30px ${rgba(highlight, 0.22)}`,
                '--glow-green': `0 0 15px ${rgba(mid, 0.55)}, 0 0 30px ${rgba(mid, 0.22)}`,
                '--viewport-bg': '#000000',
                '--viewport-glow': rgba(main, 0.5),
                '--scanline-color': rgba(main, 0.03),
                '--bg-grid-color': rgba(main, 0.035),
                '--bg-wash-main': rgba(main, 0.12),
                '--bg-wash-highlight': rgba(highlight, 0.07),
                '--tint-main': rgba(main, 0.08)
            });
        } else {
            /* Flat chrome: surfaces separate by lightness alone, borders are hairlines rather
               than lit edges, and "glow" is shallow elevation you would only notice if it were
               missing. Status colours are fixed per lightness instead of following the accent
               -- a red that means "critical" should not turn amber because someone picked an
               amber theme. */
            Object.assign(vars, {
                '--bg-panel': light ? rgba(mix(background, '#ffffff', 0.75), 0.9) : rgba(raise(background, 1.0, 11), 0.9),
                '--bg-panel-hover': light ? '#ffffff' : raise(background, 1.0, 24),
                '--border-neon': light ? 'rgba(0, 0, 0, 0.10)' : 'rgba(255, 255, 255, 0.09)',
                '--neon-green': light ? '#0f7b0f' : '#6ccb5f',
                '--neon-amber': light ? '#9d5d00' : '#fce100',
                '--neon-red': light ? '#c42b1c' : '#ff99a4',
                '--glow-cyan': light ? '0 1px 2px rgba(0, 0, 0, 0.14)' : '0 1px 2px rgba(0, 0, 0, 0.35)',
                '--glow-purple': light ? '0 1px 2px rgba(0, 0, 0, 0.14)' : '0 1px 2px rgba(0, 0, 0, 0.35)',
                '--glow-green': light ? '0 1px 2px rgba(0, 0, 0, 0.14)' : '0 1px 2px rgba(0, 0, 0, 0.35)',
                /* The avatar sits in a dark bay whatever the shell around it is doing: a
                   hologram projected onto a white page reads as a picture of one. Not the
                   Cyberpunk void either -- a grey, so it is a screen inset into the window
                   rather than a hole cut in it. On a light shell that is a fixed dark grey;
                   on a dark one it is the ground itself taken down, so a custom background
                   still gets a bay that is recessed rather than one that fights it. */
                '--viewport-bg': light ? '#202020' : raise(background, 0.6, 0),
                '--viewport-glow': rgba(main, 0.22),
                '--scanline-color': 'transparent',
                '--bg-grid-color': 'transparent',
                '--bg-wash-main': 'transparent',
                '--bg-wash-highlight': 'transparent',
                '--tint-main': light ? rgba(main, 0.06) : rgba(main, 0.10)
            });
        }
        return vars;
    }

    /* Paints a theme onto a document: the mode as an attribute for the chrome rules, the
       derived colours as custom properties, and color-scheme so the browser's own furniture
       -- scrollbars, the dropdown a <select> opens -- matches instead of staying dark on a
       white page. Takes the document so the desktop sprite can paint its own window. */
    function paint(doc, mode, colours) {
        const root = doc.documentElement;
        root.setAttribute('data-theme', mode);
        const vars = variablesFor(mode, colours);
        Object.keys(vars).forEach((name) => root.style.setProperty(name, vars[name]));
        root.style.colorScheme = isLight(colours.background) ? 'light' : 'dark';
    }

    // ---- what is stored -----------------------------------------------------------------

    function blankState() {
        return { mode: null, colours: {} };
    }

    /* Reading is deliberately forgiving. Storage can hold anything -- a half-written state
       from an interrupted write, a hand-edited value, the previous format -- and none of it is
       worth a blank window, so every field is validated and anything unusable falls back. */
    function normalise(raw) {
        const state = blankState();
        if (!raw || typeof raw !== 'object') return state;
        if (MODES.indexOf(raw.mode) !== -1) state.mode = raw.mode;
        MODES.forEach((mode) => {
            const c = raw.colours && raw.colours[mode];
            if (!c || !isColour(c.background) || !isColour(c.main) || !isColour(c.highlight)) return;
            state.colours[mode] = {
                background: c.background,
                main: c.main,
                highlight: c.highlight,
                preset: preset(c.preset) ? c.preset : null
            };
        });
        return state;
    }

    /* localStorage throws rather than returning null in a few real situations (a browser set to
       block site data, storage partitioned off in an iframe, private mode on some builds).
       None of them should take a window down over a colour, so they read as "nothing saved"
       and the OS setting decides. */
    function readStored() {
        try {
            const stored = global.localStorage.getItem(STORAGE_KEY);
            if (stored) return normalise(JSON.parse(stored));
        } catch (err) {
            return blankState();
        }
        return migrateLegacy();
    }

    /* Before this, the whole theme was one word under a different key. Translating it means an
       upgrade keeps the look someone chose, instead of silently resetting to the default and
       looking like the app forgot. */
    function migrateLegacy() {
        const state = blankState();
        let old = null;
        try {
            old = global.localStorage.getItem(LEGACY_KEY);
        } catch (err) {
            return state;
        }
        if (!old) return state;
        const aliases = { 'corporate-light': 'solar', 'corporate-dark': 'eclipse', crimson: 'red', matrix: 'nexus' };
        const p = preset(aliases[old] || old);
        if (!p) return state;
        state.mode = p.mode;
        state.colours[p.mode] = presetColours(p.id);
        return state;
    }

    function write(state) {
        try {
            global.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
        } catch (err) {
            /* The theme still applies for this session; it just will not survive a restart.
               Better than throwing over a colour. */
        }
    }

    // ---- the current answer --------------------------------------------------------------

    function systemMode() {
        const dark = global.matchMedia && global.matchMedia('(prefers-color-scheme: dark)').matches;
        return dark ? 'eclipse' : 'solar';
    }

    /* True while the OS is still the authority -- nothing has been chosen in this browser. */
    function followingSystem() {
        return readStored().mode === null;
    }

    function current() {
        const state = readStored();
        const mode = state.mode || systemMode();
        return { mode: mode, colours: state.colours[mode] || defaultColours(mode) };
    }

    function apply(doc) {
        const now = current();
        paint(doc || global.document, now.mode, now.colours);
        return now;
    }

    /* Switching mode keeps whatever colours that mode was last wearing, and starts from its
       default preset if it has never been worn. Carrying colours *across* modes is what you
       must not do: Solar with Cyberpunk's near-black ground is not a light theme, it is a
       broken one. */
    function setMode(mode) {
        if (MODES.indexOf(mode) === -1) return current();
        const state = readStored();
        state.mode = mode;
        if (!state.colours[mode]) state.colours[mode] = defaultColours(mode);
        write(state);
        return current();
    }

    function setPreset(id) {
        const p = preset(id);
        if (!p) return current();
        const state = readStored();
        state.mode = p.mode;
        state.colours[p.mode] = presetColours(id);
        write(state);
        return current();
    }

    /* One colour at a time, which is how the pickers in Settings send them. The mode's other
       two are left exactly as they are -- that is what makes the background stable while an
       accent is being tried out. */
    function setColour(slot, hex) {
        if (['background', 'main', 'highlight'].indexOf(slot) === -1 || !isColour(hex)) return current();
        const now = current();
        const state = readStored();
        state.mode = now.mode;
        const colours = state.colours[now.mode] || defaultColours(now.mode);
        colours[slot] = toHex(toRgb(hex));
        colours.preset = null;   // hand-mixed now, so no preset should show as selected
        state.colours[now.mode] = colours;
        write(state);
        return current();
    }

    function resetColours() {
        const now = current();
        const state = readStored();
        state.mode = now.mode;
        state.colours[now.mode] = defaultColours(now.mode);
        write(state);
        return current();
    }

    /* Calls onChange when the OS flips between light and dark, but only while the choice is
       still the OS's to make. The guard is re-read on every event rather than captured once,
       so the moment someone picks a mode this goes quiet without needing to be unsubscribed. */
    function followSystem(onChange) {
        if (!global.matchMedia) return;
        const query = global.matchMedia('(prefers-color-scheme: dark)');
        const handler = function () {
            if (followingSystem()) onChange(current());
        };
        if (query.addEventListener) query.addEventListener('change', handler);
        else if (query.addListener) query.addListener(handler); // older WebKit
    }

    global.Aether1Theme = {
        MODES: MODES,
        MODE_LABELS: MODE_LABELS,
        STORAGE_KEY: STORAGE_KEY,
        presets: presets,
        preset: preset,
        paletteFor: paletteFor,
        variablesFor: variablesFor,
        paint: paint,
        apply: apply,
        current: current,
        followingSystem: followingSystem,
        systemMode: systemMode,
        setMode: setMode,
        setPreset: setPreset,
        setColour: setColour,
        resetColours: resetColours,
        followSystem: followSystem,
        isColour: isColour,
        isLight: isLight
    };
})(window);
