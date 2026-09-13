/* Avatar: A.R.X.LOREGENDA -- an elongated faceted head, one squashed icosahedron shell
 * wrapped around a smaller, dimmer twin sitting just inside it, so the head reads as
 * one recessed-panel construction rather than a solid lump. At its centre, instead of
 * a plain gem, a small stylised face -- two flattened lens "eyes" with a bright pupil
 * each, and a thin glowing bar for a mouth -- sits behind the recessed inner shell like
 * a mask glowing through it, and blinks now and then to read as alive rather than just
 * decorative. A slow drifting dust of small points and a tilted halo ring stand in for
 * the reference's ambient particle field and UI rings.
 *
 * Loosely inspired by faceted low-poly sci-fi companion-drone motifs in general -- an
 * elongated faceted head shell around a small glowing face -- not a copy of any one
 * specific design, and redrawn from scratch in this engine's own flat-shape/unlit-
 * material language (MeshBasicMaterial, no scene lighting) the same way every other
 * avatar here is hand built; see js/hologram/core.js's note that only hAlcy's obsidian
 * core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-loregenda',
    label: 'A.R.X.LOREGENDA',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the face's glow, this avatar's one identity colour
        // that never retints, the same convention every other Umbral's hot accent follows.
        const CORE_HOT = 0x00e1ff;

        const HEAD_RADIUS = 34;
        const HEAD_SCALE = { x: 1.0, y: 1.25, z: 0.85 }; // squash/stretch into an egg-shaped head
        const INNER_RADIUS = 24;
        const EYE_OFFSET_X = 12;
        const EYE_Y = 4;
        // Just outside the inner shell's own front face (INNER_RADIUS * HEAD_SCALE.z) and
        // well short of the outer shell's, so only one translucent layer sits in front of
        // it -- nested behind both, the small face washed out into an unreadable glow.
        const FACE_Z = INNER_RADIUS * HEAD_SCALE.z * 1.1;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.15, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(140, 140, 1);
        glow.position.z = -10;
        group.add(glow);

        const spinGroup = new THREE.Group();
        group.add(spinGroup);

        // --- Outer and inner facet shells: two squashed icosahedra, the inner one a
        // recessed twin of the outer, both translucent so the face glows through them. ---
        // depthWrite: false -- a translucent shell that still wrote depth would punch
        // solid-looking holes in its own z-buffer, hiding the face behind whichever
        // facet happened to be nearest the camera at that instant instead of letting
        // it show through consistently.
        const shellFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.2, depthWrite: false });
        const outerWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });
        const innerWireMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.5 });

        const outerGeo = new THREE.IcosahedronGeometry(HEAD_RADIUS, 1);
        outerGeo.scale(HEAD_SCALE.x, HEAD_SCALE.y, HEAD_SCALE.z);
        const outerShell = new THREE.Mesh(outerGeo, shellFillMat);
        spinGroup.add(outerShell);
        const outerWire = new THREE.LineSegments(new THREE.EdgesGeometry(outerGeo), outerWireMat);
        spinGroup.add(outerWire);

        const innerGeo = new THREE.IcosahedronGeometry(INNER_RADIUS, 1);
        innerGeo.scale(HEAD_SCALE.x, HEAD_SCALE.y, HEAD_SCALE.z);
        const innerShell = new THREE.Mesh(innerGeo, shellFillMat.clone());
        innerShell.material.opacity = 0.14;
        spinGroup.add(innerShell);
        const innerWire = new THREE.LineSegments(new THREE.EdgesGeometry(innerGeo), innerWireMat);
        spinGroup.add(innerWire);

        // --- The face: two lens-shaped eyes with a bright pupil each, and a thin
        // glowing mouth bar, all fixed-colour. A sibling of spinGroup rather than a
        // child of it, so the shell can slowly turn around the face without ever
        // carrying it out of view -- the face itself always looks straight at the
        // viewer, the way a fixed lens would even inside a rotating housing.
        const faceGroup = new THREE.Group();
        faceGroup.position.z = FACE_Z;
        group.add(faceGroup);

        const eyeMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending });
        const pupilMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95 });
        const mouthMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.75, blending: THREE.AdditiveBlending });

        const eyeGeo = new THREE.OctahedronGeometry(8, 0);
        eyeGeo.scale(0.75, 1.5, 0.45);
        const pupilGeo = new THREE.SphereGeometry(2.2, 12, 12);

        const leftEye = new THREE.Mesh(eyeGeo, eyeMat);
        leftEye.position.set(-EYE_OFFSET_X, EYE_Y, 0);
        faceGroup.add(leftEye);
        const leftPupil = new THREE.Mesh(pupilGeo, pupilMat);
        leftPupil.position.set(-EYE_OFFSET_X, EYE_Y, 3);
        faceGroup.add(leftPupil);

        const rightEye = new THREE.Mesh(eyeGeo, eyeMat);
        rightEye.position.set(EYE_OFFSET_X, EYE_Y, 0);
        faceGroup.add(rightEye);
        const rightPupil = new THREE.Mesh(pupilGeo, pupilMat);
        rightPupil.position.set(EYE_OFFSET_X, EYE_Y, 3);
        faceGroup.add(rightPupil);

        const mouth = new THREE.Mesh(new THREE.BoxGeometry(20, 2, 1.8), mouthMat);
        mouth.position.set(0, -11, 0);
        faceGroup.add(mouth);

        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#bff6ff', '#00e1ff'), transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = 52;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        coreGlow.position.set(0, EYE_Y - 3, -4);
        faceGroup.add(coreGlow);

        // --- A tilted halo ring and a slow drifting dust of points, standing in for
        // the reference's UI rings and ambient particle cloud. ---
        const ringMat = new THREE.SpriteMaterial({ map: api.helpers.ringTexture(64, 0.05), color: api.palette.hex3, transparent: true, opacity: 0.5, depthWrite: false });
        const ring = new THREE.Sprite(ringMat);
        ring.scale.set(96, 96, 1);
        ring.rotation.z = Math.PI / 5;
        spinGroup.add(ring);

        const dustCount = 70;
        const dustPositions = new Float32Array(dustCount * 3);
        for (let i = 0; i < dustCount * 3; i += 3) {
            const r = 55 + Math.random() * 30;
            const theta = Math.random() * Math.PI * 2;
            const phi = Math.acos(Math.random() * 2 - 1);
            dustPositions[i] = r * Math.sin(phi) * Math.cos(theta);
            dustPositions[i + 1] = r * Math.cos(phi) * 0.6;
            dustPositions[i + 2] = r * Math.sin(phi) * Math.sin(theta);
        }
        const dustGeo = new THREE.BufferGeometry();
        dustGeo.setAttribute('position', new THREE.BufferAttribute(dustPositions, 3));
        const dustMat = new THREE.PointsMaterial({ color: api.palette.hex, size: 1.4, transparent: true, opacity: 0.55 });
        const dust = new THREE.Points(dustGeo, dustMat);
        group.add(dust);

        return {
            group, spinGroup, faceGroup, dust,
            glowMat, shellFillMat, innerShellMat: innerShell.material, outerWireMat, innerWireMat,
            eyeMat, mouthMat, coreGlow, coreGlowMat, coreGlowScale, ringMat, dustMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 1.5) * 3;
        model.spinGroup.rotation.y = ctx.time * 0.15;
        model.spinGroup.rotation.x = Math.sin(ctx.time * 0.5) * 0.08;
        model.dust.rotation.y = ctx.time * 0.05;

        // A brief glitch offset/flicker on click, plus a wireframe flash.
        const glitch = ctx.click;
        model.faceGroup.position.x = (Math.random() - 0.5) * glitch * 3;
        model.outerWireMat.opacity = 0.7 + glitch * 0.3;

        // A slow, mostly-open blink: a short dip to fully shut every few seconds.
        const blinkCycle = 4.2;
        const blinkWindow = 0.18;
        const t = ctx.time % blinkCycle;
        const blink = t < blinkWindow ? 1 - Math.pow(Math.sin((t / blinkWindow) * Math.PI), 2) : 1;
        model.faceGroup.scale.y = blink;

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 3) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.35 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.eyeMat.opacity = 0.7 + pulse * 0.3;
        model.mouthMat.opacity = 0.55 + pulse * 0.35 + (isSpeaking ? ctx.audio * 0.2 : 0);
        model.coreGlowMat.opacity = Math.min(1, 0.3 + pulse * 0.5);
        const scale = 1 + pulse * 0.15 + ctx.click * 0.2;
        model.coreGlow.scale.set(model.coreGlowScale * scale, model.coreGlowScale * scale, 1);

        model.glowMat.opacity = 0.13 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.2) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.shellFillMat.color.setHex(palette.hex);
        model.innerShellMat.color.setHex(palette.hex);
        model.outerWireMat.color.setHex(palette.hex2);
        model.innerWireMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        model.ringMat.color.setHex(palette.hex3);
        model.dustMat.color.setHex(palette.hex);
        // The face -- eyes, pupils and mouth -- stays fixed, this avatar's one hot
        // accent that never retints.
    },
});
