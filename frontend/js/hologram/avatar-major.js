/* Avatar: m.A.J.o.r. -- Modular Assembly, Judgement, Orchestration and Records. The
 * eXcelsior Class's seventh, and the only one that takes itself apart.
 *
 * Trident's sketch for this one was a wireframe heart inside three hoops turning on
 * different axes, inside a shell of drifting points. That silhouette is kept whole here --
 * the hoop radii, their six rotation rates and the shell's inner and outer bounds are his
 * numbers scaled into the panel. What is added is the thing the rest of the line does not
 * do: the heart is wrapped in six plates that open.
 *
 * Idle it is shut, one quiet closed form breathing on his pulse, with only the seams
 * between the plates to say it has an inside. Thinking, it comes apart: the plates travel
 * out along their own axes, callout leaders snap from each one to an anchor out in the
 * shell, and dimension lines draw themselves across the gaps that have opened. Speaking,
 * the ticks run outward along the leaders like a line being read off. Then it shuts again.
 *
 * Two things are worth knowing before editing it.
 *
 * Each plate's travel direction is read from its own geometry's bounding box rather than
 * worked out from the angles it was built at. Nothing then depends on remembering which
 * way three.js lays a sphere's phi out, and a plate can be re-cut without a plate flying
 * inward.
 *
 * The annotation -- leaders, ticks, dimension lines -- keeps the one fixed colour his
 * outermost hoop had, while the machine itself takes the theme (see applyPalette). A
 * measurement that changed colour with the furniture would read as part of the furniture.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'major',
    label: 'm.A.J.o.r.',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const HEART = 13;           // the wireframe sphere at the middle
        const PLATE = 31;           // the radius the plates sit on when shut
        const SPREAD = 21;          // how far out a plate travels, fully open
        const SHELL_IN = 48;        // the point shell's inner and outer bounds
        const SHELL_OUT = 84;
        const POINTS = 520;
        const PLATES = 6;
        // Leaders, ticks and the dimension lines across the gaps, in one buffer: six
        // leaders, six ticks and four dimension lines is sixteen segments, and the count
        // never changes, so the buffer is sized once and only its contents are rewritten.
        const SEGMENTS = PLATES + PLATES + 4;
        /* Fixed, not theme-tinted: his outermost hoop was amber against two blues, and
           that is the colour the annotation keeps whatever theme the HUD is wearing. */
        const ANNOTATION = 0xffaa00;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.12, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(220, 220, 1);
        glow.position.z = -60;
        group.add(glow);

        // --- The heart: his wireframe sphere, on his idle pulse. ---
        const heartMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, wireframe: true,
            transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending,
        });
        const heart = new THREE.Mesh(new THREE.SphereGeometry(HEART, 14, 10), heartMat);
        group.add(heart);

        /* The six plates. A cap top and bottom and four around the middle, each one a patch
           of the same sphere with a gap left around it -- shut, the seams are the only sign
           there is anything inside. */
        const plateMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.16,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const edgeMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
        });
        const GAP = 0.2;
        const cuts = [
            { phi: 0, phiLen: Math.PI * 2, theta: 0, thetaLen: 0.55 },
            { phi: 0, phiLen: Math.PI * 2, theta: Math.PI - 0.55, thetaLen: 0.55 },
        ];
        for (let i = 0; i < 4; i++) {
            const step = (Math.PI * 2) / 4;
            cuts.push({
                phi: i * step + GAP, phiLen: step - GAP * 2,
                theta: 0.55 + GAP, thetaLen: Math.PI - 1.1 - GAP * 2,
            });
        }
        const plates = cuts.map((cut) => {
            const geom = new THREE.SphereGeometry(
                PLATE, 14, 8, cut.phi, cut.phiLen, cut.theta, cut.thetaLen
            );
            /* Which way this plate travels, read off the geometry it actually has rather
               than re-derived from the angles above -- see the note in the file header. */
            geom.computeBoundingBox();
            const dir = new THREE.Vector3();
            geom.boundingBox.getCenter(dir);
            // A full band's box is centred on the axis, so it has no sideways direction of
            // its own: those plates travel straight out from the middle of the sphere.
            if (dir.length() < 0.001) dir.set(0, 1, 0);
            dir.normalize();
            const mesh = new THREE.Mesh(geom, plateMat);
            /* Outline only. EdgesGeometry keeps every edge whose two faces meet at more
               than the threshold angle, plus every edge with only one face -- and a sphere
               patch at this tessellation bends by around twenty degrees a quad, so the
               default one degree returns the whole mesh and the plate reads as a wireframe
               ball rather than a panel. Forty leaves the border, which is the plate. */
            mesh.add(new THREE.LineSegments(new THREE.EdgesGeometry(geom, 40), edgeMat));
            group.add(mesh);
            return { mesh, dir };
        });

        /* His three hoops, on his radii and his six rotation rates. They are the cradle the
           assembly hangs in: nothing else in the avatar turns on more than one axis, and
           the three of them disagreeing is what keeps a shut m.A.J.o.r. from reading as a
           ball sitting still. */
        const hoopMats = [
            new THREE.MeshBasicMaterial({ color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending }),
            new THREE.MeshBasicMaterial({ color: api.palette.hex2, wireframe: true, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending }),
            new THREE.MeshBasicMaterial({ color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending }),
        ];
        const hoops = [
            new THREE.Mesh(new THREE.TorusGeometry(48, 0.7, 5, 64), hoopMats[0]),
            new THREE.Mesh(new THREE.TorusGeometry(66, 0.5, 5, 72), hoopMats[1]),
            new THREE.Mesh(new THREE.TorusGeometry(86, 0.35, 5, 80), hoopMats[2]),
        ];
        hoops.forEach((h) => group.add(h));

        // --- His point shell, thinned out: the volume the readings are taken from. ---
        const shellPos = new Float32Array(POINTS * 3);
        for (let i = 0; i < POINTS; i++) {
            const r = SHELL_IN + Math.random() * (SHELL_OUT - SHELL_IN);
            const theta = Math.random() * Math.PI * 2;
            // Drawn from the arc cosine of a uniform number so the points spread evenly
            // over the sphere instead of bunching at its poles.
            const phi = Math.acos(Math.random() * 2 - 1);
            shellPos[i * 3] = r * Math.sin(phi) * Math.cos(theta);
            shellPos[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
            shellPos[i * 3 + 2] = r * Math.cos(phi);
        }
        const shellGeom = new THREE.BufferGeometry();
        shellGeom.setAttribute('position', new THREE.BufferAttribute(shellPos, 3));
        // Mapped, not bare: an unmapped point sprite is a hard square.
        const shellMat = new THREE.PointsMaterial({
            map: api.helpers.glowTexture(32), color: api.palette.hex2, size: 3.2,
            transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        const shell = new THREE.Points(shellGeom, shellMat);
        group.add(shell);

        /* Where each plate's leader ends: a point fixed out in the shell, roughly along the
           plate's own direction so the leaders splay rather than cross. Fixed at build,
           because a callout that wandered would be an animation, not a measurement. */
        const anchors = plates.map(({ dir }, i) => {
            const spin = (i / PLATES) * Math.PI * 2;
            return dir.clone().multiplyScalar(76).add(
                new THREE.Vector3(Math.cos(spin) * 11, Math.sin(spin) * 11, Math.sin(spin * 2) * 7)
            );
        });

        const annGeom = new THREE.BufferGeometry();
        const annPos = new Float32Array(SEGMENTS * 6);
        annGeom.setAttribute('position', new THREE.BufferAttribute(annPos, 3));
        annGeom.setDrawRange(0, 0);
        const annMat = new THREE.LineBasicMaterial({
            color: ANNOTATION, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
        });
        group.add(new THREE.LineSegments(annGeom, annMat));

        return {
            group, glow, glowMat, heart, heartMat, plates, plateMat, edgeMat, hoops, hoopMats,
            shell, shellMat, annGeom, annPos, annMat, anchors,
            plateRadius: PLATE, spread: SPREAD, segments: SEGMENTS,
            open: 0,            // 0 shut, 1 fully apart; eased toward its target each frame
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        /* How far apart it should be. Thinking is the working state, so it is wide open;
           speaking opens it as far as the voice is loud; listening pulls it shut and holds
           it there, which is what attention looks like on a thing made of parts. */
        const target = isThinking ? 1
            : isSpeaking ? 0.35 + ctx.audio * 0.5
                : isListening ? 0 : ctx.click * 0.5;
        // Eased, never set: the whole point of this avatar is the travel between the two
        // states, and a plate that arrives in one frame has not opened, it has teleported.
        model.open += (target - model.open) * 0.07;
        const open = model.open;

        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.55 : 0;

        // His idle pulse on the heart, brighter the further the plates are out of its way.
        const pulse = 1 + Math.sin(ctx.time * 3) * 0.08;
        model.heart.scale.setScalar(pulse);
        model.heartMat.opacity = 0.36 + open * 0.34 + heat * 0.2;

        for (let i = 0; i < model.plates.length; i++) {
            const { mesh, dir } = model.plates[i];
            // Staggered, so the assembly unpacks rather than bursting: each plate leaves a
            // little after the one before it.
            const lag = Math.max(0, Math.min(1, open * 1.6 - i * 0.1));
            mesh.position.copy(dir).multiplyScalar(lag * model.spread);
        }
        model.plateMat.opacity = 0.16 - open * 0.04 + heat * 0.1;
        model.edgeMat.opacity = 0.95 - open * 0.05 + ctx.click * 0.12;

        // His six rates, taken faster while it is working.
        const spin = isThinking ? 1.9 : isListening ? 0.45 : 1;
        const t = ctx.time * spin;
        model.hoops[0].rotation.set(t * 0.5, t * 0.3, 0);
        model.hoops[1].rotation.set(0, -t * 0.4, t * 0.2);
        model.hoops[2].rotation.set(-t * 0.2, 0, -t * 0.6);
        model.hoopMats[0].opacity = 0.5 + heat * 0.22;
        model.hoopMats[1].opacity = 0.42 + heat * 0.2;
        model.hoopMats[2].opacity = 0.32 + heat * 0.16;

        model.shell.rotation.y = ctx.time * 0.05;
        model.shellMat.opacity = 0.55 + heat * 0.3 + ctx.click * 0.15;

        /* The annotation. Drawn only once there is a gap worth measuring, which is why the
           draw range is set from nothing rather than the lines being hidden: shut, there is
           no geometry to skip. */
        if (open < 0.06) {
            model.annGeom.setDrawRange(0, 0);
            model.annMat.opacity = 0;
        } else {
            const pos = model.annPos;
            let n = 0;
            const put = (a, b) => {
                const at = n * 6;
                pos[at] = a.x; pos[at + 1] = a.y; pos[at + 2] = a.z;
                pos[at + 3] = b.x; pos[at + 4] = b.y; pos[at + 5] = b.z;
                n++;
            };
            const from = new THREE.Vector3();
            const tick = new THREE.Vector3();
            for (let i = 0; i < model.plates.length; i++) {
                const { mesh, dir } = model.plates[i];
                // Leaves the plate's outer face, not its centre, so the leader starts on
                // the surface it is calling out.
                from.copy(dir).multiplyScalar(model.plateRadius).add(mesh.position);
                const anchor = model.anchors[i];
                put(from, anchor);
                /* The tick. At rest it sits on the anchor; speaking, it runs the length of
                   the leader, one plate after another -- a line being read off rather than
                   six lights blinking together. */
                const u = isSpeaking ? (ctx.time * 0.6 + i * 0.16) % 1 : 1;
                tick.lerpVectors(from, anchor, u);
                put(
                    tick.clone().add(new THREE.Vector3(-5, 3, 0)),
                    tick.clone().add(new THREE.Vector3(5, -3, 0))
                );
            }
            // The four dimension lines, across the gaps the middle plates have opened.
            for (let i = 0; i < 4; i++) {
                const a = model.plates[2 + i];
                const b = model.plates[2 + ((i + 1) % 4)];
                put(
                    from.copy(a.dir).multiplyScalar(model.plateRadius).add(a.mesh.position).clone(),
                    tick.copy(b.dir).multiplyScalar(model.plateRadius).add(b.mesh.position).clone()
                );
            }
            model.annGeom.setDrawRange(0, n * 2);
            model.annGeom.attributes.position.needsUpdate = true;
            model.annMat.opacity = Math.min(0.9, (open - 0.06) * 1.6) * (0.7 + heat * 0.3);
        }

        model.glowMat.opacity = 0.1 + (isSpeaking ? ctx.audio * 0.18 : Math.sin(ctx.time * 1.2) * 0.03 + 0.03);
    },

    applyPalette(model, palette) {
        model.heartMat.color.setHex(palette.hex2);
        model.plateMat.color.setHex(palette.hex);
        model.edgeMat.color.setHex(palette.hex);
        model.hoopMats[0].color.setHex(palette.hex);
        model.hoopMats[1].color.setHex(palette.hex2);
        model.hoopMats[2].color.setHex(palette.hex);
        model.shellMat.color.setHex(palette.hex2);
        model.glowMat.color.setHex(palette.hex);
        // The annotation keeps its fixed colour -- see build().
    },
});
