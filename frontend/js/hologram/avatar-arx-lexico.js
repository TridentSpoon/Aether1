/* Avatar: A.R.X.LEXICO -- a plus-shaped cross of four real cubes orbiting a smaller,
 * brighter core cube at the centre, joined to it by four data-beam lines and trailed
 * by a small marker node past each arm. Two independent gyro rings, counter-rotating
 * in the picture plane, and a slow drifting dust field wrap the whole structure.
 *
 * The reference this riffs on drew its cross with flat isometric-shaded polygons --
 * a single 2D illusion of a cube, readable only from one fixed angle. Every "cube"
 * here is genuine THREE.BoxGeometry with its own edges, so the whole cross keeps
 * reading correctly (and shows real depth) from any angle the scene gets rotated to,
 * not just the one the drawing was posed for -- the same real-solid-over-flat-illusion
 * upgrade A.R.X.LOGOS's core went through.
 *
 * Loosely inspired by isometric cube-lattice / data-matrix motifs in general -- a
 * cross of cubes around a glowing core -- not a copy of any one specific design, and
 * redrawn from scratch in this engine's own flat-shape/unlit-material language
 * (MeshBasicMaterial, no scene lighting) the same way every other avatar here is hand
 * built; see js/hologram/core.js's note that only hAlcy's obsidian core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-lexico',
    label: 'A.R.X.LEXICO',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the core's colour, this avatar's one identity hue
        // that never retints, the same convention every other Umbral's hot accent follows.
        const CORE_HOT = 0x00ffff;

        const OUTER_SIZE = 16;
        const OUTER_DIST = 30;
        const NODE_DIST = 52;
        const CORE_SIZE = 11;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(150, 150, 1);
        glow.position.z = -12;
        group.add(glow);

        // Everything below (the cross, its beams and marker nodes, the core) shares one
        // slow tumble in animate() -- unlike the reference's flat, un-rotating cross,
        // letting real 3D geometry show its depth over time instead of sitting frozen
        // at one flattering angle.
        const spinGroup = new THREE.Group();
        group.add(spinGroup);

        // --- Four outer cubes in a plus arrangement around the centre. ---
        const outerFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.35, depthWrite: false });
        const outerWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.9 });
        const outerGeo = new THREE.BoxGeometry(OUTER_SIZE, OUTER_SIZE, OUTER_SIZE);
        const outerEdges = new THREE.EdgesGeometry(outerGeo);

        const axes = [
            { x: 0, y: OUTER_DIST },
            { x: 0, y: -OUTER_DIST },
            { x: -OUTER_DIST, y: 0 },
            { x: OUTER_DIST, y: 0 },
        ];
        const outerCubes = axes.map((pos, i) => {
            const cube = new THREE.Mesh(outerGeo, outerFillMat);
            cube.position.set(pos.x, pos.y, 0);
            spinGroup.add(cube);
            const wire = new THREE.LineSegments(outerEdges, outerWireMat);
            wire.position.copy(cube.position);
            spinGroup.add(wire);
            return { cube, wire, phase: i * 1.4 };
        });

        // --- Data-beam lines from the core out to each outer cube. ---
        const beamMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.7 });
        const beamPositions = new Float32Array(axes.length * 2 * 3);
        axes.forEach((pos, i) => {
            const o = i * 6;
            beamPositions[o] = 0; beamPositions[o + 1] = 0; beamPositions[o + 2] = 0;
            beamPositions[o + 3] = pos.x; beamPositions[o + 4] = pos.y; beamPositions[o + 5] = 0;
        });
        const beamGeo = new THREE.BufferGeometry();
        beamGeo.setAttribute('position', new THREE.BufferAttribute(beamPositions, 3));
        const beams = new THREE.LineSegments(beamGeo, beamMat);
        spinGroup.add(beams);

        // --- Small marker nodes trailing past each outer cube -- fixed hot colour, the
        // same identity accent as the core, so they read as sparks of the same energy. ---
        const nodeMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending });
        const nodeGeo = new THREE.SphereGeometry(2, 10, 10);
        const nodes = axes.map((pos, i) => {
            const scale = NODE_DIST / OUTER_DIST;
            const node = new THREE.Mesh(nodeGeo, nodeMat);
            node.position.set(pos.x * scale, pos.y * scale, 0);
            spinGroup.add(node);
            return { node, phase: i * 1.1 };
        });

        // --- Central core cube: the glowing aperture focal point, fixed hot colour. ---
        const coreFillMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending });
        const coreWireMat = new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 });
        const coreGeo = new THREE.BoxGeometry(CORE_SIZE, CORE_SIZE, CORE_SIZE);
        const coreCube = new THREE.Mesh(coreGeo, coreFillMat);
        spinGroup.add(coreCube);
        const coreWire = new THREE.LineSegments(new THREE.EdgesGeometry(coreGeo), coreWireMat);
        spinGroup.add(coreWire);

        const apertureMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95 });
        const aperture = new THREE.Mesh(new THREE.SphereGeometry(2.6, 12, 12), apertureMat);
        spinGroup.add(aperture);

        // A sprite sitting at spinGroup's own origin -- the core's position -- so its
        // billboard quad never has to move as spinGroup tumbles, only ever glow brighter
        // or dimmer in place, per the sprite-billboarding rule (only .rotation.z and
        // inherited .position respond to a parent's transform; a sprite at the rotation
        // origin has nowhere to move to anyway).
        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#e0ffff', '#00ffff'), transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = 32;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        spinGroup.add(coreGlow);

        // --- Two independent gyro rings, counter-rotating in the picture plane -- a
        // sprite's own .rotation.z is respected regardless of any ancestor's transform,
        // so animating it directly (rather than parenting into spinGroup) is what lets
        // each ring spin freely on its own axis instead of tumbling with the cross. ---
        const ringOuterMat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.035), color: api.palette.hex2, transparent: true, opacity: 0.45, depthWrite: false });
        const ringOuter = new THREE.Sprite(ringOuterMat);
        ringOuter.scale.set(118, 118, 1);
        group.add(ringOuter);

        const ringInnerMat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.06), color: api.palette.hex3, transparent: true, opacity: 0.6, depthWrite: false });
        const ringInner = new THREE.Sprite(ringInnerMat);
        ringInner.scale.set(96, 96, 1);
        group.add(ringInner);

        // --- A slow drifting dust of points, standing in for the reference's scanline
        // sweep -- ambient "data" texture around the structure. ---
        const dustCount = 60;
        const dustPositions = new Float32Array(dustCount * 3);
        for (let i = 0; i < dustCount * 3; i += 3) {
            const r = 65 + Math.random() * 35;
            const theta = Math.random() * Math.PI * 2;
            const phi = Math.acos(Math.random() * 2 - 1);
            dustPositions[i] = r * Math.sin(phi) * Math.cos(theta);
            dustPositions[i + 1] = r * Math.cos(phi) * 0.6;
            dustPositions[i + 2] = r * Math.sin(phi) * Math.sin(theta);
        }
        const dustGeo = new THREE.BufferGeometry();
        dustGeo.setAttribute('position', new THREE.BufferAttribute(dustPositions, 3));
        const dustMat = new THREE.PointsMaterial({ color: api.palette.hex, size: 1.4, transparent: true, opacity: 0.5 });
        const dust = new THREE.Points(dustGeo, dustMat);
        group.add(dust);

        return {
            group, spinGroup, dust,
            glowMat, outerFillMat, outerWireMat, outerCubes,
            beamMat, nodeMat, nodes,
            coreFillMat, coreWireMat, coreGlow, coreGlowMat, coreGlowScale, apertureMat,
            ringOuter, ringOuterMat, ringInner, ringInnerMat, dustMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 1.5) * 3;
        model.spinGroup.rotation.y = ctx.time * 0.12;
        model.spinGroup.rotation.x = Math.sin(ctx.time * 0.4) * 0.15;
        model.dust.rotation.y = ctx.time * 0.05;

        // Two gyro rings, counter-rotating at different speeds -- echoing the reference's
        // dashed/solid rings without literally copying their timing.
        model.ringOuter.rotation.z = ctx.time * 0.15;
        model.ringInner.rotation.z = -ctx.time * 0.22;

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 3) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.35 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.outerFillMat.opacity = 0.28 + pulse * 0.22;
        model.outerWireMat.opacity = 0.7 + pulse * 0.2 + ctx.click * 0.1;
        model.outerCubes.forEach(({ cube, phase }) => {
            const s = 1 + Math.sin(ctx.time * 2 + phase) * 0.05;
            cube.scale.set(s, s, s);
        });

        model.beamMat.opacity = 0.5 + pulse * 0.4 + ctx.click * 0.1;
        model.nodes.forEach(({ node, phase }) => {
            const s = 1 + Math.sin(ctx.time * 3 + phase) * 0.35;
            node.scale.set(s, s, s);
        });

        model.coreFillMat.opacity = Math.min(1, 0.65 + pulse * 0.3 + ctx.click * 0.15);
        model.coreWireMat.opacity = 0.75 + ctx.click * 0.25;
        model.apertureMat.opacity = 0.85 + ctx.click * 0.15;
        model.coreGlowMat.opacity = Math.min(1, 0.35 + pulse * 0.45);
        const coreScale = 1 + pulse * 0.15 + ctx.click * 0.25;
        model.coreGlow.scale.set(model.coreGlowScale * coreScale, model.coreGlowScale * coreScale, 1);

        model.glowMat.opacity = 0.13 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.outerFillMat.color.setHex(palette.hex);
        model.outerWireMat.color.setHex(palette.hex2);
        model.beamMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        model.ringOuterMat.color.setHex(palette.hex2);
        model.ringInnerMat.color.setHex(palette.hex3);
        model.dustMat.color.setHex(palette.hex);
        // The core cube, its wireframe, aperture and marker nodes stay fixed, this
        // avatar's one hot accent that never retints.
    },
});
