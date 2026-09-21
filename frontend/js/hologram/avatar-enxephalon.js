/* Avatar: enXephalon -- the first of The eXcelsior Class: a mind-scanning instrument
 * rather than a face. A shell of thought-points hangs in the middle, wired to its
 * neighbours by a lattice of synapses, inside a wireframe chamber, with two thin rings
 * sweeping it on crossed axes like a scan in progress. A scattering of the points are
 * signatures: they sit brighter than the rest and flare when the scan passes them.
 *
 * Built as an instrument rather than a head, because the thing it borrows from is a
 * room you sit in to listen to other minds, not a creature. The chamber is the room,
 * the point shell is what it is looking at, and the rings are the looking.
 *
 * Nothing here recomputes the lattice per frame. The points never move relative to each
 * other, so which of them are neighbours is settled once at build time and only the
 * brightness changes afterwards -- the difference between a fixed cost at startup and
 * a quarter of a million distance checks every frame for an answer that cannot change.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'enxephalon',
    label: 'enXephalon',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        // The scan rings and the signature points among the cloud share one hot colour
        // that never retints -- the same "hot accents stay fixed" convention R.E.D. 9000's
        // lens and A.R.X.LOCAS's core follow. It is what makes a signature read as a
        // signature: the rest of the shell is whatever theme is on, and these are not.
        const SIGNATURE = 0xff2fb0;

        const MIND_RADIUS = 38;     // the shell the thought-points sit on
        const MIND_JITTER = 3.6;    // how far off that shell each one strays
        const MIND_COUNT = 700;
        const DOME_RADIUS = 62;     // the chamber around the whole thing
        const SCAN_RADIUS = 47;     // the two sweeping rings, clear of the shell
        const LINK_DISTANCE = 8.5;  // neighbours closer than this get a synapse
        const MAX_LINKS = 1400;     // a ceiling, so a dense draw cannot run away

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(170, 170, 1);
        glow.position.z = -20;
        group.add(glow);

        // --- The chamber: a faint wireframe shell, turning against everything inside
        // it so the two never read as one body. ---
        const domeMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3, wireframe: true, transparent: true, opacity: 0.16,
        });
        const dome = new THREE.Mesh(new THREE.IcosahedronGeometry(DOME_RADIUS, 2), domeMat);
        group.add(dome);

        // Everything the scan is actually looking at turns together, inside the chamber.
        const mindGroup = new THREE.Group();
        group.add(mindGroup);

        // --- The thought-points. Spread evenly over the sphere by inverting the area
        // distribution (acos of a uniform value, not a uniform angle -- that one bunches
        // everything at the poles), each nudged off the shell so the surface reads as a
        // cloud with thickness instead of a skin.
        //
        // Three kinds: the body of ordinary thought, a scattering of brighter ones, and
        // the few signatures the scan is hunting for. Which kind a point is gets kept, not
        // its colour, so a theme change recolours the shell without disturbing where
        // anything sits -- and so the signatures can sit out the theme entirely. ---
        const CHANNELS = ['hex', 'hex3', null];
        const positions = new Float32Array(MIND_COUNT * 3);
        const colors = new Float32Array(MIND_COUNT * 3);
        const points = [];

        for (let i = 0; i < MIND_COUNT; i++) {
            const theta = Math.random() * Math.PI * 2;
            const phi = Math.acos(2 * Math.random() - 1);
            const r = MIND_RADIUS + (Math.random() - 0.5) * MIND_JITTER * 2;
            const x = r * Math.sin(phi) * Math.cos(theta);
            const y = r * Math.sin(phi) * Math.sin(theta);
            const z = r * Math.cos(phi);
            positions[i * 3] = x;
            positions[i * 3 + 1] = y;
            positions[i * 3 + 2] = z;

            const roll = Math.random();
            const channel = roll > 0.88 ? 2 : roll > 0.62 ? 1 : 0;
            points.push({
                x, y, z, channel,
                // A signature flares on its own phase; an ordinary point just breathes.
                phase: Math.random() * Math.PI * 2,
                rate: channel === 2 ? 1.4 + Math.random() * 0.8 : 0.5 + Math.random() * 0.5,
            });
        }

        const mindGeom = new THREE.BufferGeometry();
        mindGeom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        mindGeom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
        // Mapped, not bare: an unmapped point sprite is a hard square, and a shell of
        // several hundred of them reads as pixel noise rather than as nodes. A soft round
        // falloff is what makes them look like something glowing.
        const mindMat = new THREE.PointsMaterial({
            map: api.helpers.glowTexture(32), size: 4.2, vertexColors: true,
            transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        const mindCloud = new THREE.Points(mindGeom, mindMat);
        mindGroup.add(mindCloud);

        // --- The synapses: every close pair, wired once. Squared distances, because a
        // square root per pair buys nothing when the only question is which side of a
        // threshold the pair falls on. ---
        const linkPositions = [];
        const linkLimit = LINK_DISTANCE * LINK_DISTANCE;
        for (let i = 0; i < MIND_COUNT && linkPositions.length < MAX_LINKS * 6; i++) {
            for (let j = i + 1; j < MIND_COUNT; j++) {
                const dx = points[i].x - points[j].x;
                const dy = points[i].y - points[j].y;
                const dz = points[i].z - points[j].z;
                if (dx * dx + dy * dy + dz * dz > linkLimit) continue;
                linkPositions.push(points[i].x, points[i].y, points[i].z);
                linkPositions.push(points[j].x, points[j].y, points[j].z);
                if (linkPositions.length >= MAX_LINKS * 6) break;
            }
        }
        const linkMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.3, blending: THREE.AdditiveBlending,
        });
        const linkGeom = new THREE.BufferGeometry();
        linkGeom.setAttribute('position', new THREE.Float32BufferAttribute(linkPositions, 3));
        const synapses = new THREE.LineSegments(linkGeom, linkMat);
        mindGroup.add(synapses);

        // --- The scan: two thin rings on crossed axes, sweeping the shell. Their own
        // group, outside mindGroup, so the scan is something done *to* the shell rather
        // than something carried around by it. ---
        const scanMat = new THREE.MeshBasicMaterial({
            color: SIGNATURE, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
        });
        const scanGeom = new THREE.TorusGeometry(SCAN_RADIUS, 1.1, 8, 96);
        const scanRingA = new THREE.Mesh(scanGeom, scanMat);
        const scanRingB = new THREE.Mesh(scanGeom, scanMat);
        group.add(scanRingA, scanRingB);

        return {
            group, mindGroup, dome, domeMat, mindCloud, mindMat, mindGeom, colors, points,
            synapses, linkMat, scanRingA, scanRingB, scanMat, glow, glowMat,
            channels: CHANNELS,
            // Recoloured in place on a theme change; the per-frame brightness below is
            // applied on top of these, so the two never have to agree in advance.
            base: CHANNELS.map((channel) => new THREE.Color(channel ? api.palette[channel] : SIGNATURE)),
            mindRadius: MIND_RADIUS,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        model.group.position.y = Math.sin(ctx.time * 0.6) * 3;

        // The shell and its chamber turn against each other, slowly -- this is an
        // instrument at work, not a thing showing off.
        model.mindGroup.rotation.y = ctx.time * 0.05;
        model.dome.rotation.y = -ctx.time * 0.02;
        model.dome.rotation.x = Math.sin(ctx.time * 0.11) * 0.06;

        // The scan runs faster when there is something to think about, and a click sends
        // it round hard once.
        const scanSpeed = (isThinking ? 2.4 : isListening ? 1.4 : 1) + ctx.click * 2.5;
        const t = ctx.time * scanSpeed;
        model.scanRingA.rotation.x = t * 0.4;
        model.scanRingA.rotation.y = t * 0.2;
        model.scanRingB.rotation.y = -t * 0.3;
        model.scanRingB.rotation.z = t * 0.5;
        model.scanMat.opacity = 0.45 + Math.abs(Math.sin(t * 0.8)) * 0.35 + ctx.click * 0.2;

        // A click pushes the whole shell outward for a moment, like a pulse going out
        // through it; speech swells it with the voice.
        model.mindGroup.scale.setScalar(1 + ctx.audio * 0.08 + ctx.click * 0.12);

        /* Per-point brightness. Signatures (channel 2) flare on their own phase and ride
           the voice; everything else just breathes. Written straight into the colour
           buffer rather than as a second attribute, because one buffer the GPU already
           reads is cheaper than a shader that would have to combine two. */
        const { colors, points, base } = model;
        const voice = isSpeaking ? ctx.audio : 0;
        const think = isThinking ? 0.25 : 0;
        for (let i = 0; i < points.length; i++) {
            const p = points[i];
            const wave = Math.sin(ctx.time * p.rate + p.phase) * 0.5 + 0.5;
            const level = p.channel === 2
                ? 0.55 + wave * 0.45 + voice * 0.5 + think
                : 0.52 + wave * 0.22 + voice * 0.2 + think * 0.5;
            const c = base[p.channel];
            const b = Math.min(1.4, level);
            colors[i * 3] = c.r * b;
            colors[i * 3 + 1] = c.g * b;
            colors[i * 3 + 2] = c.b * b;
        }
        model.mindGeom.attributes.color.needsUpdate = true;

        // The lattice brightens while it is being used, and the chamber with it.
        model.linkMat.opacity = 0.26 + (isThinking ? Math.abs(Math.sin(ctx.time * 4)) * 0.2 : voice * 0.22) + ctx.click * 0.2;
        model.domeMat.opacity = 0.12 + (isListening ? Math.sin(ctx.time * 1.6) * 0.04 + 0.06 : 0.04) + ctx.click * 0.1;
        model.glowMat.opacity = 0.14 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.3) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.channels.forEach((channel, index) => {
            if (channel) model.base[index].setHex(palette[channel]);
        });
        model.domeMat.color.setHex(palette.hex3);
        model.linkMat.color.setHex(palette.hex);
        model.glowMat.color.setHex(palette.hex);
        // The point colours themselves are rewritten by animate() on the next frame from
        // the base colours just set, so there is nothing to repaint here. The scan rings
        // and the signature points keep their fixed hot colour through a theme change.
    },
});
