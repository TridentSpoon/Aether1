/* Avatar: A1 -- Monogram Placeholder.
 *
 * A point-cloud 'A' beside a point-cloud '1', modelled on Aether1's own name-mark rather
 * than a character or creature -- this is the avatar shown before an operator has
 * actually picked a persona/avatar (see the currentAvatar default in core.js), so it
 * deliberately reads as a blank slate: no eyes, no face, just the wordmark rendered as
 * scattered light, ASCII-art style, in three dimensions. The 'A' is denser than the '1'
 * so it still reads as the mark's more solid half without needing an actual solid mesh.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js -- rather than hand-attached
 * to HologramAvatar.prototype like the six hand-modelled built-ins, even though it ships
 * with Aether1 and is the default. Fully theme-tinted (no fixed obsidian-core piece, the
 * one convention this avatar deliberately skips) since it's a blank projection rather
 * than a physical object.
 */

// Samples `pointCount` points from inside an extruded shape's volume via bounding-box
// rejection sampling: draw a random point in the shape's own bounding box, keep it only
// if a ray from it crosses the volume's boundary an odd number of times (i.e. it's
// inside), repeat. DoubleSide is load-bearing on the sampling mesh's material, not
// cosmetic: a single-sided raycast only registers the *entry* crossing into a solid (the
// exit face is back-facing from the ray's own direction and gets culled), which inverts
// the odd/even parity this test depends on -- a point actually inside the shape would see
// zero forward crossings and read as outside. DoubleSide counts both, so parity means
// what it should.
function sampleVolumePoints(shape, extrudeSettings, pointCount) {
    const geo = new THREE.ExtrudeGeometry(shape, extrudeSettings);
    geo.center();
    geo.computeBoundingBox();
    const bbox = geo.boundingBox;

    const positions = new Float32Array(pointCount * 3);
    const raycaster = new THREE.Raycaster();
    const dummyMesh = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));

    let validPoints = 0;
    let attempts = 0;
    const maxAttempts = pointCount * 200; // guards a degenerate shape from ever hanging the build
    while (validPoints < pointCount && attempts < maxAttempts) {
        attempts++;
        const testPt = new THREE.Vector3(
            THREE.MathUtils.lerp(bbox.min.x, bbox.max.x, Math.random()),
            THREE.MathUtils.lerp(bbox.min.y, bbox.max.y, Math.random()),
            THREE.MathUtils.lerp(bbox.min.z, bbox.max.z, Math.random())
        );

        // Odd number of raycast intersections = point is inside the volume.
        raycaster.set(testPt, new THREE.Vector3(0, 0, 1));
        const hits = raycaster.intersectObject(dummyMesh, false);
        if (hits.length % 2 === 1) {
            positions[validPoints * 3] = testPt.x;
            positions[validPoints * 3 + 1] = testPt.y;
            positions[validPoints * 3 + 2] = testPt.z;
            validPoints++;
        }
    }
    return positions;
}

HologramAvatar.registerAvatar({
    id: 'a1',
    label: 'A1',

    build(api) {
        const group = new THREE.Group();

        const extrudeSettings = {
            steps: 2,
            depth: 15.6,
            bevelEnabled: true,
            bevelThickness: 2.6,
            bevelSize: 1.3,
            bevelSegments: 3
        };

        // --- 'A': slanted left leg / straight right leg, with a triangular counter so it
        // reads unambiguously as an 'A'. The counter's left edge (from its apex down to
        // where it meets the crossbar) and the crossbar's own bottom-left corner sit on
        // one straight line down to the foot -- not two mismatched slopes either side of
        // the crossbar -- so the whole inner-left boundary reads as a single straight
        // stroke edge, uninterrupted by the crossbar between them. ---
        const shapeA = new THREE.Shape();
        shapeA.moveTo(-54.6, -39.0);
        shapeA.lineTo(-13.0, 39.0);    // slanted left outer leg
        shapeA.lineTo(15.6, 39.0);     // apex top
        shapeA.lineTo(15.6, -39.0);    // straight vertical right leg outer
        shapeA.lineTo(-2.6, -39.0);    // inner right leg bottom
        shapeA.lineTo(-2.6, -10.4);    // inner crossbar right corner
        shapeA.lineTo(-21.528, -10.4); // inner crossbar bottom-left -- on the apex-to-foot line below
        shapeA.lineTo(-36.4, -39.0);   // inner left foot bottom
        shapeA.closePath();

        const holeA = new THREE.Path();
        holeA.moveTo(-13.416, 5.2); // counter bottom-left -- on the apex-to-foot line above
        holeA.lineTo(-2.6, 26.0);   // counter apex
        holeA.lineTo(-2.6, 5.2);    // counter bottom-right
        holeA.closePath();
        shapeA.holes.push(holeA);

        const aPositions = sampleVolumePoints(shapeA, extrudeSettings, 4500);
        const aPointCloudGeo = new THREE.BufferGeometry();
        aPointCloudGeo.setAttribute('position', new THREE.BufferAttribute(aPositions, 3));
        const aPointsMaterial = new THREE.PointsMaterial({ color: api.palette.hex, size: 2.1 });
        const aPoints = new THREE.Points(aPointCloudGeo, aPointsMaterial);
        aPoints.position.set(-13, 0, 0);
        group.add(aPoints);

        // --- '1': the same technique, sparser -- the "digital" half sitting beside the
        // 'A's denser half, same contrast the reference monogram this was built from drew
        // between its two halves. ---
        const shapeOne = new THREE.Shape();
        shapeOne.moveTo(15.6, 39.0);
        shapeOne.lineTo(0.0, 27.3);   // top-left beak notch
        shapeOne.lineTo(0.0, 16.9);   // beak thickness
        shapeOne.lineTo(15.6, 27.3);  // beak inner corner
        shapeOne.lineTo(15.6, -27.3); // main vertical stem left
        shapeOne.lineTo(2.6, -27.3);  // base left extension
        shapeOne.lineTo(2.6, -39.0);  // bottom left base
        shapeOne.lineTo(41.6, -39.0); // bottom right base
        shapeOne.lineTo(41.6, -27.3); // base right extension
        shapeOne.lineTo(31.2, -27.3); // main vertical stem right
        shapeOne.lineTo(31.2, 39.0);  // top right corner
        shapeOne.closePath();

        const onePositions = sampleVolumePoints(shapeOne, extrudeSettings, 1800);
        const onePointCloudGeo = new THREE.BufferGeometry();
        onePointCloudGeo.setAttribute('position', new THREE.BufferAttribute(onePositions, 3));
        const onePointsMaterial = new THREE.PointsMaterial({ color: api.palette.hex3, size: 1.6 });
        const onePoints = new THREE.Points(onePointCloudGeo, onePointsMaterial);
        onePoints.position.set(28.6, 0, 0);
        group.add(onePoints);

        // Soft backing glow so the monogram doesn't read as flat/empty at rest -- every
        // avatar here has some form of grounding accent (a core glow, a lens shell, an
        // ICE ring...).
        const glowMaterial = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64),
            color: api.palette.hex,
            transparent: true,
            opacity: 0.22,
            blending: THREE.AdditiveBlending,
            depthWrite: false
        });
        const glow = new THREE.Sprite(glowMaterial);
        glow.scale.set(160, 160, 1);
        glow.position.z = -20;
        group.add(glow);

        return { group, aPoints, aPointsMaterial, onePoints, onePointsMaterial, glow, glowMaterial };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // The whole monogram tilts as one unit -- both point clouds and the glow are all
        // children of model.group, so this alone carries them together.
        if (isThinking) {
            model.group.rotation.y = Math.sin(ctx.time * 1.4) * 0.25;
        } else if (isSpeaking) {
            model.group.rotation.y = Math.sin(ctx.time * 0.6) * 0.12;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            model.group.rotation.y += (ctx.click * 0.35 - model.group.rotation.y) * 0.15;
        }

        // The 'A' breathes gently as the mark's "body" -- a smaller, slower pulse than
        // the '1' below, so it reads as the steadier, denser half.
        const aScale = isSpeaking ? 1.0 + ctx.audio * 0.12 : 1.0 + ctx.click * 0.06;
        model.aPoints.scale.setScalar(aScale);
        model.aPointsMaterial.size = 2.1 + (isSpeaking ? ctx.audio * 0.8 : 0);

        // The point-cloud '1' breathes independently -- a faint drift while idle, a
        // livelier shimmer while speaking (driven by audio) or thinking (a fixed fast
        // pulse), same "state tells you what it's doing" convention as every other avatar.
        let oneScale;
        if (isSpeaking) {
            oneScale = 1.0 + ctx.audio * 0.5;
        } else if (isThinking) {
            oneScale = 1.0 + Math.sin(ctx.time * 12) * 0.15;
        } else {
            oneScale = 1.0 + Math.sin(ctx.time * 0.8) * 0.04;
        }
        model.onePoints.scale.setScalar(oneScale);
        model.onePointsMaterial.size = 1.6 + (isSpeaking ? ctx.audio * 1.2 : 0);

        let glowIntensity;
        if (isSpeaking) {
            glowIntensity = 0.22 + ctx.audio * 0.35;
        } else if (isThinking) {
            glowIntensity = 0.22 + Math.sin(ctx.time * 10) * 0.12;
        } else {
            glowIntensity = 0.18 + ctx.click * 0.25;
        }
        model.glowMaterial.opacity = glowIntensity;
    },

    applyPalette(model, palette) {
        model.aPointsMaterial.color.setHex(palette.hex);
        model.onePointsMaterial.color.setHex(palette.hex3);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
