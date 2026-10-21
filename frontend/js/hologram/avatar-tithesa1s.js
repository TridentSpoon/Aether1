/* Avatar: TithesA1s -- Terminal Interface for Theoretic Heuristics, Execution System on
 * A1 Service. The eXcelsior Class's ninth, and the only one that is never entirely there.
 *
 * Trident named it. It shipped as L1minal, which was a placeholder of mine -- the word for
 * the state of being in between, which is still what the avatar is about. The old id is
 * kept as an alias in avatar-loader.js, because that one did ship and somebody's saved
 * settings will still name it.
 *
 * Every other avatar in the line is fully present at all times. This one is a shell cut
 * into forty horizontal slices, of which only a travelling band is ever solid. The rest
 * are not dimmed, they are absent, and all that is left of them is the yellow framework
 * saying where they would have been -- which is what keeps the silhouette readable while
 * most of it is gone. The band never stops, so it is always materialising somewhere and
 * dematerialising somewhere else.
 *
 * The states fall out of that. Thinking narrows the band, because more of it is elsewhere.
 * Speaking opens the band to nearly the whole shell, because to talk to you is to be here.
 * Listening stops the band dead and holds it, half-present.
 *
 * Colours are Trident's and are deliberately NOT theme-tinted, because for this one the
 * palette is the point: the stone is blue, the framework is yellow, and the edge where a
 * slice is coming back into being glows red -- between them they cover everywhere the
 * stone has been. The teal inner lattice and the crimson of the solid slices come from his
 * file. The setting the gem sits in is the one part that follows the HUD's colour, so the
 * theme still reaches the avatar somewhere.
 *
 * The stone never goes. Every other part of this avatar can be absent on a given frame;
 * that cannot, which is the point of it.
 *
 * Two things about the port. His shell is a MeshPhysicalMaterial with transmission, and
 * his lights are DirectionalLights over a scene-level exponential fog -- none of those
 * three is in the vendored three.js bundle (see scripts/build_vendor_three.sh), and the
 * HUD owns the scene and its lighting in any case, so the translucency here is additive
 * blending instead. And the slices are one LineSegments with per-vertex colour rather than
 * forty objects, so a frame costs a colour write and nothing else.
 *
 * His file also hangs a cape off it. It was built and then taken out at Trident's word --
 * it read as a skirt under the shell however it was tapered, leaned or thinned, and the
 * avatar is cleaner as a thing hanging in space with nothing below it.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'tithesa1s',
    label: 'TithesA1s',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const R = 52;               // the shell's radius
        const SLICES = 40;
        const SEG = 32;             // points round one slice
        const BAND_MESHES = 14;     // how many slices the solid band can show at once
        const MOTES = 180;

        /* Trident's palette, fixed. See the file header for why none of this follows the
           theme: the stone is blue, the framework yellow, the materialising edge red. */
        const STONE = 0x2b6bff;
        const FRAME = 0xffcc00;
        const FLUX = 0xff2244;
        const LATTICE = 0x00ffcc;   // his inner neural lattice
        const SOLID = 0xbb0033;     // his shell crimson

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: STONE,
            transparent: true, opacity: 0.1, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(220, 220, 1);
        glow.position.z = -60;
        group.add(glow);

        /* The framework: every slice, drawn once. One geometry for all forty, because the
           points never move -- only their colour does, and a frame that writes colours and
           nothing else is the cheapest way to make matter come and go. */
        const framePos = [];
        const sliceHeight = [];     // each slice's y, for working out what the band covers
        for (let s = 0; s < SLICES; s++) {
            // Spread across the sphere's diameter, with the poles left off: a slice of
            // radius zero is a point, and a point is not a slice.
            const t = (s + 0.5) / SLICES;
            const y = (t * 2 - 1) * R * 0.97;
            const radius = Math.sqrt(Math.max(0, R * R - y * y));
            sliceHeight.push({ y, radius });
            for (let j = 0; j < SEG; j++) {
                const a = (j / SEG) * Math.PI * 2;
                const b = ((j + 1) / SEG) * Math.PI * 2;
                framePos.push(Math.cos(a) * radius, y, Math.sin(a) * radius);
                framePos.push(Math.cos(b) * radius, y, Math.sin(b) * radius);
            }
        }
        const frameGeom = new THREE.BufferGeometry();
        frameGeom.setAttribute('position', new THREE.Float32BufferAttribute(framePos, 3));
        frameGeom.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(framePos.length), 3));
        const frameMat = new THREE.LineBasicMaterial({
            vertexColors: true, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        });
        group.add(new THREE.LineSegments(frameGeom, frameMat));

        /* The matter in the band: one short open-ended wall per present slice, from a pool
           moved onto whichever slices are solid this frame.

           Two shapes were wrong before this one. Flat discs on each slice are invisible,
           because the HUD's camera is level with the avatar and a horizontal disc is seen
           edge-on. A single wall spanning the whole band is worse: a cylinder is straight
           and the shell is not, so at the top of the travel the wall stands outside the
           shell as a box. A stack of short walls, each at its own slice's radius, follows
           the shell exactly and is solid from any angle the view can be dragged to. */
        const bandMat = new THREE.MeshBasicMaterial({
            color: SOLID, transparent: true, opacity: 0.3,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const wallGeom = new THREE.CylinderGeometry(1, 1, 1, SEG, 1, true);
        const bandMeshes = [];
        for (let i = 0; i < BAND_MESHES; i++) {
            const wall = new THREE.Mesh(wallGeom, bandMat);
            wall.visible = false;
            group.add(wall);
            bandMeshes.push(wall);
        }

        // --- His inner neural lattice, always present: the part of it that is not matter. ---
        const latticeMat = new THREE.MeshBasicMaterial({
            color: LATTICE, wireframe: true,
            transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending,
        });
        const lattice = new THREE.Mesh(new THREE.IcosahedronGeometry(R * 0.82, 2), latticeMat);
        group.add(lattice);

        /* The stone at the brow: a cut gem, not a disc. Never absent -- it is not in the
           slice system at all, which is the whole of why it reads as the constant.

           It is cut by hand because there is no lighting to cut it with. The avatar's
           materials are unlit, so a faceted solid would come back as one flat silhouette
           however many faces it had. Instead each facet is given its own brightness at
           build time, written into vertex colours, and the gem reads as faceted because
           the facets genuinely differ -- not because anything is being shaded.

           The cut is a tall hexagon with a raised table: an outer girdle, an inner table
           lifted towards the viewer, crown facets between them, and a flat keel behind so
           there is no hole when the view is dragged round. */
        const stoneGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: STONE,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const stoneGlow = new THREE.Sprite(stoneGlowMat);
        stoneGlow.scale.set(44, 44, 1);

        const stoneMat = new THREE.MeshBasicMaterial({
            vertexColors: true, transparent: true, opacity: 0.95, side: THREE.DoubleSide,
        });
        const stone = new THREE.Mesh(tithesCutGem(R * 0.085, R * 0.125, R * 0.05), stoneMat);

        /* The setting the gem sits in, and the one part of this avatar that follows the
           HUD's colour. The stone, the framework and the arriving rim are all Trident's
           fixed palette, so without this the theme would reach the avatar nowhere at all. */
        const bezelMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.9,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const bezel = new THREE.Mesh(tithesGemBezel(R * 0.085, R * 0.125, R * 0.022), bezelMat);

        /* Sunk into a dark socket. Additive blue over the dark panel is bright, but the
           band passes behind the brow twice a cycle and additive over something already
           bright is white -- the stone washed out exactly when it was in front of the
           matter it is supposed to be the constant through. The socket is the floor it
           is read against. */
        const socketMat = new THREE.MeshBasicMaterial({ color: 0x04060f, transparent: true, opacity: 0.92 });
        const socket = new THREE.Mesh(tithesGemBezel(0, R * 0.148, 0), socketMat);

        const stoneAt = new THREE.Group();
        stoneAt.position.set(0, R * 0.44, R * 0.94);
        stoneAt.add(stoneGlow);
        socket.position.z = 0.3;
        stoneAt.add(socket);
        bezel.position.z = 0.6;
        stoneAt.add(bezel);
        stone.position.z = 0.9;
        stoneAt.add(stone);
        group.add(stoneAt);

        /* The matter that is not currently being used. Each mote belongs to a slice: while
           its slice is present it sits on the shell, and while the slice is absent it hangs
           out at its drifted position. Lerped between two fixed points rather than
           simulated, because what it has to say is "this is elsewhere", not "this is
           falling". */
        const motePos = new Float32Array(MOTES * 3);
        const moteHome = new Float32Array(MOTES * 3);
        const moteAway = new Float32Array(MOTES * 3);
        const moteSlice = new Int16Array(MOTES);
        for (let i = 0; i < MOTES; i++) {
            const s = Math.floor(Math.random() * SLICES);
            const a = Math.random() * Math.PI * 2;
            const { y, radius } = sliceHeight[s];
            moteSlice[i] = s;
            moteHome[i * 3] = Math.cos(a) * radius;
            moteHome[i * 3 + 1] = y;
            moteHome[i * 3 + 2] = Math.sin(a) * radius;
            const out = 1.25 + Math.random() * 0.5;
            moteAway[i * 3] = Math.cos(a) * radius * out;
            moteAway[i * 3 + 1] = y + (Math.random() - 0.5) * 22;
            moteAway[i * 3 + 2] = Math.sin(a) * radius * out;
        }
        const moteGeom = new THREE.BufferGeometry();
        moteGeom.setAttribute('position', new THREE.BufferAttribute(motePos, 3));
        // Mapped, not bare: an unmapped point sprite is a hard square.
        const moteMat = new THREE.PointsMaterial({
            map: api.helpers.glowTexture(32), color: FRAME, size: 3.2,
            transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        group.add(new THREE.Points(moteGeom, moteMat));

        return {
            group, glow, glowMat, frameGeom, frameMat, bandMeshes, bandMat,
            lattice, latticeMat, stoneAt, stoneGlow, stoneGlowMat, stone, stoneMat,
            bezel, bezelMat, socket, socketMat,
            moteGeom, motePos, moteHome, moteAway, moteSlice, moteMat,
            sliceHeight, slices: SLICES, seg: SEG, radius: R,
            presence: new Float32Array(SLICES),
            frame: new THREE.Color(FRAME), flux: new THREE.Color(FLUX), solid: new THREE.Color(SOLID),
            at: 0.5,                // where the band is, 0 at the bottom and 1 at the top
            dir: 1,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';
        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.5 : 0;

        /* How much of it is here, and where. Speaking opens the band almost to the whole
           shell; thinking narrows it; listening stops it where it stands. */
        const half = (isSpeaking ? 0.42 + ctx.audio * 0.3 : isThinking ? 0.09 : isListening ? 0.16 : 0.17)
            + ctx.click * 0.1;
        const speed = isListening ? 0 : isThinking ? 0.5 : 0.26;
        model.at += model.dir * speed * 0.016;
        // Turned at the ends rather than wrapped: a band that wraps vanishes from the top
        // and reappears at the bottom, which reads as a glitch and not as travel.
        if (model.at > 1) { model.at = 1; model.dir = -1; }
        if (model.at < 0) { model.at = 0; model.dir = 1; }

        /* Each slice's presence, and the frame colour that says which of the three states
           it is in: absent (framework only), present (solid), or just arriving (the red
           edge). The ramp at the band's rim is what makes the edge a place rather than a
           line. */
        const colours = model.frameGeom.attributes.color.array;
        const perSlice = model.seg * 2 * 3;
        const EDGE = 0.26;
        for (let s = 0; s < model.slices; s++) {
            const t = (s + 0.5) / model.slices;
            const d = Math.abs(t - model.at) / half;
            const presence = d >= 1 ? 0 : d <= 1 - EDGE ? 1 : (1 - d) / EDGE;
            model.presence[s] = presence;
            /* Arriving, not merely present: the leading rim of the band on the side it is
               travelling towards. That is the only place the red belongs -- everywhere
               else it would just be a second colour. */
            const leading = model.dir > 0 ? t > model.at : t < model.at;
            const arriving = leading && presence > 0 && presence < 1 ? 1 - presence : 0;
            /* One colour for the framework, carried from dim to bright by how present the
               slice is, and red added only at the arriving rim. An earlier version mixed
               the solid crimson into the line as well, and crimson against red is not a
               distinction anyone can see -- the whole leading half of the band read as the
               edge. The band's own colour belongs on the wall, not on the framework. */
            const level = 0.26 + presence * 0.74 + heat * 0.2;
            const r = model.frame.r * level + model.flux.r * arriving;
            const g = model.frame.g * level + model.flux.g * arriving;
            const b = model.frame.b * level + model.flux.b * arriving;
            const from = s * perSlice;
            for (let i = 0; i < perSlice; i += 3) {
                colours[from + i] = r;
                colours[from + i + 1] = g;
                colours[from + i + 2] = b;
            }
        }
        model.frameGeom.attributes.color.needsUpdate = true;

        /* The walls, laid onto the most present slices. Working outward from the band's
           middle keeps the pool where the matter actually is when the band is wider than
           the pool, rather than filling from one end and leaving the far side hollow. */
        const centre = Math.round(model.at * model.slices - 0.5);
        const step = (model.radius * 2 * 0.97) / model.slices;
        for (let i = 0; i < model.bandMeshes.length; i++) {
            const s = centre + Math.ceil(i / 2) * (i % 2 ? 1 : -1);
            const wall = model.bandMeshes[i];
            if (s < 0 || s >= model.slices || model.presence[s] <= 0.02) {
                wall.visible = false;
                continue;
            }
            const slice = model.sliceHeight[s];
            wall.visible = true;
            wall.position.y = slice.y;
            // A shade over one slice's spacing, so neighbours meet instead of leaving gaps.
            wall.scale.set(slice.radius, step * 1.15, slice.radius);
            wall.material = model.bandMat;
        }
        model.bandMat.opacity = 0.14 + heat * 0.12;
        model.frameMat.opacity = 0.75 + heat * 0.25;

        // His two lattice rotations, the part of it that never phases.
        model.lattice.rotation.y = -ctx.time * 0.15;
        model.lattice.rotation.x = ctx.time * 0.05;
        model.latticeMat.opacity = 0.35 + heat * 0.3;

        // His stone pulse, on his rate. It brightens with the voice and never goes out.
        const pulse = 0.8 + Math.sin(ctx.time * 5) * 0.2;
        model.stoneGlowMat.opacity = (0.65 + heat * 0.35) * pulse;
        model.stoneMat.opacity = 0.85 + heat * 0.15;
        model.stoneGlow.scale.setScalar(38 + Math.sin(ctx.time * 5) * 4 + heat * 10);

        model.bezelMat.opacity = 0.75 + heat * 0.25 + ctx.click * 0.15;

        /* The motes: on the shell while their slice is here, out in the room while it is
           not. */
        const { motePos, moteHome, moteAway, moteSlice, presence } = model;
        for (let i = 0; i < moteSlice.length; i++) {
            const here = presence[moteSlice[i]];
            const at = i * 3;
            motePos[at] = moteAway[at] + (moteHome[at] - moteAway[at]) * here;
            motePos[at + 1] = moteAway[at + 1] + (moteHome[at + 1] - moteAway[at + 1]) * here;
            motePos[at + 2] = moteAway[at + 2] + (moteHome[at + 2] - moteAway[at + 2]) * here;
        }
        model.moteGeom.attributes.position.needsUpdate = true;
        model.moteMat.opacity = 0.45 + heat * 0.3;

        model.group.rotation.y = Math.sin(ctx.time * 0.25) * 0.12;
        model.glowMat.opacity = 0.08 + (isSpeaking ? ctx.audio * 0.16 : Math.sin(ctx.time * 1.2) * 0.02 + 0.02);
    },

    applyPalette(model, palette) {
        model.bezelMat.color.setHex(palette.hex);
        // Everything else keeps Trident's fixed palette -- see the file header.
    },
});

/* Cuts the gem: a tall hexagonal outline, an inner table raised towards the viewer, the
   crown facets between the two, and a flat keel closing the back.

   Every facet carries its own brightness in its vertex colours, because the avatar's
   materials are unlit and a faceted solid under an unlit material is one flat silhouette.
   The brightnesses are fixed at build -- a gem whose facets changed as it sat there would
   be a lamp, not a stone. */
function tithesCutGem(rx, ry, depth) {
    const SIDES = 6;
    const TABLE = 0.46;        // how far in the flat top sits
    const pos = [];
    const col = [];
    /* STONE's own blue (0x2b6bff) as a float triple, lifted a little off the floor so the
       facet turned furthest from the light is still blue rather than black. Each facet
       multiplies this; the table goes past 1 and comes out the pale blue a table catches. */
    const tint = [0.26, 0.52, 1];

    const at = (i, scale, z) => {
        // Turned a half-step so the hexagon stands on a point rather than an edge, which
        // is what makes it read as tall rather than as a bolt head.
        const a = (i / SIDES) * Math.PI * 2 + Math.PI / 2;
        return [Math.cos(a) * rx * scale, Math.sin(a) * ry * scale, z];
    };
    const push = (p, level) => {
        pos.push(p[0], p[1], p[2]);
        col.push(tint[0] * level, tint[1] * level, tint[2] * level);
    };

    for (let i = 0; i < SIDES; i++) {
        const j = (i + 1) % SIDES;
        /* A fixed spread of brightness around the crown, lit as if from the upper left:
           it is a constant, not a calculation, so the same facet is the same every launch. */
        const level = 0.34 + 0.72 * (0.5 + 0.5 * Math.cos((i / SIDES) * Math.PI * 2 - 2.2));
        const o1 = at(i, 1, 0);
        const o2 = at(j, 1, 0);
        const t1 = at(i, TABLE, depth);
        const t2 = at(j, TABLE, depth);
        push(o1, level); push(o2, level); push(t2, level * 1.12);
        push(o1, level); push(t2, level * 1.12); push(t1, level * 1.12);
        // The keel: back to the middle, dimmer, so a dragged view finds a solid and not a hole.
        const k = [0, 0, -depth * 0.55];
        push(o2, level * 0.3); push(o1, level * 0.3); push(k, level * 0.18);
    }
    // The table, brightest because it faces the viewer squarely.
    for (let i = 1; i < SIDES - 1; i++) {
        push(at(0, TABLE, depth), 1.45);
        push(at(i, TABLE, depth), 1.45);
        push(at(i + 1, TABLE, depth), 1.45);
    }

    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    geom.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    return geom;
}

/* The band around the gem, on the same hexagonal outline so the setting follows the cut
   rather than being a circle the stone happens to sit in. An inner radius of zero fills
   it, which is how the dark socket behind the gem is made from the same function. */
function tithesGemBezel(rx, ry, width) {
    const SIDES = 6;
    const pos = [];
    const at = (i, px, py) => {
        const a = (i / SIDES) * Math.PI * 2 + Math.PI / 2;
        return [Math.cos(a) * px, Math.sin(a) * py, 0];
    };
    for (let i = 0; i < SIDES; i++) {
        const j = (i + 1) % SIDES;
        const o1 = at(i, rx + width, ry + width);
        const o2 = at(j, rx + width, ry + width);
        const i1 = at(i, rx, ry);
        const i2 = at(j, rx, ry);
        pos.push(...o1, ...o2, ...i2);
        pos.push(...o1, ...i2, ...i1);
    }
    const geom = new THREE.BufferGeometry();
    geom.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    return geom;
}
