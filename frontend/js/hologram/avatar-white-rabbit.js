/* Avatar: White Rabbit -- a blank silhouette of a sitting rabbit, drawn as scattered
 * light rather than a solid shape, the same "ASCII-art in three dimensions" idiom
 * A1's monogram uses. Front-facing: a round body and head, and two long ears that
 * mostly sit still but flick every few seconds, independently of each other, the way
 * a real rabbit's ears do -- not a continuous animation, a rare little twitch.
 *
 * No obsidian core, no fixed hot accent -- like A1, this is a blank projection rather
 * than a physical object, so it's fully theme-tinted throughout: the body one shade,
 * the ears a brighter accent shade, the same two-tone contrast A1 draws between its
 * denser 'A' and its sparser '1'.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

// Rejection-samples `count` points inside the union of a set of ellipses (each
// { cx, cy, rx, ry }), with a little z-jitter for volume. Plain 2D containment math --
// no raycasting needed, since every shape here is just a circle/ellipse.
function sampleEllipseCluster(ellipses, count, zJitter) {
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    ellipses.forEach(({ cx, cy, rx, ry }) => {
        minX = Math.min(minX, cx - rx); maxX = Math.max(maxX, cx + rx);
        minY = Math.min(minY, cy - ry); maxY = Math.max(maxY, cy + ry);
    });

    const positions = new Float32Array(count * 3);
    let filled = 0;
    let attempts = 0;
    const maxAttempts = count * 200;
    while (filled < count && attempts < maxAttempts) {
        attempts++;
        const x = THREE.MathUtils.lerp(minX, maxX, Math.random());
        const y = THREE.MathUtils.lerp(minY, maxY, Math.random());
        const inside = ellipses.some(({ cx, cy, rx, ry }) => {
            const dx = (x - cx) / rx, dy = (y - cy) / ry;
            return dx * dx + dy * dy <= 1;
        });
        if (!inside) continue;

        positions[filled * 3] = x;
        positions[filled * 3 + 1] = y;
        positions[filled * 3 + 2] = (Math.random() - 0.5) * zJitter;
        filled++;
    }
    return positions;
}

HologramAvatar.registerAvatar({
    id: 'white-rabbit',
    label: 'White Rabbit',

    build(api) {
        const group = new THREE.Group();

        // --- Body + head: one combined point cloud from two overlapping ellipses --
        // sampled separately and merged into a single BufferGeometry, so there's no
        // visible seam where they meet (points from both simply interleave there). ---
        const bodyPositions = sampleEllipseCluster(
            [
                { cx: 0, cy: -10, rx: 21, ry: 23 },
                { cx: 0, cy: 18, rx: 15, ry: 16 }
            ],
            2600, 10
        );
        const bodyGeo = new THREE.BufferGeometry();
        bodyGeo.setAttribute('position', new THREE.BufferAttribute(bodyPositions, 3));
        const bodyMaterial = new THREE.PointsMaterial({
            color: api.palette.hex, map: api.helpers.glowTexture(24), size: 2.0,
            transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const bodyPoints = new THREE.Points(bodyGeo, bodyMaterial);
        group.add(bodyPoints);

        // A soft nose/eye accent -- theme-tinted like the rest, not a fixed hot color,
        // since this avatar deliberately skips the "fixed core" convention entirely.
        const faceMaterial = new THREE.PointsMaterial({
            color: api.palette.hex3, map: api.helpers.glowTexture(20), size: 2.4,
            transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const facePositions = new Float32Array([
            -5, 20, 8, 5, 20, 8, 0, 14, 10 // two eyes, one nose
        ]);
        const faceGeo = new THREE.BufferGeometry();
        faceGeo.setAttribute('position', new THREE.BufferAttribute(facePositions, 3));
        const facePoints = new THREE.Points(faceGeo, faceMaterial);
        group.add(facePoints);

        // --- Ears: each its own point cloud, sampled in a LOCAL space where the ear's
        // base (where it meets the head) sits at the pivot's own origin -- so rotating
        // the pivot flicks the whole ear about its root, the way a real ear hinges. ---
        const earMaterial = new THREE.PointsMaterial({
            color: api.palette.hex3, map: api.helpers.glowTexture(20), size: 2.7,
            transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false
        });

        function buildEar(baseX, restAngle) {
            const positions = sampleEllipseCluster(
                [{ cx: 0, cy: 19, rx: 4.3, ry: 19 }], // base at y=0, tip at y=38
                520, 6
            );
            const geo = new THREE.BufferGeometry();
            geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
            const points = new THREE.Points(geo, earMaterial);

            const pivot = new THREE.Group();
            pivot.position.set(baseX, 30, 0);
            pivot.rotation.z = restAngle;
            pivot.add(points);
            group.add(pivot);

            return pivot;
        }

        const leftEarPivot = buildEar(-6.5, 0.22);
        const rightEarPivot = buildEar(6.5, -0.22);

        // Soft ambient glow behind everything, theme-tinted.
        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true,
            opacity: 0.18, blending: THREE.AdditiveBlending, depthWrite: false
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(150, 150, 1);
        glow.position.z = -20;
        group.add(glow);

        return {
            group, bodyPoints, bodyMaterial, facePoints, faceMaterial,
            leftEarPivot, rightEarPivot, earMaterial,
            // Each ear tracks its own next-twitch time and when its last twitch started,
            // seeded a little apart so the two ears don't flick in lockstep.
            leftEar: { nextTwitch: 1.5 + Math.random() * 3, twitchStart: -10, restAngle: 0.22 },
            rightEar: { nextTwitch: 3 + Math.random() * 3, twitchStart: -10, restAngle: -0.22 },
            glow, glowMaterial
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';
        const alert = isThinking || isSpeaking || isListening;

        // A faint idle sway, settling toward a click-triggered nod -- the same "front-
        // facing and still at rest" convention A1 uses.
        model.group.rotation.y += (ctx.click * 0.3 - model.group.rotation.y) * 0.12;
        model.group.rotation.x = Math.sin(ctx.time * 0.5) * 0.02;

        // The body breathes gently, a little more with the voice.
        const scale = 1 + Math.sin(ctx.time * 0.8) * 0.015 + (isSpeaking ? ctx.audio * 0.06 : 0) + ctx.click * 0.04;
        model.bodyPoints.scale.setScalar(scale);
        model.faceMaterial.opacity = 0.75 + (isSpeaking ? ctx.audio * 0.25 : Math.abs(Math.sin(ctx.time * 1.4)) * 0.15);

        // Ears: alert (thinking/listening/speaking) pulls them more upright and speeds
        // up how often they flick; idle lets them relax back to their resting splay.
        // A twitch itself is a short, damped flick -- not a continuous wiggle -- so most
        // of the time an ear is simply still.
        const alertLift = alert ? 0.12 : 0;
        const twitchIntervalMin = isThinking ? 1.2 : 2.8;
        const twitchIntervalMax = isThinking ? 3 : 7;
        const twitchDuration = 0.5;

        function updateEar(pivot, ear, side) {
            if (ctx.time > ear.nextTwitch) {
                ear.twitchStart = ctx.time;
                ear.nextTwitch = ctx.time + THREE.MathUtils.lerp(twitchIntervalMin, twitchIntervalMax, Math.random());
            }
            const dt = ctx.time - ear.twitchStart;
            let twitch = 0;
            if (dt >= 0 && dt < twitchDuration) {
                twitch = Math.sin(dt * 24) * 0.4 * (1 - dt / twitchDuration);
            }
            const idleSway = Math.sin(ctx.time * 1.05 + side * 1.7) * 0.02;
            const baseAngle = ear.restAngle + (-side) * alertLift; // alert stands them more upright/inward
            pivot.rotation.z = baseAngle + twitch + idleSway;
        }

        updateEar(model.leftEarPivot, model.leftEar, 1);
        updateEar(model.rightEarPivot, model.rightEar, -1);

        let glowIntensity;
        if (isSpeaking) glowIntensity = 0.16 + ctx.audio * 0.3;
        else if (isThinking) glowIntensity = 0.16 + Math.sin(ctx.time * 8) * 0.1;
        else glowIntensity = 0.14 + ctx.click * 0.2;
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.bodyMaterial.color.setHex(palette.hex);
        model.faceMaterial.color.setHex(palette.hex3);
        model.earMaterial.color.setHex(palette.hex3);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
