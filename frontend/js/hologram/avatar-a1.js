/* Avatar: A1 -- Monogram Placeholder.
 *
 * A solid extruded 'A' beside a point-cloud '1', modelled on Aether1's own name-mark
 * rather than a character or creature -- this is the avatar shown before an operator has
 * actually picked a persona/avatar (see the currentAvatar default in core.js), so it
 * deliberately reads as a blank slate: no eyes, no face, just the wordmark rendered in
 * three dimensions.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js -- rather than hand-attached
 * to HologramAvatar.prototype like the six hand-modelled built-ins, even though it ships
 * with Aether1 and is the default. Fully theme-tinted (no fixed obsidian-core piece, the
 * one convention this avatar deliberately skips) since it's a blank projection rather
 * than a physical object.
 */

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

        // --- 'A': a solid extruded glyph, slanted left leg / straight right leg, with a
        // clean triangular cutout so it reads unambiguously as an 'A'. Phong (not Basic)
        // because this half is meant to read as a lit, solid object -- the reference this
        // was built from calls it out explicitly as the "solid" half, next to the "pure
        // point-cloud" half below. ---
        const shapeA = new THREE.Shape();
        shapeA.moveTo(-54.6, -39.0);
        shapeA.lineTo(-13.0, 39.0);   // slanted left outer leg
        shapeA.lineTo(15.6, 39.0);    // apex top
        shapeA.lineTo(15.6, -39.0);   // straight vertical right leg outer
        shapeA.lineTo(-2.6, -39.0);   // inner right leg bottom
        shapeA.lineTo(-2.6, -10.4);   // inner crossbar right corner
        shapeA.lineTo(-28.6, -10.4);  // inner crossbar bottom
        shapeA.lineTo(-36.4, -39.0);  // inner left foot bottom
        shapeA.closePath();

        const holeA = new THREE.Path();
        holeA.moveTo(-20.8, 5.2);
        holeA.lineTo(-2.6, 26.0);
        holeA.lineTo(-2.6, 5.2);
        holeA.closePath();
        shapeA.holes.push(holeA);

        const geoA = new THREE.ExtrudeGeometry(shapeA, extrudeSettings);
        geoA.center();

        const fillMaterial = new THREE.MeshPhongMaterial({
            color: api.palette.hex,
            specular: 0xffffff,
            shininess: 80,
            flatShading: true,
            transparent: true,
            opacity: 0.92
        });
        const mesh = new THREE.Mesh(geoA, fillMaterial);
        mesh.position.set(-13, 0, 0);
        group.add(mesh);

        // A faint wireframe shell just outside the solid glyph -- reads as a projection
        // rather than solid matter, same convention as R.E.D. 9000's lens shell.
        const outlineMaterial = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, wireframe: true, transparent: true, opacity: 0.35
        });
        const outline = new THREE.Mesh(geoA.clone(), outlineMaterial);
        outline.position.copy(mesh.position);
        outline.scale.set(1.04, 1.04, 1.04);
        group.add(outline);

        // --- '1': a pure point-cloud glyph, no solid mesh underneath -- sampled inside
        // the extruded volume via bounding-box rejection sampling. The "digital" half
        // sitting beside the 'A's solid half. ---
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

        const geoOne = new THREE.ExtrudeGeometry(shapeOne, extrudeSettings);
        geoOne.center();
        geoOne.computeBoundingBox();
        const bbox = geoOne.boundingBox;

        const pointCount = 1800;
        const positions = new Float32Array(pointCount * 3);
        const raycaster = new THREE.Raycaster();
        // DoubleSide is load-bearing here, not cosmetic: a single-sided raycast only
        // registers the *entry* crossing into a solid (the exit face is back-facing from
        // the ray's own direction and gets culled), which inverts the odd/even parity this
        // inside-test depends on -- a point actually inside the shape sees zero forward
        // crossings and reads as outside. DoubleSide counts both, so parity means what it
        // should.
        const dummyMesh = new THREE.Mesh(geoOne, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));

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

        const pointCloudGeo = new THREE.BufferGeometry();
        pointCloudGeo.setAttribute('position', new THREE.BufferAttribute(positions, 3));

        const pointsMaterial = new THREE.PointsMaterial({ color: api.palette.hex3, size: 1.6 });
        const points = new THREE.Points(pointCloudGeo, pointsMaterial);
        points.position.set(28.6, 0, 0);
        group.add(points);

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

        return { group, mesh, fillMaterial, outline, outlineMaterial, points, pointsMaterial, glow, glowMaterial };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // The whole monogram tilts as one unit -- mesh, outline, points and glow are all
        // children of model.group, so this alone carries them together.
        if (isThinking) {
            model.group.rotation.y = Math.sin(ctx.time * 1.4) * 0.25;
        } else if (isSpeaking) {
            model.group.rotation.y = Math.sin(ctx.time * 0.6) * 0.12;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            model.group.rotation.y += (ctx.click * 0.35 - model.group.rotation.y) * 0.15;
        }

        const meshScale = isSpeaking ? 1.0 + ctx.audio * 0.15 : 1.0 + ctx.click * 0.08;
        model.mesh.scale.setScalar(meshScale);

        const wobble = isThinking ? 1.05 + Math.sin(ctx.time * 10) * 0.03 : 1.04;
        model.outline.scale.setScalar(wobble);

        // The point-cloud '1' breathes independently of the solid 'A' -- a faint drift
        // while idle, a livelier shimmer while speaking (driven by audio) or thinking (a
        // fixed fast pulse), same "state tells you what it's doing" convention as every
        // other avatar.
        let pointScale;
        if (isSpeaking) {
            pointScale = 1.0 + ctx.audio * 0.5;
        } else if (isThinking) {
            pointScale = 1.0 + Math.sin(ctx.time * 12) * 0.15;
        } else {
            pointScale = 1.0 + Math.sin(ctx.time * 0.8) * 0.04;
        }
        model.points.scale.setScalar(pointScale);
        model.pointsMaterial.size = 1.6 + (isSpeaking ? ctx.audio * 1.2 : 0);

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
        model.fillMaterial.color.setHex(palette.hex);
        model.outlineMaterial.color.setHex(palette.hex2);
        model.pointsMaterial.color.setHex(palette.hex3);
        model.glowMaterial.color.setHex(palette.hex);
    },
});
