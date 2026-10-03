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
 *           just named bundles of them, so anything a preset can do the colour pickers in
 *           Settings can do too. How many of the three are yours depends on the mode -- see
 *           slotsFor below: all three in Cyberpunk, the two accents in Daylight and Midnight,
 *           whose ground is the light and the dark shell rather than a colour choice.
 *
 * Each mode remembers its own colours, so switching to Solar to read something in daylight and
 * back to Cyberpunk afterwards does not cost you the accent you had picked.
 *
 * Two rules about where the first answer comes from:
 *
 *   1. Nothing chosen yet -> follow the desktop. That means Daylight or Midnight to match the
 *      system's light/dark setting, wearing the system's own accent colour. A companion that
 *      opens in blazing white on a machine set to dark looks broken before it has said a word,
 *      and one that opens in a blue nobody picked looks like it was not paying attention.
 *   2. Something chosen -> use it, and stop listening to the desktop. An explicit choice is not
 *      a preference to be second-guessed the next time someone toggles Windows into night mode.
 *
 * Following is a state, not only a starting point: `setMode('system')` goes back to it, and the
 * three mode buttons plus that fourth choice are what Settings offers. While it is on, the
 * desktop is re-read when the window is shown again and whenever the system flips light/dark,
 * so changing the accent in Windows' settings and coming back shows the new colour.
 *
 * Where the desktop's answer comes from:
 *
 *   light/dark   `prefers-color-scheme` in the webview, overridden by the native probe
 *                (desktop_theme.rs) when it has an answer. On Windows and macOS the webview is
 *                told the truth and the two agree; on Linux WebKitGTK infers it from the GTK
 *                theme name, so the native read of the XDG appearance portal is the one to
 *                trust.
 *   accent       Only the native probe. There is no media query for the accent colour on any
 *                platform, so a window with no bridge to the native side (a page served to a
 *                plain browser with the HTTP API unreachable) keeps the designed accent, which
 *                is the right fallback rather than a failure.
 *
 * The desktop hands over one accent and the engine wants two, so the companion is derived --
 * see companionFor.
 *
 * The HUD, the desktop sprite and the avatar workbench all ask this file rather than each
 * keeping their own copy of the answer.
 */
(function (global) {
    'use strict';

    const STORAGE_KEY = 'aether_theme';
    const LEGACY_KEY = 'aether_color_theme';   // the single-word themes this replaced
    const MODES = ['solar', 'eclipse', 'cyberpunk'];

    /* Not a mode. It is the value the fourth button in the mode row sends to setMode, meaning
       "stop choosing and follow the desktop", and it is never stored and never painted -- what
       gets painted is whichever of the three the desktop is asking for. Kept out of MODES so
       that nothing iterating the real modes (the stored colour sets, normalise, the palette
       grid) has to learn about it. */
    const SYSTEM_MODE = 'system';

    /* The names shown to the operator. Internally the two flat modes are still solar and
       eclipse -- every stored state and every data-theme attribute uses those -- but the
       two presets that draw them were always called Daylight and Midnight, and that is
       what they are called out loud now, in one place rather than two. */
    const MODE_LABELS = { solar: 'Daylight', eclipse: 'Midnight', cyberpunk: 'Cyberpunk', system: 'Desktop' };
    const DEFAULT_PRESET = { solar: 'solar', eclipse: 'eclipse', cyberpunk: 'halcy' };

    /* What a mode lets you change.
     *
     * Cyberpunk is the mode that is *made* of its colours: the ground is part of the look, the
     * washes, grid and scanlines are drawn out of it, and a near-black with a blue cast is as
     * much the theme as the cyan on top of it. All three are yours there.
     *
     * Daylight and Midnight are not palettes, they are the light and the dark window shell --
     * a near-white page and a near-black one, drawn the way they were designed. Letting the
     * ground move in those two only ever produced the broken middle: a "light" theme on a grey
     * page, a Midnight that had drifted halfway to Daylight, and the light-or-dark decisions
     * below (which read the background rather than the mode) quietly flipping with it. So in
     * those two you pick the accents and the shell stays as drawn. Depth still shifts the
     * ground, because that is a bounded adjustment of the designed colour rather than a
     * replacement of it. */
    const COLOUR_SLOTS = ['background', 'main', 'highlight'];
    const ACCENT_SLOTS = ['main', 'highlight'];

    function slotsFor(mode) {
        return mode === 'cyberpunk' ? COLOUR_SLOTS.slice() : ACCENT_SLOTS.slice();
    }

    function groundIsFixed(mode) {
        return mode !== 'cyberpunk';
    }

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

/* The one above answers "does this look light or dark", which is a different question from
   "can this be read on that". WCAG's relative luminance is the second one: the same three
   channels, gamma-corrected first and weighted the way the standard weights them. It is
   only used for the contrast sum below -- every light-or-dark decision still asks
   luminance(), which is cheaper and is what the eye means by bright. */
    function relativeLuminance(hex) {
        const c = toRgb(hex);
        const f = [c.r, c.g, c.b].map((v) => {
            v /= 255;
            return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
        });
        return 0.2126 * f[0] + 0.7152 * f[1] + 0.0722 * f[2];
    }

    /* 1 is the same colour twice, 21 is black on white. WCAG asks 4.5 of body-sized text. */
    function contrast(a, b) {
        const l1 = relativeLuminance(a);
        const l2 = relativeLuminance(b);
        return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
    }

    /* WCAG asks 4.5:1 of body-sized text, and this aims a little past it. Almost no text in
       the HUD sits on the page itself -- it sits on a panel, which is a shade lifted off the
       ground, and measuring against the ground therefore flatters the answer by roughly a
       tenth. Cyberpunk's purple cleared 4.62 against its near-black ground and still only
       managed 4.35 on the panel in front of it. The margin is that gap, rounded up. */
    const ACCENT_CONTRAST = 5.0;

    /* An accent is chosen for how it looks next to the avatar, not for how it reads as a
       word. Solar's purple on its near-white page measures 4.47:1 and Cyberpunk's measures
       4.12:1 -- both a hair under the line, and both from presets that ship with the app, so
       a colour somebody mixes themselves can land anywhere. This walks the accent towards
       black on a light ground, or white on a dark one, in twentieths, and stops at the first
       step that reads. A colour that already reads is returned untouched, which is the usual
       case: nothing here repaints a palette that was fine. */
    function readable(colour, background, target) {
        if (contrast(colour, background) >= target) return colour;
        const towards = isLight(background) ? '#000000' : '#ffffff';
        for (let step = 1; step <= 20; step++) {
            const out = mix(colour, towards, step / 20);
            if (contrast(out, background) >= target) return out;
        }
        return towards;
    }

    /* Cyberpunk's panels are the ground scaled up rather than mixed toward grey, which is what
       keeps a near-black with a blue cast reading as blue rather than drifting to slate. The
       added constant is a floor, so a background of pure black still produces panels you can
       see the edges of. */
    function raise(hex, factor, floor) {
        const c = toRgb(hex);
        return toHex({ r: c.r * factor + floor, g: c.g * factor + floor, b: c.b * factor + floor });
    }

    /* ---- hue, for the one job that needs it ------------------------------------------
       Everything above works in RGB because mixing and fading do not need anything else. The
       companion accent does: it is the same colour family turned a little way round the wheel,
       and "a little way round the wheel" has no expression in RGB. */

    function rgbToHsl(hex) {
        const c = toRgb(hex);
        const r = c.r / 255, g = c.g / 255, b = c.b / 255;
        const max = Math.max(r, g, b), min = Math.min(r, g, b);
        const l = (max + min) / 2;
        if (max === min) return { h: 0, s: 0, l: l };   // grey has no hue to report
        const d = max - min;
        const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
        let h;
        if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
        else if (max === g) h = ((b - r) / d + 2) / 6;
        else h = ((r - g) / d + 4) / 6;
        return { h: h, s: s, l: l };
    }

    function hslToRgb(hsl) {
        const h = ((hsl.h % 1) + 1) % 1;
        const s = Math.max(0, Math.min(1, hsl.s));
        const l = Math.max(0, Math.min(1, hsl.l));
        if (s === 0) {
            const v = Math.round(l * 255);
            return toHex({ r: v, g: v, b: v });
        }
        const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
        const p = 2 * l - q;
        const channel = (t) => {
            t = ((t % 1) + 1) % 1;
            if (t < 1 / 6) return p + (q - p) * 6 * t;
            if (t < 1 / 2) return q;
            if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
            return p;
        };
        return toHex({ r: channel(h + 1 / 3) * 255, g: channel(h) * 255, b: channel(h - 1 / 3) * 255 });
    }

    /* The desktop publishes one accent colour. This engine is built on two -- a main and a
       highlight, used for different things all over the stylesheet, and blended into a third
       in between -- so the second one has to come from somewhere, and the only honest place is
       the first.

       The companion is the same hue, lifted and softened. That is to say: a tint of the
       accent, not a second colour.

       The first attempt here was not that. The two palettes drawn by hand for exactly these two
       modes separate their accents by hue --

         Daylight   #0067c0 -> #8764b8    +58 degrees, lightness .38 -> .56
         Midnight   #60cdff -> #b4a0ff    +55 degrees, lightness .69 -> .81

       -- so turning the accent 56 degrees reproduced both pairs closely, and looked right for
       every accent that happens to be blue. It is wrong as a general rule, and a desktop set to
       an orange accent is where that shows: 56 degrees off orange is yellow-green, so the HUD
       came out orange and olive. Both designed pairs start from a blue, where a rotation of that
       size stays inside the blue-violet family; from a warm hue the same rotation crosses into
       the cool half and reads as two colours that do not belong together.

       There is no rotation that is harmonious from every starting hue, and a table of per-hue
       exceptions would be a pile of taste with nothing behind it. Holding the hue has nothing to
       go wrong: an accent and a tint of it are the same colour, so the pair cannot clash whatever
       somebody's desktop is set to. It is also what the desktops themselves do -- Windows stores
       its accent as `AccentPalette`, eight shades of the one hue, and shows exactly that ramp in
       its own settings. Following the desktop's accent should mean following the desktop's idea
       of what goes with it.

       Two sets of numbers, chosen by the ground rather than by the mode -- the same way every
       other light-or-dark decision in this file is made. The light shell softens harder because
       a saturated tint on a near-white page is the one that glares.

       The cap is a floor in disguise. An accent that is already pale -- a light grey, a pastel --
       would lift past the page itself and leave every border and fill painted with it invisible,
       so the cap pulls it back down instead. Text is a separate matter and already handled:
       variablesFor runs both accents through readable().

       A grey accent -- Windows allows one, macOS calls it graphite -- comes out a lighter grey,
       which falls out of the rule rather than needing a case of its own. */
    const COMPANION = {
        light: { lift: 0.18, saturationScale: 0.80, cap: 0.74 },
        dark: { lift: 0.16, saturationScale: 0.90, cap: 0.86 }
    };

    function companionFor(accent, groundIsLight) {
        if (!isColour(accent)) return accent;
        const rule = groundIsLight ? COMPANION.light : COMPANION.dark;
        const hsl = rgbToHsl(accent);
        return hslToRgb({
            h: hsl.h,
            s: hsl.s * rule.saturationScale,
            l: Math.min(rule.cap, hsl.l + rule.lift)
        });
    }

    /* Drains the colour out of a hex without changing how bright it looks, by mixing it toward
       the grey of its own perceived luminance. Mixing toward a fixed mid-grey instead would
       darken pale colours and lighten deep ones on the way, so a slider that was meant to say
       "less shouty" would also be saying "different brightness". */
    function desaturate(hex, amount) {
        if (!(amount > 0)) return hex;
        const grey = Math.round(luminance(hex) * 255);
        return mix(hex, toHex({ r: grey, g: grey, b: grey }), Math.min(1, amount));
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
        const c = derive(colours);
        return global.themePaletteFrom(c.main, c.mid, c.highlight);
    }

    // ---- the two tone controls ------------------------------------------------------------
    /* Saturation and depth are separate axes, and the feedback that asked for them named both:
       "the colours are too bright" is chroma, and "Solar and Eclipse are not dark enough" is
       lightness. One slider cannot answer both -- draining the colour out of a background does
       not move it toward black, it moves it toward grey at the same brightness.

       Both are stored per mode alongside that mode's three colours, and both default to
       "exactly as the preset was drawn", so an install that never touches them looks today
       the way it looked yesterday. */

    const SATURATION_DEFAULT = 100;   // 0 = greyscale, 100 = the colours as picked
    /* Depth runs both ways from 0, and stops at 40 rather than 100 on purpose. isLight() is
       measured from the background rather than declared by the mode, so a background dragged
       far enough crosses the line and the whole theme inverts -- light text on what is still
       nominally the light theme. At 40 the darkest Solar is still light and the lightest
       Eclipse is still dark, so the slider cannot flip the shell out from under you. */
    const DEPTH_LIMIT = 40;

    function clampNumber(value, min, max, fallback) {
        const n = Number(value);
        if (!isFinite(n)) return fallback;
        return Math.max(min, Math.min(max, Math.round(n)));
    }

    function saturationOf(colours) {
        return clampNumber(colours.saturation, 0, 100, SATURATION_DEFAULT);
    }

    function depthOf(colours) {
        return clampNumber(colours.depth, -DEPTH_LIMIT, DEPTH_LIMIT, 0);
    }

    /* The colours as they should actually be painted: the three that were picked, plus the
       mid-tone, with both tone controls applied. Everything downstream -- every panel, border,
       glow and wash, and the avatar's own palette -- is derived from what this returns, so the
       sliders reach all of it without a single extra line anywhere else.

       The mid-tone is worked out before desaturating rather than after, so a preset's
       hand-tuned middle colour is the thing being drained rather than a fresh midpoint between
       two already-drained ends. */
    function derive(colours) {
        const drop = 1 - saturationOf(colours) / 100;
        const depth = depthOf(colours);
        let background = desaturate(colours.background, drop);
        if (depth !== 0) {
            background = mix(background, depth < 0 ? '#000000' : '#ffffff', Math.abs(depth) / 100);
        }
        return {
            background: background,
            main: desaturate(colours.main, drop),
            highlight: desaturate(colours.highlight, drop),
            mid: desaturate(midToneFor(colours), drop)
        };
    }

    // ---- deriving the stylesheet's variables ---------------------------------------------

    /* One background, one main, one highlight in; every colour the stylesheet reads out. This
       is the whole reason the eight CSS blocks could go: they were eight hand-written answers
       to this function. */
    function variablesFor(mode, colours) {
        const { background, main, highlight, mid } = derive(colours);
        const light = isLight(background);
        const ink = light ? mix('#000000', background, 0.10) : mix('#ffffff', main, 0.14);

        const vars = {
            '--bg-core': background,
            '--border-neon-bright': main,
            '--neon-cyan': main,
            '--neon-blue': mid,
            '--neon-purple': highlight,
            '--text-main': ink,
            /* The two accents again, but guaranteed to read as text. The stylesheet uses
               --neon-* for borders, fills and glows, where the preset's own colour is the
               point and contrast does not apply, and these two wherever the accent is a
               word somebody has to read. */
            '--text-accent': readable(main, background, ACCENT_CONTRAST),
            '--text-highlight': readable(highlight, background, ACCENT_CONTRAST),
            /* The quiet half of the text -- labels, captions, units -- is the ink faded
               towards the ground. How far it can fade before it stops being readable is not
               the same in both directions. On a dark ground 45% lands around 6:1 against it,
               comfortably past the 4.5:1 that WCAG asks of body text. On a light ground the
               old 62% landed on #a0a0a0, which measures 2.1:1 on Solar's near-white page --
               thirty-two separate labels across the HUD, every one of them below the line and
               several of them ten pixels tall. 36% is the same idea at a readable weight:
               about 5:1, still visibly quieter than the main ink, which is the whole point
               of the colour. */
            '--text-dim': mix(ink, background, light ? 0.36 : 0.45)
        };

        if (mode === 'cyberpunk') {
            /* Lit chrome: panels glow faintly with the ground's own hue, borders are the accent
               at low alpha, and the glow variables are real neon -- a wide soft halo, not a
               drop shadow. */
            Object.assign(vars, {
                '--bg-panel': rgba(raise(background, 2.0, 4), 0.8),
                '--bg-panel-solid': rgba(raise(background, 2.0, 4), 0.9),
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
                '--bg-panel-solid': light ? rgba(mix(background, '#ffffff', 0.75), 0.9) : rgba(raise(background, 1.0, 11), 0.9),
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
        root.style.colorScheme = isLight(derive(colours).background) ? 'light' : 'dark';
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
                /* Not merely un-editable: a background stored for Daylight or Midnight by an
                   older build is replaced by the designed one here, so the rule holds for what
                   is painted and not only for what the pickers offer. */
                background: groundIsFixed(mode) ? defaultColours(mode).background : c.background,
                main: c.main,
                highlight: c.highlight,
                saturation: clampNumber(c.saturation, 0, 100, SATURATION_DEFAULT),
                depth: clampNumber(c.depth, -DEPTH_LIMIT, DEPTH_LIMIT, 0),
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

    /* What the native side said about the desktop, or nothing yet.
     *
     * Deliberately not in localStorage: this is a fact about the machine as it is right now,
     * not a preference, and a stored copy would be a stale colour waiting to be painted on
     * the next start before the probe comes back. It lives for the life of the window and is
     * refreshed by followDesktop.
     *
     * `accent` is the desktop's own colour; `mode` is `'light'`, `'dark'` or null. `note` is
     * what to tell an operator when the answer is incomplete or the platform cannot answer at
     * all -- Settings shows it, which is the difference between "this does nothing here" and
     * "this is broken". */
    let desktop = { mode: null, accent: null, source: 'none', note: '' };

    function desktopTheme() {
        return { mode: desktop.mode, accent: desktop.accent, source: desktop.source, note: desktop.note };
    }

    /* Records what the native probe found. Returns true when it changed anything worth
       repainting for, so a caller can skip a repaint on a probe that confirmed what it
       already knew -- which is most of them, since this re-reads on every window focus. */
    function setDesktopTheme(info) {
        const mode = (info && (info.mode === 'light' || info.mode === 'dark')) ? info.mode : null;
        const accent = (info && isColour(info.accent)) ? toHex(toRgb(info.accent)) : null;
        const next = {
            mode: mode,
            accent: accent,
            source: (info && typeof info.source === 'string') ? info.source : 'none',
            note: (info && typeof info.note === 'string') ? info.note : ''
        };
        const changed = next.mode !== desktop.mode || next.accent !== desktop.accent;
        desktop = next;
        return changed;
    }

    /* Which of the two flat modes the desktop is asking for.
     *
     * The native probe wins when it has an answer, because on Linux it is reading the XDG
     * appearance portal while the media query below is guessing from the GTK theme name. The
     * media query is the fallback, and on Windows and macOS the two agree anyway. */
    function systemMode() {
        if (desktop.mode === 'dark') return 'eclipse';
        if (desktop.mode === 'light') return 'solar';
        const dark = global.matchMedia && global.matchMedia('(prefers-color-scheme: dark)').matches;
        return dark ? 'eclipse' : 'solar';
    }

    /* True while the desktop is still the authority -- nothing has been chosen in this
       browser, or Follow the desktop theme has been chosen again since. */
    function followingSystem() {
        return readStored().mode === null;
    }

    /* The colours to wear while following the desktop: the mode's own designed shell, with the
       desktop's accent and its derived companion in place of the designed pair.
       
       The ground is the designed one rather than anything stored, because following the desktop
       means wearing the shell as drawn -- an accent somebody mixed by hand for Midnight last
       week is not what the desktop is asking for. The two tone sliders are kept, though: those
       are "not dark enough for me" and "too bright for me", which are true of a person rather
       than of a palette, and resetting them every time the desktop is followed would quietly
       undo a deliberate adjustment.
       
       No accent reported -- an older Windows with none set, a Linux desktop with no portal, a
       window with no bridge to the native side at all -- leaves the designed pair alone. That
       is the whole of the fallback: the shell still follows light and dark, and the colours are
       the ones the mode was drawn with. */
    function systemColours(mode, stored) {
        const colours = defaultColours(mode);
        if (stored) {
            colours.saturation = stored.saturation;
            colours.depth = stored.depth;
        }
        if (!desktop.accent) return colours;
        colours.main = desktop.accent;
        /* Asked of the ground that will actually be painted, not of the mode: the two flat
           modes keep the shell they were designed with, so for them the two are the same
           thing, but reading the background is what the rest of this file does and a mode
           added later would get the right answer for free. */
        colours.highlight = companionFor(desktop.accent, isLight(colours.background));
        /* Not a named palette any more, so nothing in Settings should show as selected: these
           two colours came from the desktop, and claiming they are Night City's would be a
           lie the swatch grid tells on the engine's behalf. */
        colours.preset = null;
        return colours;
    }

    /* The theme as it should be painted right now.
     *
     * `following` rides along so the UI can say where the colours came from without asking a
     * second question and risking a different answer. */
    function current() {
        const state = readStored();
        if (state.mode === null) {
            const mode = systemMode();
            return { mode: mode, colours: systemColours(mode, state.colours[mode]), following: true };
        }
        return {
            mode: state.mode,
            colours: state.colours[state.mode] || defaultColours(state.mode),
            following: false
        };
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
        /* The fourth choice in the mode row, and the one that is not a mode: it hands the
           decision back to the desktop. Stored as a null mode, which is already what "nothing
           chosen" means, so a fresh install and someone who has chosen this end up in exactly
           the same state rather than in two states that have to behave the same. */
        if (mode === SYSTEM_MODE) {
            const state = readStored();
            state.mode = null;
            write(state);
            return current();
        }
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
    /* Saturation and depth, which the two sliders in Settings send. Unlike a colour these do
       not clear the selected preset: turning the neon down on Night City is still Night City,
       and showing nothing as selected afterwards would imply a hand-mixed palette that is not
       what happened. */
    function setTone(slot, value) {
        if (slot !== 'saturation' && slot !== 'depth') return current();
        const now = current();
        const state = readStored();
        state.mode = now.mode;
        const colours = state.colours[now.mode] || defaultColours(now.mode);
        colours[slot] = slot === 'saturation'
            ? clampNumber(value, 0, 100, SATURATION_DEFAULT)
            : clampNumber(value, -DEPTH_LIMIT, DEPTH_LIMIT, 0);
        state.colours[now.mode] = colours;
        write(state);
        return current();
    }

    function setColour(slot, hex) {
        if (!isColour(hex)) return current();
        const now = current();
        if (slotsFor(now.mode).indexOf(slot) === -1) return current();
        const state = readStored();
        state.mode = now.mode;
        const colours = state.colours[now.mode] || defaultColours(now.mode);
        colours[slot] = toHex(toRgb(hex));
        colours.preset = null;   // hand-mixed now, so no preset should show as selected
        state.colours[now.mode] = colours;
        write(state);
        return current();
    }

    /* A named palette worn as accents only, leaving the mode's own ground alone. This is how
       the swatches work in Daylight and Midnight: Night City's yellow on a white page is a
       perfectly reasonable thing to want, Night City's near-black ground on a light shell is
       not, and picking a colour should not silently move you into another mode to get it.
       The preset is still recorded, so it shows as selected and its hand-tuned mid-tone is
       still the one used -- midToneFor matches on the two accents, which are exactly what was
       taken from it. */
    function setAccents(id) {
        const p = preset(id);
        if (!p) return current();
        const now = current();
        const state = readStored();
        state.mode = now.mode;
        const colours = state.colours[now.mode] || defaultColours(now.mode);
        colours.main = p.main;
        colours.highlight = p.highlight;
        colours.preset = p.id;
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

    /* Asks the native side what the desktop looks like.
     *
     * Over the Tauri bridge in the app, and over the HTTP API in a browser -- the second one
     * matters more, not less: a page served to a browser has the media query and no way at all
     * to ask for the accent colour. A caller with its own authenticated fetch passes it as
     * `fetcher` (the HUD does, because the LAN API wants a device token); without one this
     * falls back to a plain same-origin request, which is what the sprite and the workbench
     * need and all they need.
     *
     * Every failure resolves to null rather than rejecting. A probe that cannot run is not an
     * error worth a console full of red: it means the designed palette stands, which is a
     * perfectly good theme. */
    function readDesktop(fetcher) {
        if (typeof fetcher === 'function') {
            return Promise.resolve().then(fetcher).catch(() => null);
        }
        if (global.__TAURI__ && global.__TAURI__.core) {
            return global.__TAURI__.core.invoke('desktop_theme_rust').catch(() => null);
        }
        if (typeof global.fetch !== 'function') return Promise.resolve(null);
        return global.fetch('/api/desktop/theme')
            .then((resp) => (resp.ok ? resp.json() : null))
            .catch(() => null);
    }

    /* The whole of "follow the desktop theme", from one call.
     *
     * Reads the desktop now, repaints through onChange, and keeps following:
     *
     *   - the media query, for a light/dark flip, as followSystem always did;
     *   - the window being shown again, because that is when a re-read is both cheap and
     *     likely to find something new.
     *
     * The second one is there because *the accent has no change event*, on any platform. The
     * media query fires for light and dark and nothing fires for a colour, so the realistic
     * sequence -- open Windows' settings, pick a new accent, come back to AETHER1 -- would
     * otherwise show the old colour until a restart. Coming back to the window is exactly that
     * moment, so that is where the re-read goes. A probe runs one short-lived process and only
     * repaints when the answer actually changed, which on a focus switch is almost never.
     *
     * Guarded by followingSystem() on every event rather than unsubscribed, same as
     * followSystem: the moment a mode is picked this goes quiet by itself.
     */
    function followDesktop(onChange, fetcher) {
        const refresh = function (force) {
            if (!followingSystem()) return Promise.resolve();
            return readDesktop(fetcher).then((info) => {
                if (!info) return;
                const changed = setDesktopTheme(info);
                if ((changed || force) && followingSystem()) onChange(current());
            });
        };

        followSystem(function () {
            /* A light/dark flip repaints on the media query's word straight away -- the shell
               must not wait on a process -- and the probe that follows it confirms the mode and
               picks up an accent changed at the same time. */
            onChange(current());
            refresh(false);
        });

        if (global.document) {
            global.document.addEventListener('visibilitychange', function () {
                if (!global.document.hidden) refresh(false);
            });
        }
        global.addEventListener('focus', function () { refresh(false); });

        /* The first read repaints even when nothing changed: on startup "nothing changed" means
           the probe agreed with the designed palette, and the window has not yet been painted
           with the desktop's accent at all. */
        return refresh(true);
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
        SYSTEM_MODE: SYSTEM_MODE,
        desktopTheme: desktopTheme,
        setDesktopTheme: setDesktopTheme,
        companionFor: companionFor,
        followDesktop: followDesktop,
        setMode: setMode,
        setPreset: setPreset,
        setColour: setColour,
        setAccents: setAccents,
        slotsFor: slotsFor,
        groundIsFixed: groundIsFixed,
        setTone: setTone,
        toneOf: function (colours) {
            return { saturation: saturationOf(colours), depth: depthOf(colours) };
        },
        DEPTH_LIMIT: DEPTH_LIMIT,
        SATURATION_DEFAULT: SATURATION_DEFAULT,
        resetColours: resetColours,
        followSystem: followSystem,
        isColour: isColour,
        isLight: isLight
    };
})(window);
