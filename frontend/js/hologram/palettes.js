/**
 * Color Themes: halcy (cyan), nexus (green), arx-limes (amber), arx-logos (magenta), red (crimson),
 * night-city (Cyberpunk 2077's classic yellow/cyan/red)
 * Any color theme can be applied to any avatar shape (see core.js).
 */
const THEME_PALETTES = {
    halcy:      { r: 0.0, g: 0.9, b: 1.0, hex: 0x00f0ff, hex2: 0x00a2ff, hex3: 0xb026ff },
    nexus:      { r: 0.1, g: 1.0, b: 0.4, hex: 0x00ff66, hex2: 0x00cc55, hex3: 0x00ffaa },
    'arx-limes': { r: 1.0, g: 0.67, b: 0.0, hex: 0xffaa00, hex2: 0xff5500, hex3: 0xff3300 },
    'arx-logos': { r: 0.88, g: 0.14, b: 0.76, hex: 0xe024c3, hex2: 0x9d00ff, hex3: 0xff00ff },
    red:        { r: 1.0, g: 0.07, b: 0.13, hex: 0xff1133, hex2: 0x0066ff, hex3: 0x00f0ff },
    'night-city': { r: 1.0, g: 0.93, b: 0.04, hex: 0xfcee0a, hex2: 0x00e5ff, hex3: 0xff003c }
};
