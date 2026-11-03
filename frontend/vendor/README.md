# Vendored front-end assets

The HUD used to fetch its stylesheet, its 3D library and its fonts from three CDNs every
time it opened. That is normal for a web page and wrong for this app: with no network — or
a blocked one, or a corporate proxy, or a bad afternoon at Cloudflare — the window still
opened, but unstyled, with no avatar and nothing on screen saying why. It also meant a
companion sold as *yours*, running on *your* hardware, announced itself to Cloudflare and
Google on every launch.

These are local copies. Nothing here is fetched at runtime.

| File | What it is | Version | Where it came from |
| --- | --- | --- | --- |
| `tailwind.css` | The utility classes the HUD is laid out with | built with Tailwind 3.4.17 | generated — see below |
| `three.min.js` | 3D rendering, for the avatars | three r128 (`three@0.128.0`) | npm, tree-shaken with esbuild; MIT terms in `licenses/threejs-MIT.txt` |
| `fonts/*.woff2` | Noto Sans and Noto Sans Mono, regular and bold | Noto Project, 2022 | Noto system fonts converted to WOFF2; SIL OFL 1.1 in `fonts/OFL.txt` |
| `fonts.css` | `@font-face` rules pointing at the local Noto files | — | hand-written |
| `xterm/xterm.js` | The terminal screen — draws a real shell inside the HUD panel | `@xterm/xterm` 5.5.0 | npm, `lib/xterm.js`, unmodified |
| `xterm/xterm.css` | Its stylesheet | `@xterm/xterm` 5.5.0 | npm, `css/xterm.css`, unmodified |
| `xterm/addon-fit.js` | Works out how many rows and columns fit the panel | `@xterm/addon-fit` 0.10.0 | npm, `lib/addon-fit.js`, unmodified |
| `xterm/LICENSE` | The MIT licence those three are published under | — | npm, unmodified |
| `qrcode/qrcode.js` | Draws the QR code the pairing sequence shows, so a phone can reach this machine by scanning rather than typing an address | `qrcode-generator` 1.4.4 | npm, `qrcode.js`, unmodified (MIT, notice in the file) |
| `licenses/` | Full MIT notices for three.js, Tailwind CSS and qrcode-generator | — | upstream license texts |

`three r128` is the version the code was written against; the avatar code uses APIs that
later releases changed, so this is a deliberate pin rather than a stale one.

Fonts are licensed, not copyright-free: the Noto Project's SIL Open Font License permits
bundling and redistribution with A1 when its copyright and full license are included. Noto
replaces the previous custom font files so the interface uses documented open fonts without
runtime font requests.

## Rebuilding the stylesheet

The CDN build of Tailwind compiled styles in the browser, so any class worked the moment
it appeared in the DOM. A static build contains only the classes found by scanning the
files listed in `tailwind.config.js`. They are found in plain text, so a class inside a
template literal (`` `text-xs ${tone}` ``) is fine as long as the value it can take is
written literally somewhere in the source — which is how the frontend already writes them.

If you add classes and they appear to do nothing, rebuild:

```sh
./scripts/build_vendor_css.sh
```

That needs Node and npm. Running Aether1 does not.

## Updating a library

Replace the file, update the version in the table above, and check the HUD still draws
(the avatar in particular, for three.js). None of this is wired into the app's build:
these are files on disk that the page loads.
