/* Avatar: A.R.X.LUCRE -- a flattened diamond shell lying across the XZ plane with four
 * cubes stacked along the vertical axis above and below it, all rotated to the
 * diamond's 45-degree alignment, joined by a dashed vertical data beam running the
 * full height and a small glowing amber core-aperture sitting at the very centre. Two
 * tilted halo rings tumble independently around the whole stack, and a slow amber dust
 * field drifts past it.
 *
 * The reference this riffs on gave its halo rings a true horizontal lie (flat across
 * the ground plane) for an orbit-controlled camera that could tilt down to see them
 * face-on. This engine's camera sits fixed, straight-on, so a ring laid perfectly flat
 * would render edge-on as an invisible line; tilting them partway instead (as real
 * THREE.RingGeometry mesh, not a camera-facing sprite -- the same tilted-and-tumbling
 * real-geometry approach A.R.X.LOGOS's shell rings use) keeps them visible as halo
 * bands from a static front view while still reading as rings orbiting a vertical
 * column, the same idea the reference was going for.
 *
 * Loosely inspired by vertically-stacked geometric index/core motifs in general -- a
 * flattened diamond core with cubes stacked along one axis -- not a copy of any one
 * specific design, and redrawn from scratch in this engine's own flat-shape/unlit-
 * material language (MeshBasicMaterial, no scene lighting) the same way every other
 * avatar here is hand built; see js/hologram/core.js's note that only hAlcy's obsidian
 * core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-lucre',
    label: 'A.R.X.LUCRE',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the core-aperture's amber glow, this avatar's one
        // identity colour that never retints, the same convention every other Umbral's
        // hot accent follows.
        const CORE_HOT = 0xffcc00;

        const DIAMOND_RADIUS = 22;
        const CUBE_BASE = 8;
        const CUBE_Y = [30, 16.5, -16.5, -30];
        const CUBE_SCALE = [0.65, 0.85, 0.85, 0.65];
        const CORE_RADIUS = 9;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(150, 150, 1);
        glow.position.z = -14;
        group.add(glow);

        // The diamond, cube stack, core and beam all share one slow tumble in animate()
        // -- the halo rings live outside this group so they can keep their own
        // independent spin instead of tumbling with the rest.
        const spinGroup = new THREE.Group();
        group.add(spinGroup);

        // --- Main flattened diamond, laid across the XZ plane. Translucent shell fill
        // plus a bright wireframe, the same layered-shell convention every other
        // Umbral's shell uses (depthWrite: false so the translucent fill never punches
        // self-occluding holes in its own facets). ---
        const shellFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.22, depthWrite: false });
        const diamondWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.9 });

        const diamondGeo = new THREE.OctahedronGeometry(DIAMOND_RADIUS, 0);
        diamondGeo.scale(1.5, 0.45, 1.5);
        const diamondMesh = new THREE.Mesh(diamondGeo, shellFillMat);
        spinGroup.add(diamondMesh);
        const diamondWire = new THREE.LineSegments(new THREE.EdgesGeometry(diamondGeo), diamondWireMat);
        spinGroup.add(diamondWire);

        // --- Four cubes stacked along Y, rotated 45 degrees to the diamond's own
        // alignment, alternating wire colour between hex2/hex3 for a bit of variety. ---
        const cubeWireMatA = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });
        const cubeWireMatB = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.85 });
        const cubeGeo = new THREE.BoxGeometry(CUBE_BASE, CUBE_BASE, CUBE_BASE);
        const cubeEdges = new THREE.EdgesGeometry(cubeGeo);

        const yCubes = CUBE_Y.map((yPos, i) => {
            const s = CUBE_SCALE[i];
            const cube = new THREE.Mesh(cubeGeo, shellFillMat);
            cube.scale.set(s, s, s);
            cube.position.set(0, yPos, 0);
            cube.rotation.y = Math.PI / 4;
            spinGroup.add(cube);
            const wire = new THREE.LineSegments(cubeEdges, i % 2 === 0 ? cubeWireMatA : cubeWireMatB);
            wire.scale.copy(cube.scale);
            wire.position.copy(cube.position);
            wire.rotation.copy(cube.rotation);
            spinGroup.add(wire);
            return { cube, wire, baseY: yPos, phase: (i + 1) * 0.5 };
        });

        // --- Central glowing amber core-aperture, fixed hot colour, at the exact
        // centre where the diamond and beam both meet. ---
        const coreFillMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending });
        const coreWireMat = new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 });
        const coreGeo = new THREE.OctahedronGeometry(CORE_RADIUS, 0);
        coreGeo.scale(1.1, 0.5, 1.1);
        const coreMesh = new THREE.Mesh(coreGeo, coreFillMat);
        spinGroup.add(coreMesh);
        const coreWire = new THREE.LineSegments(new THREE.EdgesGeometry(coreGeo), coreWireMat);
        spinGroup.add(coreWire);

        const pupilMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95 });
        const pupil = new THREE.Mesh(new THREE.SphereGeometry(2.4, 14, 14), pupilMat);
        spinGroup.add(pupil);

        // A sprite sitting at spinGroup's own origin -- the core's position -- so its
        // billboard quad never has to move as spinGroup tumbles, only ever glow
        // brighter or dimmer in place (see A.R.X.LEXICO for the same reasoning).
        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#fff2c0', '#ffcc00'), transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = 30;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        spinGroup.add(coreGlow);

        // --- A dashed vertical data beam running the full height of the stack. ---
        const beamMat = new THREE.LineDashedMaterial({ color: CORE_HOT, transparent: true, opacity: 0.75, dashSize: 3, gapSize: 1.8 });
        const beamGeo = new THREE.BufferGeometry().setFromPoints([
            new THREE.Vector3(0, -34, 0),
            new THREE.Vector3(0, 34, 0),
        ]);
        const beam = new THREE.Line(beamGeo, beamMat);
        beam.computeLineDistances();
        spinGroup.add(beam);

        // --- Two halo rings, tilted partway (not laid flat -- see file header) and
        // each tumbling at its own independent rate, outside spinGroup so their spin
        // never locks to the rest of the assembly's slower turn. ---
        const ringConfigs = [
            { inner: 44, outer: 45.4, tiltX: 1.15, tiltY: 0.1, color: api.palette.hex2, opacity: 0.4, speed: { x: 0.0, y: 0.18, z: 0.0 } },
            { inner: 50, outer: 50.9, tiltX: 1.3, tiltY: -0.15, color: api.palette.hex3, opacity: 0.28, speed: { x: 0.0, y: -0.24, z: 0.0 } },
        ];
        const rings = ringConfigs.map((cfg) => {
            const mat = new THREE.MeshBasicMaterial({ color: cfg.color, side: THREE.DoubleSide, transparent: true, opacity: cfg.opacity, depthWrite: false });
            const ring = new THREE.Mesh(new THREE.RingGeometry(cfg.inner, cfg.outer, 64), mat);
            ring.rotation.x = cfg.tiltX;
            ring.rotation.y = cfg.tiltY;
            ring.userData = { speed: cfg.speed };
            group.add(ring);
            return { ring, mat };
        });

        // --- A slow drifting amber dust field. ---
        const dustCount = 70;
        const dustPositions = new Float32Array(dustCount * 3);
        for (let i = 0; i < dustCount * 3; i += 3) {
            const r = 55 + Math.random() * 30;
            const theta = Math.random() * Math.PI * 2;
            const phi = Math.acos(Math.random() * 2 - 1);
            dustPositions[i] = r * Math.sin(phi) * Math.cos(theta);
            dustPositions[i + 1] = r * Math.cos(phi);
            dustPositions[i + 2] = r * Math.sin(phi) * Math.sin(theta);
        }
        const dustGeo = new THREE.BufferGeometry();
        dustGeo.setAttribute('position', new THREE.BufferAttribute(dustPositions, 3));
        const dustMat = new THREE.PointsMaterial({ color: api.palette.hex, size: 1.4, transparent: true, opacity: 0.55 });
        const dust = new THREE.Points(dustGeo, dustMat);
        group.add(dust);

        return {
            group, spinGroup, dust,
            glowMat, shellFillMat, diamondWireMat, cubeWireMatA, cubeWireMatB, yCubes,
            coreMesh, coreWire, coreFillMat, coreWireMat, coreGlow, coreGlowMat, coreGlowScale, pupilMat,
            beamMat, rings, dustMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 1.5) * 3;
        model.spinGroup.rotation.y = ctx.time * 0.15;
        model.dust.rotation.y = ctx.time * 0.05;

        // Halo rings tumble independently of the rest of the assembly.
        model.rings.forEach(({ ring }) => {
            ring.rotation.x += ring.userData.speed.x;
            ring.rotation.y += ring.userData.speed.y * 0.016;
            ring.rotation.z += ring.userData.speed.z;
        });

        // Cubes float and gently rock around the diamond's 45-degree alignment.
        model.yCubes.forEach(({ cube, wire, baseY, phase }) => {
            const y = baseY + Math.sin(ctx.time * 2 + phase) * 1.2;
            const rotY = Math.PI / 4 + Math.sin(ctx.time + phase) * 0.2;
            cube.position.y = y;
            cube.rotation.y = rotY;
            wire.position.y = y;
            wire.rotation.y = rotY;
        });

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 3) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.35 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.diamondWireMat.opacity = 0.75 + pulse * 0.2 + ctx.click * 0.1;
        model.cubeWireMatA.opacity = 0.7 + pulse * 0.2;
        model.cubeWireMatB.opacity = 0.7 + pulse * 0.2;
        model.beamMat.opacity = 0.55 + pulse * 0.35 + ctx.click * 0.1;

        model.coreFillMat.opacity = Math.min(1, 0.65 + pulse * 0.3 + ctx.click * 0.15);
        model.coreWireMat.opacity = 0.75 + ctx.click * 0.25;
        model.pupilMat.opacity = 0.85 + ctx.click * 0.15;
        const coreBreathe = 1 + Math.sin(ctx.time * 2) * 0.06 + ctx.click * 0.12;
        model.coreMesh.scale.set(coreBreathe, coreBreathe, coreBreathe);
        model.coreWire.scale.copy(model.coreMesh.scale);
        model.coreGlowMat.opacity = Math.min(1, 0.35 + pulse * 0.45);
        const coreScale = 1 + pulse * 0.15 + ctx.click * 0.25;
        model.coreGlow.scale.set(model.coreGlowScale * coreScale, model.coreGlowScale * coreScale, 1);

        model.glowMat.opacity = 0.13 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.shellFillMat.color.setHex(palette.hex);
        model.diamondWireMat.color.setHex(palette.hex2);
        model.cubeWireMatA.color.setHex(palette.hex2);
        model.cubeWireMatB.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        model.rings[0].mat.color.setHex(palette.hex2);
        model.rings[1].mat.color.setHex(palette.hex3);
        model.dustMat.color.setHex(palette.hex);
        // The core, its wireframe, pupil and vertical beam stay fixed, this avatar's
        // one hot amber accent that never retints.
    },
});
