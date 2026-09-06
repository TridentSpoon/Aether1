/* Avatar: A1 -- Monogram Placeholder.
 *
 * A point-cloud 'A' with the '1' carved out of its own right leg -- subtracted from the
 * 'A's point cloud as a void, then refilled with its own sparser, contrasting-colour point
 * cloud exactly in that void -- rather than a separate glyph floating beside it. This is
 * the avatar shown before an operator has actually picked a persona/avatar (see the
 * currentAvatar default in core.js), so it deliberately reads as a blank slate: no eyes, no
 * face, just the wordmark rendered as scattered light, ASCII-art style, in three
 * dimensions. Embedding the '1' inside the 'A' (instead of setting it beside it) is what
 * makes "A1" read as one mark rather than two unrelated shapes sharing a frame.
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
//
// `excludeShape`, when given, is subtracted from the result: a point otherwise inside
// `shape` is rejected if it also lands inside `excludeShape` (tested the same
// odd/even-parity way, extruded with the same settings). Positions are returned in
// `shape`'s own raw coordinate space -- uncentred, so callers that need scale/rotation to
// pivot about a shape's own centroid must recentre the result themselves (see build()
// below, where the 'A' and the embedded '1' each need a *different* pivot).
function sampleVolumePoints(shape, extrudeSettings, pointCount, excludeShape) {
    const geo = new THREE.ExtrudeGeometry(shape, extrudeSettings);
    geo.computeBoundingBox();
    const bbox = geo.boundingBox;

    const positions = new Float32Array(pointCount * 3);
    const raycaster = new THREE.Raycaster();
    const dummyMesh = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
    const excludeMesh = excludeShape
        ? new THREE.Mesh(
              new THREE.ExtrudeGeometry(excludeShape, extrudeSettings),
              new THREE.MeshBasicMaterial({ side: THREE.DoubleSide })
          )
        : null;

    let validPoints = 0;
    let attempts = 0;
    const maxAttempts = pointCount * 300; // guards a degenerate shape from ever hanging the build
    while (validPoints < pointCount && attempts < maxAttempts) {
        attempts++;
        const testPt = new THREE.Vector3(
            THREE.MathUtils.lerp(bbox.min.x, bbox.max.x, Math.random()),
            THREE.MathUtils.lerp(bbox.min.y, bbox.max.y, Math.random()),
            THREE.MathUtils.lerp(bbox.min.z, bbox.max.z, Math.random())
        );

        // Odd number of raycast intersections = point is inside the volume.
        raycaster.set(testPt, new THREE.Vector3(0, 0, 1));
        if (raycaster.intersectObject(dummyMesh, false).length % 2 !== 1) continue;

        if (excludeMesh) {
            raycaster.set(testPt, new THREE.Vector3(0, 0, 1));
            if (raycaster.intersectObject(excludeMesh, false).length % 2 === 1) continue;
        }

        positions[validPoints * 3] = testPt.x;
        positions[validPoints * 3 + 1] = testPt.y;
        positions[validPoints * 3 + 2] = testPt.z;
        validPoints++;
    }
    return positions;
}

// Bounding-box centre of a shape's extrusion, in its own raw coordinate space -- used to
// recentre a point cloud onto its own centroid (so scale/rotation pivot correctly) and to
// work out how far that centroid sits from another shape's, when one is embedded inside
// the other.
function centerOf(shape, extrudeSettings) {
    const geo = new THREE.ExtrudeGeometry(shape, extrudeSettings);
    geo.computeBoundingBox();
    return geo.boundingBox.getCenter(new THREE.Vector3());
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

        // --- '1': placed to sit entirely inside the 'A's straight right leg (a full-height
        // solid strip, x from -2.6 to 15.6, untouched by the counter hole), inset a couple
        // of units from the leg's edges on every side. It's subtracted out of the 'A's
        // point cloud below, so this same shape doubles as the boundary of that cutout --
        // the leg's own dot density forms the numeral's silhouette, with a border of the
        // 'A's points always separating it from the leg's own edges. ---
        const shapeOne = new THREE.Shape();
        shapeOne.moveTo(9.0, 35.0);   // stem top-right
        shapeOne.lineTo(4.0, 35.0);   // stem top-left
        shapeOne.lineTo(4.0, 23.0);   // down to flag notch
        shapeOne.lineTo(-0.6, 28.5);  // flag outer tip
        shapeOne.lineTo(-0.6, 22.5);  // flag underside outer
        shapeOne.lineTo(4.0, 17.0);   // flag underside back to stem
        shapeOne.lineTo(4.0, -25.0);  // down the stem
        shapeOne.lineTo(-0.6, -25.0); // base left extension top
        shapeOne.lineTo(-0.6, -35.0); // base bottom-left
        shapeOne.lineTo(13.6, -35.0); // base bottom-right
        shapeOne.lineTo(13.6, -25.0); // base right extension top
        shapeOne.lineTo(9.0, -25.0);  // back to stem right
        shapeOne.closePath();

        // Both shapes are written in one shared raw coordinate frame (that's how shapeOne
        // above was placed inside the leg in the first place). The 'A' and the embedded
        // '1' each need to pivot on scale/rotation about their *own* centroid, though --
        // not each other's, or a scale pulse would visibly drag the '1' sideways as it
        // grows -- so each point cloud is recentred on its own centroid, and the '1's
        // cloud gets an explicit position offset (its own centroid minus the 'A's) to land
        // back in the leg once both are placed in the shared frame the 'A' is centred in.
        const aCenter = centerOf(shapeA, extrudeSettings);
        const oneCenter = centerOf(shapeOne, extrudeSettings);

        const aPositions = sampleVolumePoints(shapeA, extrudeSettings, 4500, shapeOne);
        const aPointCloudGeo = new THREE.BufferGeometry();
        aPointCloudGeo.setAttribute('position', new THREE.BufferAttribute(aPositions, 3));
        aPointCloudGeo.translate(-aCenter.x, -aCenter.y, -aCenter.z);
        const aPointsMaterial = new THREE.PointsMaterial({ color: api.palette.hex, size: 2.1 });
        const aPoints = new THREE.Points(aPointCloudGeo, aPointsMaterial);
        group.add(aPoints);

        // --- '1': the same sampling technique, sparser -- the "digital" accent sitting
        // inside the 'A's own leg rather than beside it, contrast kept via colour (hex3)
        // and density rather than separation now that it shares the leg's footprint. ---
        const onePositions = sampleVolumePoints(shapeOne, extrudeSettings, 700);
        const onePointCloudGeo = new THREE.BufferGeometry();
        onePointCloudGeo.setAttribute('position', new THREE.BufferAttribute(onePositions, 3));
        onePointCloudGeo.translate(-oneCenter.x, -oneCenter.y, -oneCenter.z);
        const onePointsMaterial = new THREE.PointsMaterial({ color: api.palette.hex3, size: 1.6 });
        const onePoints = new THREE.Points(onePointCloudGeo, onePointsMaterial);
        onePoints.position.set(oneCenter.x - aCenter.x, oneCenter.y - aCenter.y, oneCenter.z - aCenter.z);
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
        // the '1' embedded in its leg, so it reads as the steadier, denser half.
        const aScale = isSpeaking ? 1.0 + ctx.audio * 0.12 : 1.0 + ctx.click * 0.06;
        model.aPoints.scale.setScalar(aScale);
        model.aPointsMaterial.size = 2.1 + (isSpeaking ? ctx.audio * 0.8 : 0);

        // The point-cloud '1' breathes independently, but only gently -- it sits inset in
        // the 'A's leg with just a couple of units of clearance on every side, so its pulse
        // stays small enough that it never grows out past the border of 'A' points that
        // keeps it readable as its own carved-out shape.
        let oneScale;
        if (isSpeaking) {
            oneScale = 1.0 + ctx.audio * 0.15;
        } else if (isThinking) {
            oneScale = 1.0 + Math.sin(ctx.time * 12) * 0.06;
        } else {
            oneScale = 1.0 + Math.sin(ctx.time * 0.8) * 0.04;
        }
        model.onePoints.scale.setScalar(oneScale);
        model.onePointsMaterial.size = 1.6 + (isSpeaking ? ctx.audio * 0.5 : 0);

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
