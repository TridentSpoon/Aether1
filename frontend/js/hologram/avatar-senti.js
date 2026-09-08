/* Avatar: Senti -- a small mechanical-organic probe caught mid-scan, hovering on a
 * column of light above a brass emitter ring. Inspired by the classic "hologram in a
 * porthole" beat -- a creature made of projected light rising out of a physical
 * housing -- reimagined from scratch for this engine's own point-cloud/wireframe
 * language rather than any one design being copied.
 *
 * The ring is real hardware (fixed brass, Phong-lit like every other avatar's obsidian
 * core) and stays put; everything above it -- the beam, the probe's curling legs, its
 * core -- is drawn light and follows the color theme. The core's own "sensor" glow and
 * the legs' clawed tips stay a fixed icy white-blue regardless of theme, the same
 * "hot accents don't retint" convention R.E.D. 9000's lens and the Nexus's eyes follow.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'senti',
    label: 'Senti',

    build(api) {
        const group = new THREE.Group();
        const HOT = 0xbfe8ff; // the probe's own sensor light -- fixed, not theme-tinted

        // --- Emitter ring: the physical housing the hologram projects from. Fixed brass,
        // lit (MeshPhongMaterial) rather than additive-glow, since it's meant to read as
        // solid hardware the creature above is projected out of, not drawn light itself. ---
        const ringMat = new THREE.MeshPhongMaterial({ color: 0x8a6a35, specular: 0xffdca0, shininess: 70 });
        const ring = new THREE.Mesh(new THREE.TorusGeometry(40, 5, 14, 56), ringMat);
        ring.rotation.x = Math.PI / 2;
        ring.position.y = -50;
        group.add(ring);

        const lipMat = new THREE.MeshPhongMaterial({ color: 0x2e2013, specular: 0xd8a45c, shininess: 40 });
        const lip = new THREE.Mesh(new THREE.TorusGeometry(29, 2.4, 10, 48), lipMat);
        lip.rotation.x = Math.PI / 2;
        lip.position.y = -50;
        group.add(lip);

        // --- Projection beam: a tapering column of points rising from the ring -- denser
        // and wider near the housing, thinning toward the probe above, like light
        // gathering into a shape rather than a flat cylinder of haze. ---
        const beamCount = 900;
        const beamBottomY = -46;
        const beamTopY = -2;
        const beamPositions = new Float32Array(beamCount * 3);
        for (let i = 0; i < beamCount; i++) {
            const t = Math.random();
            const y = THREE.MathUtils.lerp(beamBottomY, beamTopY, t);
            const r = THREE.MathUtils.lerp(26, 3, t) * (0.35 + Math.random() * 0.65);
            const a = Math.random() * Math.PI * 2;
            beamPositions[i * 3] = Math.cos(a) * r;
            beamPositions[i * 3 + 1] = y;
            beamPositions[i * 3 + 2] = Math.sin(a) * r;
        }
        const beamGeo = new THREE.BufferGeometry();
        beamGeo.setAttribute('position', new THREE.BufferAttribute(beamPositions, 3));
        const beamMaterial = new THREE.PointsMaterial({
            color: api.palette.hex,
            map: api.helpers.glowTexture(24),
            size: 1.8,
            transparent: true,
            opacity: 0.5,
            blending: THREE.AdditiveBlending,
            depthWrite: false
        });
        const beamPoints = new THREE.Points(beamGeo, beamMaterial);
        group.add(beamPoints);

        // --- The probe: a small obsidian core with six curling, claw-tipped legs,
        // hovering at the top of the beam. The core stays a fixed dark material like
        // every other avatar's core; the legs are drawn light and follow the theme. ---
        const coreBaseY = 18;
        const coreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0a0f, specular: HOT, shininess: 90, transparent: true, opacity: 0.97
        });
        const core = new THREE.Mesh(new THREE.IcosahedronGeometry(9, 1), coreMat);
        core.position.y = coreBaseY;
        group.add(core);

        const eyeGlowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(32), color: HOT, transparent: true,
            opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const eyeGlow = new THREE.Sprite(eyeGlowMat);
        eyeGlow.scale.set(16, 16, 1);
        eyeGlow.position.z = 8;
        core.add(eyeGlow);

        const legMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
        });
        const tipMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(24), color: HOT, transparent: true,
            opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false
        });

        // Each leg is a curved tube (built once, static) sitting inside its own pivot
        // group so animate() can twitch it by rotating the pivot rather than rebuilding
        // geometry every frame. Legs are children of the core, so they hover and bob
        // along with it for free.
        const legCount = 6;
        const legPivots = [];
        for (let i = 0; i < legCount; i++) {
            const angle = (i / legCount) * Math.PI * 2;
            const pivot = new THREE.Group();
            pivot.rotation.y = angle;
            pivot.userData = { phase: Math.random() * Math.PI * 2 };
            core.add(pivot);

            // Runs outward from the core's surface, dips down, and hooks back up at the
            // tip -- a claw-like silhouette rather than a straight spike.
            const curve = new THREE.CatmullRomCurve3([
                new THREE.Vector3(7, 1, 0),
                new THREE.Vector3(16, -4, 0),
                new THREE.Vector3(22, -13, 0),
                new THREE.Vector3(21, -20, 0),
                new THREE.Vector3(16, -23, 0)
            ]);
            const leg = new THREE.Mesh(new THREE.TubeGeometry(curve, 12, 1.1, 6, false), legMat);
            pivot.add(leg);

            const tip = new THREE.Sprite(tipMat);
            tip.scale.set(4.5, 4.5, 1);
            tip.position.set(16, -23, 0);
            pivot.add(tip);

            legPivots.push(pivot);
        }

        // Soft ambient glow behind everything, theme-tinted, so the hologram reads as
        // light filling the space rather than sitting flat against the backdrop.
        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true,
            opacity: 0.2, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(170, 170, 1);
        glow.position.set(0, -20, -30);
        group.add(glow);

        return {
            group, core, coreBaseY, beamPoints, beamMaterial, legMat, legPivots,
            eyeGlowMat, glow, glowMaterial
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // The whole probe drifts in a slow turn at rest, sharpening into a faster spin
        // while thinking, as if scanning for wherever it's "looking".
        model.group.rotation.y = ctx.time * (isThinking ? 1.1 : 0.25);

        // The core hovers and bobs on top of the beam -- a gentle idle drift, a quicker
        // jitter while thinking, and a lift with the voice while speaking. A click gives
        // it a brief startled hop.
        const bob = Math.sin(ctx.time * (isThinking ? 3.2 : 1.1)) * (isThinking ? 3 : 5);
        const audioBob = isSpeaking ? ctx.audio * 8 : 0;
        model.core.position.y = model.coreBaseY + bob + audioBob + ctx.click * 10;

        // Legs twitch independently -- faint at rest, sharp and fast while thinking, with
        // an added kick from the voice while speaking.
        model.legPivots.forEach((pivot) => {
            const twitchSpeed = isThinking ? 5 : 1.4;
            const twitchAmp = isThinking ? 0.35 : 0.12;
            const audioKick = isSpeaking ? ctx.audio * 0.4 : 0;
            pivot.rotation.z = Math.sin(ctx.time * twitchSpeed + pivot.userData.phase) * (twitchAmp + audioKick);
        });

        // The beam brightens and thickens with speech, and simmers faintly while thinking.
        let beamOpacity = 0.5;
        if (isSpeaking) beamOpacity = 0.5 + ctx.audio * 0.4;
        else if (isThinking) beamOpacity = 0.5 + Math.abs(Math.sin(ctx.time * 6)) * 0.25;
        model.beamMaterial.opacity = beamOpacity;
        model.beamMaterial.size = 1.8 + (isSpeaking ? ctx.audio * 1.2 : 0);

        // The core's sensor glow pulses like a slow heartbeat at rest, and flares with speech.
        model.eyeGlowMat.opacity = isSpeaking
            ? 0.8 + ctx.audio * 0.5
            : 0.6 + Math.abs(Math.sin(ctx.time * 1.6)) * 0.25;

        let glowIntensity;
        if (isSpeaking) glowIntensity = 0.2 + ctx.audio * 0.3;
        else if (isThinking) glowIntensity = 0.2 + Math.sin(ctx.time * 8) * 0.1;
        else glowIntensity = 0.18 + ctx.click * 0.2;
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.beamMaterial.color.setHex(palette.hex);
        model.legMat.color.setHex(palette.hex2);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
