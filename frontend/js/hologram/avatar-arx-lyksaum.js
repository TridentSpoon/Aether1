/* Avatar: A.R.X.LYKSAUM -- a HUD medallion built as an actual 3D disc: a tilted, slowly
 * spinning cylindrical housing (thick enough, and turned far enough off-axis, that its rim
 * and edge read as real depth rather than a flat sprite) carrying a glowing border, an
 * inset structural ring, a sweeping scan bar and three accent dots on its surface. Fixed in
 * front of that turning housing -- never tilting or spinning with it -- sits the avatar's
 * one constant: a small bright core node ringed by a broken 3-piece circle (a wide collar
 * arc below, two shorter shoulder arcs above, with gaps between all three). That core
 * cluster is this avatar's face in every sense the engine cares about, so it stays fixed
 * the same way A.R.X.LOREGENDA's face does -- an early build of that avatar hid its face by
 * rotating it along with its shell, and this file avoids repeating that mistake by never
 * spinning the one thing that has to stay legible.
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
        const DISC_THICKNESS = 10;
        const BASE_TILT = -0.5; // ~29 degrees off-axis, enough to show the rim as an ellipse
        const CORE_Z = DISC_THICKNESS / 2 + 3; // fixed, in front of the disc regardless of its tilt/spin

        // --- Ambient bloom behind everything. ---
        const ambientGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#eafeff', '#66d9ff'), color: api.palette.hex, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false });
        const ambientGlow = new THREE.Sprite(ambientGlowMat);
        ambientGlow.scale.set(150, 150, 1);
        ambientGlow.position.z = -18;
        group.add(ambientGlow);

        // The disc housing: tilted off-axis and slowly spun in animate() so its thickness
        // and rim actually read as a 3D object instead of a flat circle facing the camera.
        const bodyGroup = new THREE.Group();
        bodyGroup.rotation.x = BASE_TILT;
        group.add(bodyGroup);

        // --- The disc body: a real cylinder, thick enough to show an edge once tilted,
        // plus its own glowing rim. ---
        const discMat = new THREE.MeshBasicMaterial({ color: api.palette.hex3, side: THREE.DoubleSide, transparent: true, opacity: 0.22, depthWrite: false });
        const discGeo = new THREE.CylinderGeometry(DISC_RADIUS, DISC_RADIUS, DISC_THICKNESS, 48);
        discGeo.rotateX(Math.PI / 2);
        const disc = new THREE.Mesh(discGeo, discMat);
        bodyGroup.add(disc);

        const discWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.6 });
        const discWire = new THREE.LineSegments(new THREE.EdgesGeometry(discGeo), discWireMat);
        bodyGroup.add(discWire);

        const rimMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false });
        const rim = new THREE.Mesh(new THREE.TorusGeometry(DISC_RADIUS, 1.6, 12, 48), rimMat);
        bodyGroup.add(rim);

        // A thin inset structural ring, sitting just inside the rim.
        const structRingMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.55 });
        const structRingPts = [];
        for (let i = 0; i <= 64; i++) {
            const a = (i / 64) * Math.PI * 2;
            structRingPts.push(new THREE.Vector3(Math.cos(a) * 34, Math.sin(a) * 34, 0.5));
        }
        const structRing = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(structRingPts), structRingMat);
        bodyGroup.add(structRing);

        // --- Three micro accent dots at top, left and right of the disc face (never at the
        // bottom, where the core cluster's own collar arc already reads as the strongest
        // edge). Mounted on the housing, so they turn with it. ---
        const dotAngles = [90 * deg, 180 * deg, 0 * deg];
        const microDots = dotAngles.map((a) => {
            const mat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false });
            const dot = new THREE.Mesh(new THREE.SphereGeometry(1.6, 10, 10), mat);
            dot.position.set(Math.cos(a) * 27, Math.sin(a) * 27, DISC_THICKNESS / 2 + 1.2);
            bodyGroup.add(dot);
            return dot;
        });

        // A soft horizontal bar that sweeps up and down across the disc's face.
        const scanlineMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: CORE_HOT, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false });
        const scanline = new THREE.Sprite(scanlineMat);
        scanline.scale.set(DISC_RADIUS * 2.1, 7, 1);
        scanline.position.z = DISC_THICKNESS / 2 + 1.4;
        bodyGroup.add(scanline);

        // --- The core cluster: this avatar's one fixed point of focus, a bright white pip
        // ringed by a broken 3-piece circle -- a wide collar arc below, two shorter
        // shoulder arcs above, with gaps separating all three. Never parented under
        // bodyGroup, so it always faces the camera no matter how the housing behind it
        // tilts or spins. ---
        const coreGroup = new THREE.Group();
        coreGroup.position.z = CORE_Z;
        group.add(coreGroup);

        const glyphMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });

        function addArc(thetaStartDeg, arcDeg) {
            const geo = new THREE.TorusGeometry(23, 3, 10, 48, arcDeg * deg);
            const mesh = new THREE.Mesh(geo, glyphMat);
            mesh.rotation.z = thetaStartDeg * deg;
            coreGroup.add(mesh);
            return mesh;
        }
        addArc(200, 140); // bottom collar
        addArc(110, 55);  // top-left shoulder
        addArc(15, 55);   // top-right shoulder

        const coreMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9, depthWrite: false });
        const core = new THREE.Mesh(new THREE.SphereGeometry(5, 16, 16), coreMat);
        coreGroup.add(core);

        const coreRingMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreRing = new THREE.Mesh(new THREE.TorusGeometry(9, 1.1, 10, 32), coreRingMat);
        coreGroup.add(coreRing);

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
            group, bodyGroup, coreGroup, BASE_TILT,
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

        // The disc housing spins continuously around Y (revealing its rim/thickness as it
        // turns) while its base tilt breathes slightly on top -- this, not the old idle-only
        // wobble, is what sells the housing as a real 3D object. The core cluster is never
        // touched here: it lives in its own non-rotating group, always facing the camera.
        model.bodyGroup.rotation.y = ctx.time * (Math.PI * 2 / 14);
        model.bodyGroup.rotation.x = model.BASE_TILT + Math.sin(ctx.time * 0.3) * 0.06;

        // The two orbit rings, turning opposite ways at different speeds.
        model.outerRing1.rotation.z = ctx.time * (Math.PI * 2 / 20);
        model.outerRing2.rotation.z = -ctx.time * (Math.PI * 2 / 15);

        // The scan bar sweeping top to bottom on a 6s loop.
        const scanT = (ctx.time % 6) / 6;
        model.scanline.position.y = -46 + scanT * 92;

        // A brief glitch offset/flicker on click.
        const glitch = ctx.click;
        model.bodyGroup.position.x = (Math.random() - 0.5) * glitch * 2;
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
