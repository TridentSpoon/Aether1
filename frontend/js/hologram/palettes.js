/**
 * The colour presets, and the palette the avatar renderer reads.
 *
 * A theme is two independent things: a *mode*, which decides the chrome (Cyberpunk keeps the
 * scanlines, corner brackets and neon glow; Solar and Eclipse are flat window shells), and
 * three colours:
 *
 *   background -- the ground everything sits on. Stable: changing an accent never moves it.
 *   main       -- the primary colour. Headings, borders, gauges, the avatar's body.
 *   highlight  -- the companion that keeps the primary from being the only colour on screen.
 *
 * A preset is just a named bundle of those four values, so every preset below could equally
 * have been typed into the three colour pickers by hand -- which is the point: there is no
 * privileged set of themes any more, only starting points.
 *
 * `mid` is the tone between main and highlight. Avatars use it for their middle depth layer,
 * and it is spelled out for these presets rather than computed because these six were tuned
 * by eye first; theme.js computes it for colours that were not.
 */
const THEME_PRESETS = [
    { id: 'halcy',      label: 'Cyan',    mode: 'cyberpunk', background: '#040711', main: '#00f0ff', highlight: '#b026ff', mid: '#00a2ff' },
    { id: 'nexus',      label: 'Green',   mode: 'cyberpunk', background: '#010c04', main: '#00ff66', highlight: '#00ffaa', mid: '#00cc55' },
    { id: 'arx-limes',  label: 'Amber',   mode: 'cyberpunk', background: '#0e0600', main: '#ffaa00', highlight: '#ff3300', mid: '#ff5500' },
    { id: 'arx-logos',  label: 'Magenta', mode: 'cyberpunk', background: '#0b0214', main: '#e024c3', highlight: '#ff00ff', mid: '#9d00ff' },
    { id: 'red',        label: 'Crimson', mode: 'cyberpunk', background: '#0d0204', main: '#ff1133', highlight: '#00f0ff', mid: '#0066ff' },
    { id: 'night-city', label: 'Night',   mode: 'cyberpunk', background: '#0a0a06', main: '#fcee0a', highlight: '#ff003c', mid: '#00e5ff' },
    { id: 'solar',      label: 'Daylight', mode: 'solar',    background: '#f3f3f3', main: '#0067c0', highlight: '#8764b8', mid: '#005a9e' },
    { id: 'eclipse',    label: 'Midnight', mode: 'eclipse',  background: '#202020', main: '#60cdff', highlight: '#b4a0ff', mid: '#4cc2ff' }
];

const THEME_PRESETS_BY_ID = {};
THEME_PRESETS.forEach((preset) => { THEME_PRESETS_BY_ID[preset.id] = preset; });

/* What the avatar renderer and every avatar plugin actually read: hex (main), hex2 (mid),
   hex3 (highlight) and r/g/b (main again, as three 0..1 floats, for per-vertex colour
   buffers). That contract is documented in avatar-template.js and third-party avatar files
   depend on it, so it is built from the presets rather than replaced by them.
   theme.js produces the same shape for colours picked by hand -- see paletteFor(). */
function themePaletteFrom(main, mid, highlight) {
    const channel = (hex, at) => parseInt(hex.slice(at, at + 2), 16);
    return {
        r: channel(main, 1) / 255,
        g: channel(main, 3) / 255,
        b: channel(main, 5) / 255,
        hex: parseInt(main.slice(1), 16),
        hex2: parseInt(mid.slice(1), 16),
        hex3: parseInt(highlight.slice(1), 16)
    };
}

const THEME_PALETTES = {};
THEME_PRESETS.forEach((p) => {
    THEME_PALETTES[p.id] = themePaletteFrom(p.main, p.mid, p.highlight);
});
/* Aliases kept from when these were theme names rather than colour presets: a saved setting,
   a persona switch or somebody's avatar file may still ask for either of them. */
THEME_PALETTES.crimson = THEME_PALETTES.red;
THEME_PALETTES.matrix = THEME_PALETTES.nexus;

/* `const` at the top level of a classic script does not become a property of window, and
   theme.js is a separate script that needs to read these. Published deliberately rather than
   by switching to `var` and hoping. */
window.THEME_PRESETS = THEME_PRESETS;
window.THEME_PRESETS_BY_ID = THEME_PRESETS_BY_ID;
window.THEME_PALETTES = THEME_PALETTES;
window.themePaletteFrom = themePaletteFrom;
