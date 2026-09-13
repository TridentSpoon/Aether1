/* Avatar: A.R.X.LYKSAUM -- a flat circular HUD medallion rather than a spinning solid: a
 * translucent disc plate with a glowing rim, carrying a broken-ring glyph on its face (a
 * wide collar arc below, two shorter shoulder arcs above, with gaps between them) around a
 * small bright core node. Two thin rings orbit the disc in opposite directions, and a soft
 * scan bar sweeps up and down across the face. The whole thing only wobbles gently in place
 * -- it never turns away from the viewer, since the medallion's whole point is the glyph
 * printed on its face, and hiding that would defeat the design the same way an early build
 * of A.R.X.LOREGENDA once hid its own face behind a rotating shell (see that file's history).
 *
 * Loosely inspired by circular sci-fi HUD-medallion / broken-ring insignia motifs in
 * general -- a disc with a glowing cutout glyph and orbiting rings -- not a copy of any one
 * specific design, and redrawn from scratch in this engine's own flat-shape/unlit-material
 * language (MeshBasicMaterial, no scene lighting) the same way every other avatar here is
 * hand built; see js/hologram/core.js's note that only hAlcy's obsidian core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-lyksaum',
    label: 'A.R.X.LYKSAUM',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();
        const deg = Math.PI / 180;

        // Fixed, not theme-tinted: the glyph, rim, core and scan bar all share this one
        // identity colour that never retints, the same convention every other Umbral's
        // hot accent follows -- here it covers the whole "glyph system" rather than a
        // single small part, because that's what reads as this avatar's face.
        const CORE_HOT = 0x00e8ff;

        const DISC_RADIUS = 42;
        const DISC_THICKNESS = 5;
        const GLYPH_Z = DISC_THICKNESS / 2 + 1.2;

        // --- Ambient bloom behind everything. ---
        const ambientGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#eafeff', '#66d9ff'), color: api.palette.hex, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false });
        const ambientGlow = new THREE.Sprite(ambientGlowMat);
        ambientGlow.scale.set(150, 150, 1);
        ambientGlow.position.z = -18;
        group.add(ambientGlow);

        // A slow wobble carries the whole medallion, but never a full turn -- the glyph on
        // its face has to stay legible from the one viewing angle this avatar is meant to
        // be read from.
        const wobbleGroup = new THREE.Group();
        group.add(wobbleGroup);

        // --- The disc body: a squat cylinder rotated to face the camera, plus its own
        // glowing rim. ---
        const discMat = new THREE.MeshBasicMaterial({ color: api.palette.hex3, side: THREE.DoubleSide, transparent: true, opacity: 0.22, depthWrite: false });
        const discGeo = new THREE.CylinderGeometry(DISC_RADIUS, DISC_RADIUS, DISC_THICKNESS, 48);
        discGeo.rotateX(Math.PI / 2);
        const disc = new THREE.Mesh(discGeo, discMat);
        wobbleGroup.add(disc);

        const discWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.6 });
        const discWire = new THREE.LineSegments(new THREE.EdgesGeometry(discGeo), discWireMat);
        wobbleGroup.add(discWire);

        const rimMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false });
        const rim = new THREE.Mesh(new THREE.TorusGeometry(DISC_RADIUS, 1.6, 12, 48), rimMat);
        wobbleGroup.add(rim);

        // A thin inset structural ring, sitting just inside the rim.
        const structRingMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.55 });
        const structRingPts = [];
        for (let i = 0; i <= 64; i++) {
            const a = (i / 64) * Math.PI * 2;
            structRingPts.push(new THREE.Vector3(Math.cos(a) * 34, Math.sin(a) * 34, 0.5));
        }
        const structRing = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(structRingPts), structRingMat);
        wobbleGroup.add(structRing);

        // --- The glyph: a wide collar arc below, two shorter shoulder arcs above, with
        // gaps separating all three -- a broken-ring insignia rather than a full circle. ---
        const glyphMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });

        function addArc(thetaStartDeg, arcDeg) {
            const geo = new THREE.TorusGeometry(21, 2.6, 10, 48, arcDeg * deg);
            const mesh = new THREE.Mesh(geo, glyphMat);
            mesh.rotation.z = thetaStartDeg * deg;
            mesh.position.z = GLYPH_Z;
            wobbleGroup.add(mesh);
            return mesh;
        }
        addArc(200, 140); // bottom collar
        addArc(110, 55);  // top-left shoulder
        addArc(15, 55);   // top-right shoulder

        // --- The core node: a bright white pip ringed by the hot accent, the medallion's
        // one point of focus. ---
        const coreMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9, depthWrite: false });
        const core = new THREE.Mesh(new THREE.SphereGeometry(4, 16, 16), coreMat);
        core.position.z = GLYPH_Z + 0.4;
        wobbleGroup.add(core);

        const coreRingMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreRing = new THREE.Mesh(new THREE.TorusGeometry(7, 0.9, 10, 32), coreRingMat);
        coreRing.position.z = GLYPH_Z + 0.4;
        wobbleGroup.add(coreRing);

        // --- Three micro accent dots at top, left and right (never at the bottom, where
        // the collar arc already reads as the strongest edge). ---
        const dotAngles = [90 * deg, 180 * deg, 0 * deg];
        const microDots = dotAngles.map((a) => {
            const mat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false });
            const dot = new THREE.Mesh(new THREE.SphereGeometry(1.6, 10, 10), mat);
            dot.position.set(Math.cos(a) * 27, Math.sin(a) * 27, GLYPH_Z);
            wobbleGroup.add(dot);
            return dot;
        });

        // A soft horizontal bar that sweeps up and down across the disc's face.
        const scanlineMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: CORE_HOT, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false });
        const scanline = new THREE.Sprite(scanlineMat);
        scanline.scale.set(DISC_RADIUS * 2.1, 7, 1);
        scanline.position.z = GLYPH_Z + 0.8;
        wobbleGroup.add(scanline);

        // --- Two thin rings orbiting the disc in opposite directions, standing in for the
        // reference's counter-rotating interface rings. ---
        const outerRing1Mat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.04), color: api.palette.hex2, transparent: true, opacity: 0.55, depthWrite: false });
        const outerRing1 = new THREE.Sprite(outerRing1Mat);
        outerRing1.scale.set(102, 102, 1);
        group.add(outerRing1);

        const outerRing2Mat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.06), color: api.palette.hex3, transparent: true, opacity: 0.5, depthWrite: false });
        const outerRing2 = new THREE.Sprite(outerRing2Mat);
        outerRing2.scale.set(112, 112, 1);
        group.add(outerRing2);

        return {
            group, wobbleGroup,
            ambientGlow, ambientGlowMat,
            discMat, discWireMat, structRingMat, rimMat, glyphMat,
            core, coreMat, coreRing, coreRingMat,
            microDots,
            scanline, scanlineMat,
            outerRing1, outerRing1Mat, outerRing2, outerRing2Mat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * (Math.PI * 2 / 5)) * 5;
        model.wobbleGroup.rotation.y = Math.sin(ctx.time * 0.35) * 0.07;
        model.wobbleGroup.rotation.x = Math.sin(ctx.time * 0.27 + 1.3) * 0.05;

        // The two orbit rings, turning opposite ways at different speeds.
        model.outerRing1.rotation.z = ctx.time * (Math.PI * 2 / 20);
        model.outerRing2.rotation.z = -ctx.time * (Math.PI * 2 / 15);

        // The scan bar sweeping top to bottom on a 6s loop.
        const scanT = (ctx.time % 6) / 6;
        model.scanline.position.y = -46 + scanT * 92;

        // A brief glitch offset/flicker on click.
        const glitch = ctx.click;
        model.wobbleGroup.position.x = (Math.random() - 0.5) * glitch * 2;
        model.rimMat.opacity = 0.7 + glitch * 0.3;

        // A slow ambient bloom breathing (period ~8s).
        const glowPhase = (Math.sin(ctx.time * (Math.PI * 2 / 8)) + 1) / 2;
        model.ambientGlow.scale.setScalar(150 * (0.95 + 0.15 * glowPhase));

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 3) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.35 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.glyphMat.opacity = 0.7 + pulse * 0.3;
        model.coreRingMat.opacity = 0.65 + pulse * 0.35;
        model.coreMat.opacity = 0.8 + pulse * 0.2;
        const coreScale = 1 + pulse * 0.12 + glitch * 0.2;
        model.core.scale.setScalar(coreScale);
        model.coreRing.scale.setScalar(1 + pulse * 0.08);
        model.scanlineMat.opacity = 0.3 + pulse * 0.15;
        model.ambientGlowMat.opacity = 0.12 + 0.08 * glowPhase + (isSpeaking ? ctx.audio * 0.15 : 0);

        model.microDots.forEach((dot, i) => {
            dot.material.opacity = 0.55 + 0.45 * Math.abs(Math.sin(ctx.time * 1.5 + i * 2.1));
        });
    },

    applyPalette(model, palette) {
        model.discMat.color.setHex(palette.hex3);
        model.discWireMat.color.setHex(palette.hex2);
        model.structRingMat.color.setHex(palette.hex3);
        model.ambientGlowMat.color.setHex(palette.hex);
        model.outerRing1Mat.color.setHex(palette.hex2);
        model.outerRing2Mat.color.setHex(palette.hex3);
        // The glyph, rim, core ring, micro dots and scan bar stay fixed -- this avatar's
        // one hot accent that never retints.
    },
});
