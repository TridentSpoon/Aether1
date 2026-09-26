/* Which file each avatar lives in, and fetching it on demand.
 *
 * Every avatar used to arrive as its own <script> tag in the page head: nineteen files,
 * about 195 KB, parsed and built into the scene before the window could be used -- to show
 * one of them. The other eighteen were paid for at every launch by everyone, for the chance
 * that somebody might pick one. Here they are named instead, and the file is fetched the
 * first time its avatar is actually asked for.
 *
 * This list is the single source of truth for that. It used to be three copies of the same
 * nineteen tags (the HUD, the sprite window and the workbench), which had already drifted:
 * the same avatar was pinned to three different versions across the three pages. One list
 * cannot drift from itself.
 *
 * The cache-buster is per avatar and lives in `v` -- bump it when that avatar's file
 * changes, exactly as you would have bumped the ?v= on its old <script> tag.
 */
(function () {
    'use strict';

    if (!window.HologramAvatar) {
        console.error('avatar-loader.js must load after core.js');
        return;
    }

    const BASE = 'js/hologram/';

    /* builder/animator name a hand-modelled avatar's two methods on HologramAvatar.prototype
       (see avatar-halcy.js and animate.js). A plugin avatar has neither -- it registers
       itself with HologramAvatar.registerAvatar and carries its own build/animate. */
    const AVATARS = {
        'halcy':          { file: 'avatar-halcy.js',          v: 17, builder: 'buildHalcyAvatar',    animator: 'animateHalcy' },
        'arx-limes':      { file: 'avatar-arx-limes.js',      v: 18, builder: 'buildArxLimesAvatar', animator: 'animateArxLimes' },
        'nexus':          { file: 'avatar-nexus.js',          v: 29, builder: 'buildNexusAvatar',    animator: 'animateNexus' },
        'red':            { file: 'avatar-red9000.js',        v: 19, builder: 'buildRed9000Avatar',  animator: 'animateRed9000' },
        'arx-logos':      { file: 'avatar-arx-logos.js',      v: 18, builder: 'buildArxLogosAvatar', animator: 'animateArxLogos' },
        'alt':            { file: 'avatar-alt.js',            v: 3,  builder: 'buildAltAvatar',      animator: 'animateAlt' },
        'arx-locas':      { file: 'avatar-arx-locas.js',      v: 2 },
        'arx-legionare':  { file: 'avatar-arx-legionare.js',  v: 1 },
        'arx-loregenda':  { file: 'avatar-arx-loregenda.js',  v: 1 },
        'arx-lyksaum':    { file: 'avatar-arx-lyksaum.js',    v: 1 },
        'arx-lexico':     { file: 'avatar-arx-lexico.js',     v: 1 },
        'arx-lucre':      { file: 'avatar-arx-lucre.js',      v: 1 },
        'arx-lkemi':      { file: 'avatar-arx-lkemi.js',      v: 1 },
        'a1':             { file: 'avatar-a1.js',             v: 1 },
        'senti':          { file: 'avatar-senti.js',          v: 2 },
        'white-rabbit':   { file: 'avatar-white-rabbit.js',   v: 2 },
        'operator':       { file: 'avatar-operator.js',       v: 3 },
        'enxephalon':     { file: 'avatar-enxephalon.js',     v: 1 },
        'cicero':         { file: 'avatar-cicero.js',         v: 1 },
        'praxis':         { file: 'avatar-praxis.js',         v: 1 },
        'chrono-maistresse': { file: 'avatar-chrono-maistresse.js', v: 2 },
        'mairad':         { file: 'avatar-mairad.js',         v: 1 },
        /* The custom avatar is assembled from the shared parts library rather than modelled
           by hand, so its file is useless without it -- 56 KB that only this one avatar
           needs, which is exactly the kind of weight worth not carrying at launch. */
        'custom':         { file: 'avatar-custom.js',         v: 2, needs: [{ file: 'parts.js', v: 2 }] },
    };

    /* Older names the HUD and saved settings still use for the same avatar. setAvatar has
       always accepted these; resolving them here means one spelling reaches the loader. */
    const ALIASES = {
        'matrix': 'nexus',
        'crimson': 'red',
        'cunningham': 'alt',
        'a1ter_nul': 'alt',
    };

    /* One promise per file, kept so a second request for the same avatar waits on the first
       fetch instead of starting another -- clicking two pills quickly, or the HUD and the
       sprite window asking at the same moment. */
    const loads = new Map();

    function loadScript(file, v) {
        const src = `${BASE}${file}?v=${v}`;
        if (loads.has(src)) return loads.get(src);
        const pending = new Promise((resolve) => {
            const script = document.createElement('script');
            script.src = src;
            script.onload = () => resolve(true);
            script.onerror = () => {
                /* A missing avatar file must not take the window down: report it and carry
                   on showing whatever is already on screen. */
                console.error(`Avatar file "${src}" could not be loaded`);
                loads.delete(src); // a later attempt is allowed to retry
                resolve(false);
            };
            document.head.appendChild(script);
        });
        loads.set(src, pending);
        return pending;
    }

    HologramAvatar.canonicalAvatarId = function (id) {
        return ALIASES[id] || id;
    };

    HologramAvatar.builderNameFor = function (id) {
        const entry = AVATARS[HologramAvatar.canonicalAvatarId(id)];
        return entry ? entry.builder : undefined;
    };

    /* hAlcy is the engine's fallback shape for a name it does not recognise (see setAvatar
       and animate), so an unknown id animates as hAlcy here too rather than standing still. */
    HologramAvatar.animatorNameFor = function (id) {
        const entry = AVATARS[HologramAvatar.canonicalAvatarId(id)];
        if (entry) return entry.animator;
        return AVATARS.halcy.animator;
    };

    HologramAvatar.knownAvatarIds = function () {
        return Object.keys(AVATARS);
    };

    /* Resolves once this avatar's file (and anything it needs) is in the page. An avatar
       with no entry here -- one imported by hand in the workbench, say -- resolves straight
       away: there is no file to fetch, it is already registered. */
    HologramAvatar.loadAvatar = function (id) {
        const entry = AVATARS[HologramAvatar.canonicalAvatarId(id)];
        if (!entry) return Promise.resolve(true);
        const deps = (entry.needs || []).map((dep) => loadScript(dep.file, dep.v));
        return Promise.all(deps).then(() => loadScript(entry.file, entry.v));
    };

    /* Every avatar at once. The workbench wants this -- its whole job is showing you the
       full list to pick from -- and nothing else should: the HUD fetches the one it is
       wearing, which is the entire point of this file. */
    HologramAvatar.loadAllAvatars = function () {
        return Promise.all(Object.keys(AVATARS).map((id) => HologramAvatar.loadAvatar(id)));
    };
}());
