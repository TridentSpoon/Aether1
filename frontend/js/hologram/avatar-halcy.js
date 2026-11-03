/* Avatar: hAlcy -- a harmonic particle lattice around an obsidian core, inside two
 * orbital rings.
 *
 * The lattice is a sphere of 2800 points that breathes with the voice; the inner ring is
 * segmented so its circumference can thicken bar by bar like an equalizer, and the outer
 * ring carries a single broad swell baked into its band that rides around the
 * circumference as it spins. Both rings dodge outward in step with the lattice's bulge, so
 * neither is ever swallowed by it mid-click or mid-sentence.
 *
 * This is also the engine's fallback shape: a name the engine does not recognise is shown
 * as hAlcy rather than as nothing (see resolveAvatarId in core.js).
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'halcy',
    label: 'hAlcy',
    group: 'Singular Ascended Class',

    build(api) {
        const THREE = api.THREE;
        const group = new THREE.Group();

        // --- The lattice: points spread evenly over a sphere by the golden-angle spiral,
        // which gives an even covering no latitude/longitude grid does.
        const particleCount = 2800;
        const latticeRadius = 34;
        const geometry = new THREE.BufferGeometry();
        const positions = new Float32Array(particleCount * 3);
        const colors = new Float32Array(particleCount * 3);
        const basePositions = [];

        for (let i = 0; i < particleCount; i++) {
            const phi = Math.acos(-1 + (2 * i) / particleCount);
            const theta = Math.sqrt(particleCount * Math.PI) * phi;

            const x = latticeRadius * Math.cos(theta) * Math.sin(phi);
            const y = latticeRadius * Math.sin(theta) * Math.sin(phi);
            const z = latticeRadius * Math.cos(phi);

            positions[i * 3] = x;
            positions[i * 3 + 1] = y;
            positions[i * 3 + 2] = z;
            basePositions.push({ x, y, z });

            colors[i * 3] = api.palette.r;
            colors[i * 3 + 1] = Math.min(1, api.palette.g + Math.random() * 0.15);
            colors[i * 3 + 2] = api.palette.b;
        }

        geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geometry.setAttribute('color', new THREE.BufferAttribute(colors, 3));

        const particleSystem = new THREE.Points(geometry, new THREE.PointsMaterial({
            size: 3.2,
            vertexColors: true,
            map: api.helpers.glowTexture(32),
            transparent: true,
            opacity: 0.9,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
        }));
        group.add(particleSystem);

        // Obsidian inner sphere -- a solid glossy dark core, fixed colour whatever the
        // theme, the same convention every other avatar's one constant colour follows.
        const coreOrb = new THREE.Mesh(
            new THREE.SphereGeometry(18, 32, 32),
            new THREE.MeshPhongMaterial({
                color: 0x0a0a0f,
                specular: 0x8fa8ff,
                shininess: 90,
                transparent: true,
                opacity: 0.97,
            })
        );
        group.add(coreOrb);

        /* The inner ring's resting radius (a few units clear of the lattice) and its
           baseline outward reach once its equalizer segments are included. Both are
           re-derived every frame in animate so the rings dodge outward in step with the
           lattice bulging during clicks, thought and speech, instead of a fixed gap that
           would get swallowed. */
        const innerRingGap = 5;
        const restingSegReach = 6; // segment length before any equalizer lenScale
        const outerRingGap = 4;
        const outerRingBaseRadius = latticeRadius + innerRingGap + restingSegReach + outerRingGap;

        /* Static outer ring -- fixed cyan, unaffected by the theme. Authored at its resting
           radius, then scaled at runtime to track its current one. Kept perfectly flat (no
           x or y tilt) so it reads as a true circle around the inner ring rather than an
           ellipse. */
        const outerRingThickness = 2.5;
        const outerRingGeom = new THREE.RingGeometry(
            outerRingBaseRadius - outerRingThickness / 2,
            outerRingBaseRadius + outerRingThickness / 2,
            128
        );

        /* Bake a single broad, gradual swell into the band at local angle 0 -- since the
           mesh spins on its Z axis every frame, this rides around the circumference as a
           travelling wave that reads as motion, instead of the ring looking like a static
           disc. Only the outer edge moves for it: the inner edge, facing the inner ring,
           stays put so its distance from that ring never varies and the band's thickness
           swells instead. */
        const bulgeHalfAngle = Math.PI / 2.6;
        const bulgeMaxAmount = 3.5;
        const outerPos = outerRingGeom.attributes.position;
        for (let vi = 0; vi < outerPos.count; vi++) {
            const vx = outerPos.getX(vi);
            const vy = outerPos.getY(vi);
            const vAngle = Math.atan2(vy, vx);
            const baseR = Math.sqrt(vx * vx + vy * vy);
            const isOuterEdge = baseR > outerRingBaseRadius;
            let bulge = 0;
            if (isOuterEdge) {
                const angleDiff = Math.atan2(Math.sin(vAngle), Math.cos(vAngle));
                if (Math.abs(angleDiff) < bulgeHalfAngle) {
                    bulge = bulgeMaxAmount * 0.5 * (1 + Math.cos((angleDiff / bulgeHalfAngle) * Math.PI));
                }
            }
            const r = baseR + bulge;
            outerPos.setXY(vi, Math.cos(vAngle) * r, Math.sin(vAngle) * r);
        }
        outerPos.needsUpdate = true;

        const outerRing = new THREE.Mesh(outerRingGeom, new THREE.MeshBasicMaterial({
            color: 0x00f0ff,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.5,
            blending: THREE.AdditiveBlending,
        }));
        group.add(outerRing);

        /* Inner ultramarine equalizer ring -- segmented so its circumference can thicken
           bar by bar like an audio equalizer while speaking, and so the whole ring can sway
           on its Z axis. It dodges a few units clear of the lattice surface and spins
           slowly even at rest. */
        const innerRingGroup = new THREE.Group();
        const innerSegments = [];
        const innerRingRadius = latticeRadius + innerRingGap;
        const segmentCount = 40;
        const segMat = new THREE.MeshBasicMaterial({
            color: 0x2b3eff,
            vertexColors: true,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending,
        });
        for (let i = 0; i < segmentCount; i++) {
            const angle = (i / segmentCount) * Math.PI * 2;
            const segGeom = new THREE.BoxGeometry(3.6, 6, 2.6);
            segGeom.translate(0, 3, 0); // pivot at the inner edge, so it only extends outward

            // A radial gradient -- brightest where the segment meets the lattice (local
            // y = 0), fading darker toward its outer tip (local y = 6).
            const segPos = segGeom.attributes.position;
            const segColors = new Float32Array(segPos.count * 3);
            for (let vi = 0; vi < segPos.count; vi++) {
                const t = THREE.MathUtils.clamp(segPos.getY(vi) / 6, 0, 1);
                const brightness = 1 - t * 0.7;
                segColors[vi * 3] = brightness;
                segColors[vi * 3 + 1] = brightness;
                segColors[vi * 3 + 2] = brightness;
            }
            segGeom.setAttribute('color', new THREE.BufferAttribute(segColors, 3));

            const seg = new THREE.Mesh(segGeom, segMat);
            seg.position.set(Math.cos(angle) * innerRingRadius, Math.sin(angle) * innerRingRadius, 0);
            seg.rotation.z = angle - Math.PI / 2;
            seg.userData = { angle };
            innerSegments.push(seg);
            innerRingGroup.add(seg);
        }
        group.add(innerRingGroup);

        return {
            group,
            particleSystem, particleCount, basePositions, latticeRadius,
            coreOrb,
            outerRing, outerRingBaseRadius, outerRingGap, outerRingRadius: outerRingBaseRadius,
            innerRingGroup, innerSegments, innerRingGap, innerRingRadius,
            innerRingSpeed: 0.01,
        };
    },

    applyPalette(model, p) {
        /* The lattice is the only themed part: the obsidian core and both rings have fixed
           colours by design. Its points carry their colour per vertex, with a little random
           lift on the green channel so the cloud is not one flat tone. */
        const colors = model.particleSystem.geometry.attributes.color.array;
        for (let i = 0; i < model.particleCount; i++) {
            colors[i * 3] = p.r;
            colors[i * 3 + 1] = Math.min(1, p.g + Math.random() * 0.15);
            colors[i * 3 + 2] = p.b;
        }
        model.particleSystem.geometry.attributes.color.needsUpdate = true;
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const t = ctx.time;

        const positions = model.particleSystem.geometry.attributes.position.array;
        for (let i = 0; i < model.particleCount; i++) {
            const base = model.basePositions[i];
            let displacement;

            if (isSpeaking) {
                const freqVal = (ctx.audioData[i % 32] || 0) / 255;
                displacement = Math.sin(t * 8 + i * 0.1) * (8 + freqVal * 25);
            } else if (isThinking) {
                displacement = Math.sin(t * 12 + base.x * 0.2) * Math.cos(t * 8 + base.y * 0.2) * 9;
            } else {
                // Slow, uniform breathing so the lattice never reads as frozen, plus the
                // click bump on top.
                displacement = ctx.click * 10 + Math.sin(t * 0.5) * 2.5;
            }

            const scale = 1 + displacement / model.latticeRadius;
            positions[i * 3] = base.x * scale;
            positions[i * 3 + 1] = base.y * scale;
            positions[i * 3 + 2] = base.z * scale;
        }
        model.particleSystem.geometry.attributes.position.needsUpdate = true;

        let rotSpeed = ctx.click * 0.01;
        if (isThinking) rotSpeed = 0.025;
        if (isSpeaking) rotSpeed = 0.01;
        model.particleSystem.rotation.y += rotSpeed;

        if (isThinking || isSpeaking) {
            model.particleSystem.rotation.x = Math.sin(t * 0.5) * 0.1;
            model.particleSystem.rotation.z = Math.cos(t * 0.4) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            model.particleSystem.rotation.x += (ctx.click * 0.12 - model.particleSystem.rotation.x) * 0.15;
            model.particleSystem.rotation.z += (ctx.click * 0.1 - model.particleSystem.rotation.z) * 0.15;
        }

        /* The peak, not the average, of the same 32-bin range the particles and the inner
           ring's segments actually read while speaking. ctx.audio is an average of bins
           0-15, and using it here let a loud bin anywhere in 16-31 push a particle or a
           segment out further than the bulge below accounted for, because that quieter
           average under-reported the real peak. */
        let maxSpeakingFreqVal = 0;
        if (isSpeaking) {
            for (let i = 0; i < 32; i++) {
                const v = (ctx.audioData[i] || 0) / 255;
                if (v > maxSpeakingFreqVal) maxSpeakingFreqVal = v;
            }
        }

        // How far the lattice is currently bulging beyond its resting radius -- used to push
        // both rings outward in step so they never overlap it, mid-click or mid-speech.
        let latticeBulge = 2.5 + ctx.click * 10;
        if (isSpeaking) latticeBulge = 8 + maxSpeakingFreqVal * 25;
        else if (isThinking) latticeBulge = 9;

        /* Inner ultramarine equalizer ring -- each segment thickens along the circumference
           to the live audio frequencies while speaking. The segments are children of the
           rotating group, so the thickening pattern rotates together with the ring. */
        let segReachEnvelope = 1 + ctx.click * 0.3;
        if (isSpeaking) segReachEnvelope = 1 + maxSpeakingFreqVal * 2.4;
        else if (isThinking) segReachEnvelope = 1.35;

        const targetInnerRadius = model.latticeRadius + model.innerRingGap + latticeBulge;
        model.innerRingRadius += (targetInnerRadius - model.innerRingRadius) * 0.12;

        model.innerSegments.forEach((seg, idx) => {
            let lenScale;
            if (isSpeaking) {
                const fVal = (ctx.audioData[idx % 32] || 0) / 255;
                lenScale = 1 + fVal * 2.4;
            } else if (isThinking) {
                lenScale = 1 + Math.sin(t * 14 + seg.userData.angle * 6) * 0.35;
            } else {
                lenScale = 1 + ctx.click * 0.3;
            }
            seg.scale.y = lenScale;
            seg.position.set(
                Math.cos(seg.userData.angle) * model.innerRingRadius,
                Math.sin(seg.userData.angle) * model.innerRingRadius,
                0
            );
        });

        const spinMultiplier = isThinking ? 3.0 : (isSpeaking ? 1.6 : 0.4 + ctx.click * 1.1);
        const innerRingSpinDelta = model.innerRingSpeed * spinMultiplier;
        model.innerRingGroup.rotation.z += innerRingSpinDelta;

        if (isSpeaking) {
            model.innerRingGroup.rotation.x = Math.sin(t * 2.4) * 0.24;
        } else if (isThinking) {
            model.innerRingGroup.rotation.x = Math.sin(t * 0.6) * 0.05;
        } else {
            model.innerRingGroup.rotation.x += (ctx.click * 0.1 - model.innerRingGroup.rotation.x) * 0.15;
        }

        /* Static outer ring -- kept perfectly flat (a true circle, not tilted into an
           ellipse), spinning clockwise at exactly half the inner ring's current speed so
           the two stay in visible lockstep through every state, while still tracking the
           inner ring's outward dodge. Its baked-in swell rides around the circumference as
           it spins, reading as a wave of motion. */
        const targetOuterRadius = model.innerRingRadius + 6 * segReachEnvelope + model.outerRingGap;
        model.outerRingRadius += (targetOuterRadius - model.outerRingRadius) * 0.12;
        model.outerRing.scale.setScalar(model.outerRingRadius / model.outerRingBaseRadius);
        model.outerRing.rotation.z -= innerRingSpinDelta * 0.5;

        let coreScale;
        if (isSpeaking) coreScale = 1 + ctx.audio * 0.5;
        else if (isThinking) coreScale = 1 + Math.sin(t * 15) * 0.2;
        else coreScale = 1 + ctx.click * 0.25;
        model.coreOrb.scale.setScalar(coreScale);

        model.coreOrb.rotation.y -= isThinking ? 0.05 : (isSpeaking ? 0.03 : ctx.click * 0.03);
    },
});
