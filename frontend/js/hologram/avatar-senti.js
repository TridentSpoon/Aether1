/* Avatar: Senti -- a small mechanical-organic probe caught mid-scan, hovering on a
 * column of light above a brass emitter ring, with a burst of long tentacles radiating
 * from its core that ripple in a genuine traveling wave -- root anchored, amplitude
 * growing toward the tip, the way a real whip or a jellyfish's trailing arms move.
 * Inspired by the general idea of "a body with many limbs radiating outward, moving
 * fluidly" and by the classic "hologram in a porthole" beat -- a creature made of
 * projected light rising out of a physical housing -- rather than any one specific
 * design, and built in this engine's own point-cloud/wireframe language throughout.
 *
 * The ring is real hardware (fixed brass, Phong-lit like every other avatar's obsidian
 * core) and stays put; everything above it -- the beam, the tentacles, the core -- is
 * drawn light and follows the color theme. The core's own "sensor" glow and the
 * tentacles' joints/tips stay a fixed icy white-blue regardless of theme, the same
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
        // solid hardware the creature above is projected out of, not drawn light itself.
        // Still a ring (a circular loop, same as a torus) but with a square cross-section
        // instead of a round one -- a square profile swept around a circle, like a donut
        // milled from square-stock tubing instead of a smooth round one. ---
        class CircularPath extends THREE.Curve {
            constructor(radius) { super(); this.radius = radius; }
            getPoint(t, target = new THREE.Vector3()) {
                const angle = t * Math.PI * 2;
                return target.set(Math.cos(angle) * this.radius, 0, Math.sin(angle) * this.radius);
            }
        }

        function squareTubeRingGeometry(pathRadius, half, segments = 64) {
            // The cross-section: a small square, swept so its local X follows the ring's
            // radial direction and local Y follows straight up -- the same "square profile"
            // a torus would have if its round tube were milled flat on all four sides.
            const crossSection = new THREE.Shape();
            crossSection.moveTo(-half, -half);
            crossSection.lineTo(half, -half);
            crossSection.lineTo(half, half);
            crossSection.lineTo(-half, half);
            crossSection.closePath();

            return new THREE.ExtrudeGeometry(crossSection, {
                steps: segments,
                bevelEnabled: false,
                extrudePath: new CircularPath(pathRadius)
            });
        }

        const ringMat = new THREE.MeshPhongMaterial({ color: 0x8a6a35, specular: 0xffdca0, shininess: 70 });
        const ring = new THREE.Mesh(squareTubeRingGeometry(40, 5), ringMat);
        ring.position.y = -50;
        group.add(ring);

        const lipMat = new THREE.MeshPhongMaterial({ color: 0x2e2013, specular: 0xd8a45c, shininess: 40 });
        const lip = new THREE.Mesh(squareTubeRingGeometry(29, 2.4), lipMat);
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

        // --- The probe: a small obsidian core with a burst of long, fluid tentacles,
        // hovering at the top of the beam. The core stays a fixed dark material like
        // every other avatar's core; the tentacles are drawn light and follow the theme. ---
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

        // Each tentacle is a chain of joints sharing one BufferGeometry -- a thin Line for
        // the strand itself (theme-tinted, reads as the tentacle's own drawn-light body)
        // and a Points overlay at the same joints for brighter node markers (fixed icy
        // white-blue, the same "nodes brighter than the line between them" depth cue the
        // Nexus's hull uses). animate() rewrites the position attribute every frame -- a
        // cheap kinematic update, not a rebuild -- so the whole thing can ripple with a
        // real traveling wave instead of the geometry ever being reconstructed.
        const tentacleLineMat = new THREE.LineBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.6
        });
        const tentacleNodeMat = new THREE.PointsMaterial({
            color: HOT, map: api.helpers.glowTexture(20), size: 2.6,
            transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const tentacleTipMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(24), color: HOT, transparent: true,
            opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false
        });

        const tentacleCount = 12;
        const jointCount = 9; // root (doesn't move relative to the core) plus 8
        const tentacles = [];
        for (let i = 0; i < tentacleCount; i++) {
            // Each tentacle's rest direction radiates outward from the core in a burst,
            // spread evenly in azimuth with a little jitter so they don't read as a
            // perfectly mechanical fan.
            const azimuth = (i / tentacleCount) * Math.PI * 2 + (Math.random() - 0.5) * 0.3;
            const dir = new THREE.Vector3(Math.cos(azimuth), 0, Math.sin(azimuth)).normalize();
            const perpA = new THREE.Vector3().crossVectors(dir, new THREE.Vector3(0, 1, 0));
            if (perpA.lengthSq() < 1e-6) perpA.set(1, 0, 0);
            perpA.normalize();
            const perpB = new THREE.Vector3().crossVectors(dir, perpA).normalize();

            const positions = new Float32Array(jointCount * 3);
            const geom = new THREE.BufferGeometry();
            geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));

            const line = new THREE.Line(geom, tentacleLineMat);
            const nodes = new THREE.Points(geom, tentacleNodeMat);
            core.add(line, nodes);

            const tip = new THREE.Sprite(tentacleTipMat);
            tip.scale.set(4, 4, 1);
            core.add(tip);

            tentacles.push({
                positions, geom, tip, dir, perpA, perpB,
                length: 22 + Math.random() * 10,
                phase: Math.random() * Math.PI * 2,
                speedMult: 0.85 + Math.random() * 0.3
            });
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
            group, core, coreBaseY, beamPoints, beamMaterial, tentacleLineMat, tentacles,
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

        // Tentacles ripple with a traveling wave whose amplitude grows toward the tip --
        // drooping and calm at rest, whipping wider and faster while thinking, with an
        // added kick from the voice while speaking. The root (joint 0) barely moves in
        // any state, the same way a real whip's motion is all in its far end.
        const droop = (isThinking || isSpeaking) ? -0.12 : -0.45;
        const waveAmp = isThinking ? 9 : isSpeaking ? 7 + ctx.audio * 6 : 4;
        const waveFreq = isThinking ? 2.6 : 1.8;
        const waveSpeed = isThinking ? 5 : isSpeaking ? 3.5 : 1.6;

        model.tentacles.forEach((t) => {
            const segments = t.positions.length / 3 - 1;
            let tipX = 0, tipY = 0, tipZ = 0;
            for (let j = 0; j <= segments; j++) {
                const jt = j / segments;
                const reach = jt * t.length;
                const wavePhase = jt * waveFreq * Math.PI * 2 - ctx.time * waveSpeed * t.speedMult + t.phase;
                const spread = Math.pow(jt, 1.4); // keeps the root anchored, the tip doing the swinging
                const swingA = Math.sin(wavePhase) * waveAmp * spread;
                const swingB = Math.cos(wavePhase * 0.7) * waveAmp * 0.6 * spread;

                const px = t.dir.x * reach + t.perpA.x * swingA + t.perpB.x * swingB;
                const py = t.dir.y * reach + t.perpA.y * swingA + t.perpB.y * swingB + droop * reach * jt;
                const pz = t.dir.z * reach + t.perpA.z * swingA + t.perpB.z * swingB;

                t.positions[j * 3] = px;
                t.positions[j * 3 + 1] = py;
                t.positions[j * 3 + 2] = pz;
                if (j === segments) { tipX = px; tipY = py; tipZ = pz; }
            }
            t.geom.attributes.position.needsUpdate = true;
            t.tip.position.set(tipX, tipY, tipZ);
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
        model.tentacleLineMat.color.setHex(palette.hex2);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
