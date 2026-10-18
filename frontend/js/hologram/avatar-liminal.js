/* Avatar: L1minal -- the state of being in between, which is the whole of this one. The
 * eXcelsior Class's ninth, and the only one that is never entirely there.
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
 * file. The cape is the one part that follows the HUD's colour, so the theme still reaches
 * the avatar somewhere.
 *
 * The stone never goes. Every other part of this avatar can be absent on a given frame;
 * that cannot, which is the point of it.
 *
 * Three things about the port. His shell is a MeshPhysicalMaterial with transmission, and
 * his lights are DirectionalLights over a scene-level exponential fog -- none of those
 * three is in the vendored three.js bundle (see scripts/build_vendor_three.sh), and the
 * HUD owns the scene and its lighting in any case, so the translucency here is additive
 * blending instead. His cape is a 32x32 plane walked every frame; this is 16x16, which is
 * 289 vertices instead of 1,089 and reads the same at the panel's size. And the slices are
 * one LineSegments with per-vertex colour rather than forty objects, so a frame costs a
 * colour write and nothing else.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'liminal',
    label: 'L1minal',
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

        /* The stone, at the brow, on his own pulse rate. Never absent -- it is not in the
           slice system at all, which is the whole of why it reads as the constant. */
        const stoneGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: STONE,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const stoneGlow = new THREE.Sprite(stoneGlowMat);
        stoneGlow.scale.set(40, 40, 1);
        const stoneMat = new THREE.MeshBasicMaterial({
            color: 0x9ec2ff, transparent: true, opacity: 0.95, side: THREE.DoubleSide,
        });
        const stone = new THREE.Mesh(new THREE.CircleGeometry(R * 0.062, 18), stoneMat);
        /* Sunk into a dark socket. Additive blue over the dark panel is bright, but the
           band passes behind the brow twice a cycle and additive over something already
           bright is white -- the stone washed out exactly when it was in front of the
           matter it is supposed to be the constant through. The socket is the floor it
           is read against. */
        const socketMat = new THREE.MeshBasicMaterial({ color: 0x04060f, transparent: true, opacity: 0.92 });
        const socket = new THREE.Mesh(new THREE.CircleGeometry(R * 0.1, 20), socketMat);
        const stoneAt = new THREE.Group();
        stoneAt.position.set(0, R * 0.44, R * 0.94);
        stoneAt.add(stoneGlow);
        socket.position.z = 0.3;
        stoneAt.add(socket);
        stone.position.z = 0.8;
        stoneAt.add(stone);
        group.add(stoneAt);

        /* His cape, with his vertex wave, at a quarter of the vertices -- and drawn as a
           mesh of lines rather than a filled sheet, because a filled quad behind a
           translucent shell reads as a grey slab hanging off it, not as a veil. Tapered in
           at the top so it leaves the shell rather than floating under it. */
        const capeGeom = new THREE.PlaneGeometry(74, 84, 14, 14);
        const capeRaw = capeGeom.attributes.position;
        for (let i = 0; i < capeRaw.count; i++) {
            const y = capeRaw.getY(i);
            const down = (42 - y) / 84;          // 0 at the top edge, 1 at the hem
            capeRaw.setX(i, capeRaw.getX(i) * (0.34 + down * 0.66));
        }
        const capeRest = new Float32Array(capeRaw.array);
        const capeMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.22,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const cape = new THREE.Mesh(capeGeom, capeMat);
        cape.position.set(0, -64, -26);
        // Leaned back, so it trails behind rather than hanging under the shell like a skirt.
        cape.rotation.x = 0.34;
        group.add(cape);

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
            cape, capeGeom, capeRest, capeMat, socket, socketMat,
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

        // His cape wave, from the rest positions so it does not compound.
        const cap = model.capeGeom.attributes.position;
        const arr = cap.array;
        for (let i = 0; i < arr.length; i += 3) {
            const x = model.capeRest[i];
            const y = model.capeRest[i + 1];
            arr[i + 2] = Math.sin(ctx.time * 3 + y * 0.07 + x * 0.07) * (4 + heat * 5);
        }
        cap.needsUpdate = true;
        model.capeMat.opacity = 0.24 + heat * 0.20

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
        model.capeMat.color.setHex(palette.hex);
        // Everything else keeps Trident's fixed palette -- see the file header.
    },
});
