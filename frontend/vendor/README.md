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
| `three.min.js` | 3D rendering, for the avatars | three r128 (`three@0.128.0`) | npm, `build/three.min.js`, unmodified |
| `fonts/*.woff2` | Orbitron 500/700/900, Rajdhani 500/600/700, Share Tech Mono 400 | `@fontsource/*` 5.3.0 | npm, latin subsets, unmodified |
| `fonts.css` | `@font-face` rules pointing at those files | — | hand-written |

`three r128` is the version the code was written against; the avatar code uses APIs that
later releases changed, so this is a deliberate pin rather than a stale one.

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
