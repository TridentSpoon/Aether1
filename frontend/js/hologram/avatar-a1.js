/* Avatar: A1 -- Monogram Placeholder.
 *
 * A point-cloud 'A' beside a point-cloud '1', modelled on Aether1's own name-mark rather
 * than a character or creature -- this is the avatar shown before an operator has
 * actually picked a persona/avatar (see the currentAvatar default in core.js), so it
 * deliberately reads as a blank slate: no eyes, no face, just the wordmark rendered as
 * scattered light, ASCII-art style, in three dimensions. The 'A' is denser than the '1'
 * so it still reads as the mark's more solid half without needing an actual solid mesh.
 * The '1's flag naturally reaches back far enough to overlap the 'A's right leg at the
 * top -- rather than let the two clouds mix there, the 'A' is sampled with that overlap
 * subtracted out, so the '1's own points show through cleanly where its top point cuts
 * through the 'A' instead of both colours crowding the same spot.
 *
 * Softness is a deliberate property of this avatar, not a side effect, and it is carried
 * by three separate things -- changing one alone will not keep the mark soft:
 *
 *   1. The letterforms' corners are rounded (see roundedOutline below). The 'A' and the
 *      '1' are still described as the same corner points they always were, so the mark's
 *      proportions are untouched; each corner is simply arrived at and left on a curve
 *      rather than a mitre. This is what stops the volume being sampled with hard points
 *      in the first place -- the cloud cannot be softer than the shape it fills.
 *   2. Each point is drawn as a soft round dot, not a square (the glowTexture map on the
 *      points materials). A bare PointsMaterial draws axis-aligned squares, which is what
 *      gave the mark its grainy, hard-edged read at any size.
 *   3. The extrusion's bevel is wider and more finely segmented, so the cloud's front and
 *      back faces fall away gradually in z instead of stopping at a chamfer.
 *
 * Tiers 3 and 4 -- an outer boundary and an ambient layer, in the avatar-parts kit's own
 * vocabulary -- are an aurora: a curtain of light standing around the monogram, and a
 * veil behind it. Two colourways ship, Borealis and Australis (see COLOURWAYS), which are
 * the one part of this avatar that does *not* follow the colour theme: a named colourway
 * whose colours moved with the theme would not be that colourway any more, the same
 * reasoning the README's "hot accents stay fixed" convention already applies to R.E.D.
 * 9000's lens and the Nexus's eyes. Everything else here is fully theme-tinted (no fixed
 * obsidian-core piece, the one convention this avatar deliberately skips) since the mark
 * is a blank projection rather than a physical object.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js -- rather than hand-attached
 * to HologramAvatar.prototype like the six hand-modelled built-ins, even though it ships
 * with Aether1 and is the default. The aurora is built here rather than borrowed from
 * parts.js on purpose: A1 is the default avatar, so reaching for the kit would pull its
 * whole catalogue into every launch to draw two layers (see avatar-loader.js's `needs`).
 * The kit carries its own Borealis/Australis parts for every other avatar to wear.
 */

// Walks a closed polygon given as [x, y] corner points and traces it with every corner
// rounded off: into each corner on a straight line, around it on a quadratic curve whose
// control point is the original mitre, out of it on the next straight line. The corner
// radius is clamped to half of each adjoining edge, per corner, so a tight notch (the
// '1's beak, the 'A's counter) rounds as far as it can without the two curves meeting in
// the middle of an edge and eating the stroke -- one radius can therefore be given for a
// whole letterform without checking its shortest edge by hand.
//
// `target` is a THREE.Shape or THREE.Path, which is to say this draws outer boundaries
// and holes identically; the caller decides which it is making.
function roundedOutline(target, points, radius) {
    const count = points.length;
    const corners = [];
    for (let i = 0; i < count; i++) {
        const prev = new THREE.Vector2().fromArray(points[(i - 1 + count) % count]);
        const corner = new THREE.Vector2().fromArray(points[i]);
        const next = new THREE.Vector2().fromArray(points[(i + 1) % count]);
        const toPrev = prev.sub(corner);
        const toNext = next.sub(corner);
        const r = Math.min(radius, toPrev.length() / 2, toNext.length() / 2);
        corners.push({
            corner,
            entry: corner.clone().add(toPrev.normalize().multiplyScalar(r)),
            exit: corner.clone().add(toNext.normalize().multiplyScalar(r)),
        });
    }

    target.moveTo(corners[0].exit.x, corners[0].exit.y);
    for (let i = 1; i <= count; i++) {
        const c = corners[i % count];
        target.lineTo(c.entry.x, c.entry.y);
        target.quadraticCurveTo(c.corner.x, c.corner.y, c.exit.x, c.exit.y);
    }
    target.closePath();
    return target;
}

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
// `exclude`, when given ({ shape, offset }), subtracts a second shape from the result: a
// point otherwise inside `shape` is rejected if it also falls inside `exclude.shape`.
// Both shapes are sampled/tested in their own centred-on-their-own-centroid space (same
// as the geometry each ends up rendered from), so `exclude.offset` carries a candidate
// point from `shape`'s centred space into `exclude.shape`'s centred space -- in practice
// the difference between the two Points objects' final positions in the group, i.e.
// where each actually ends up once placed side by side in the scene.
function sampleVolumePoints(shape, extrudeSettings, pointCount, exclude) {
    const geo = new THREE.ExtrudeGeometry(shape, extrudeSettings);
    geo.center();
    geo.computeBoundingBox();
    const bbox = geo.boundingBox;

    const positions = new Float32Array(pointCount * 3);
    const raycaster = new THREE.Raycaster();
    const dummyMesh = new THREE.Mesh(geo, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));

    let excludeMesh = null;
    if (exclude) {
        const excludeGeo = new THREE.ExtrudeGeometry(exclude.shape, extrudeSettings);
        excludeGeo.center();
        excludeMesh = new THREE.Mesh(excludeGeo, new THREE.MeshBasicMaterial({ side: THREE.DoubleSide }));
    }

    let validPoints = 0;
    let attempts = 0;
    const maxAttempts = pointCount * 300; // guards a degenerate shape from ever hanging the build
    const shiftedPt = new THREE.Vector3();
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
            shiftedPt.copy(testPt).add(exclude.offset);
            raycaster.set(shiftedPt, new THREE.Vector3(0, 0, 1));
            if (raycaster.intersectObject(excludeMesh, false).length % 2 === 1) continue;
        }

        positions[validPoints * 3] = testPt.x;
        positions[validPoints * 3 + 1] = testPt.y;
        positions[validPoints * 3 + 2] = testPt.z;
        validPoints++;
    }
    return positions;
}

/* The two aurora colourways, bottom-of-the-curtain to tip.
 *
 * Borealis is the northern reading: a dark base, the green that does most of the work in
 * a real aurora, teal through the middle, and the rare violet crown at the tips. Australis
 * is its opposite rather than a recolour -- the accent and the active colour trade ends,
 * so the warm rose that Borealis never reaches carries the body and the cold aqua arrives
 * at the tips, and `hang` flips so the curtain descends from above instead of rising from
 * below. Seen side by side they read as the same light over two hemispheres, which is the
 * whole point of shipping both rather than one with a hue slider.
 */
const COLOURWAYS = {
    borealis: {
        label: 'Borealis',
        stops: [0x04221a, 0x16d978, 0x2fe3ff, 0x8f6bff],
        hang: 1,
    },
    australis: {
        label: 'Australis',
        stops: [0x260a33, 0xff3d7f, 0xc86bff, 0x58ffe0],
        hang: -1,
    },
};

/* Which colourway this avatar wears, and whether it wears one at all ('none' leaves the
   monogram bare, as it was before tiers 3 and 4 existed). Read once per build so a
   change takes effect on the next avatar rebuild rather than mid-frame. */
function chosenColourway() {
    let saved = null;
    try {
        saved = window.localStorage.getItem('aether_a1_colourway');
    } catch (err) {
        saved = null; // private mode, or storage refused -- the default is still an avatar
    }
    if (saved === 'none') return null;
    return COLOURWAYS[saved] || COLOURWAYS.borealis;
}

// Samples a colourway's four stops as one continuous gradient, 0 at the curtain's base
// to 1 at its tip, and multiplies the result by `brightness` -- which is how both the
// side-edge and tip fades are done. The layers blend additively, so fading a colour
// toward black *is* fading it toward invisible; there is no per-vertex alpha to reach for.
const GRADIENT_SCRATCH = new THREE.Color();
function gradientAt(stops, t, brightness) {
    const span = stops.length - 1;
    const scaled = Math.min(span - 0.0001, Math.max(0, t) * span);
    const index = Math.floor(scaled);
    const colour = new THREE.Color(stops[index]);
    colour.lerp(GRADIENT_SCRATCH.setHex(stops[index + 1]), scaled - index);
    return colour.multiplyScalar(brightness);
}

/* Tier 3: the curtain. Six ribbons of light standing on a circle around the monogram,
   each a column of soft overlapping sprites running up the colourway's gradient.
   Sprites rather than a quad per ribbon, which is what this started as and is worth not
   going back to: the renderer's canvas has an alpha channel (see core.js), and an
   additively blended quad writes its material's alpha over its whole rectangle whatever
   its vertex colours say -- so a ribbon faded to black at its edges still composited as
   a visible grey slab with hard corners against the page behind the canvas. A sprite's
   alpha comes from its own soft texture, so a ribbon has no edge anywhere. */
function buildCurtain(api, way, radius) {
    const BANDS = 6;
    const BEADS = 14;
    const HEIGHT = 128;

    const group = new THREE.Group();
    const ribbons = [];
    const texture = api.helpers.glowTexture(64);

    for (let b = 0; b < BANDS; b++) {
        const centreAngle = (b / BANDS) * Math.PI * 2 + (b % 2) * 0.12;
        // Three heights and three leans, so six ribbons at one size do not read as a fence.
        const height = HEIGHT * (0.78 + (b % 4) * 0.12);
        const lean = ((b % 3) - 1) * 0.1;
        const beads = [];
        for (let i = 0; i < BEADS; i++) {
            const t = i / (BEADS - 1);
            // Out at the tip, in at the foot, and gone at both -- a ribbon that simply
            // stops at its lowest bead draws a hard line under the whole curtain.
            const brightness = (1 - Math.pow(t, 3.4)) * Math.min(1, t / 0.18);
            const mat = new THREE.SpriteMaterial({
                map: texture,
                color: gradientAt(way.stops, t, 1).getHex(),
                transparent: true,
                opacity: 0.42 * brightness,
                blending: THREE.AdditiveBlending,
                depthWrite: false,
            });
            const sprite = new THREE.Sprite(mat);
            sprite.scale.set(radius * 0.38, height * 0.26, 1);
            group.add(sprite);
            beads.push({ sprite, mat, t, brightness });
        }
        ribbons.push({ beads, centreAngle, height, lean, phase: b * 1.7 });
    }

    return { object: group, ribbons, radius, height: HEIGHT, hang: way.hang };
}

/* Tier 4: the veil. Three wide soft washes well behind the monogram and a thin scatter of
   points in front of them -- an ambient layer rather than a structure, which is the whole
   distinction between this tier and the one above. It sits at a fixed z and is deliberately
   not part of what a zoom scales (see the README's note on the custom avatar's tiers). */
function buildVeil(api, way) {
    const group = new THREE.Group();
    const washes = [];

    const bands = [
        { colour: way.stops[1], scale: [300, 120], y: 26 * way.hang, opacity: 0.20, speed: 0.11 },
        { colour: way.stops[2], scale: [240, 86], y: -4 * way.hang, opacity: 0.16, speed: -0.08 },
        { colour: way.stops[3], scale: [180, 58], y: 44 * way.hang, opacity: 0.12, speed: 0.15 },
    ];
    bands.forEach((band, i) => {
        const mat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64),
            color: band.colour,
            transparent: true,
            opacity: band.opacity,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
        });
        const sprite = new THREE.Sprite(mat);
        sprite.scale.set(band.scale[0], band.scale[1], 1);
        sprite.position.set(0, band.y, -86 + i * 6);
        group.add(sprite);
        washes.push({ sprite, mat, base: band, phase: i * 2.1 });
    });

    const DUST = 160;
    const dustPositions = new Float32Array(DUST * 3);
    const dustColors = new Float32Array(DUST * 3);
    for (let i = 0; i < DUST; i++) {
        dustPositions[i * 3] = (Math.random() - 0.5) * 300;
        dustPositions[i * 3 + 1] = (Math.random() - 0.5) * 190;
        dustPositions[i * 3 + 2] = -70 + Math.random() * 50;
        const colour = gradientAt(way.stops, 0.35 + Math.random() * 0.65, 0.6 + Math.random() * 0.4);
        dustColors[i * 3] = colour.r;
        dustColors[i * 3 + 1] = colour.g;
        dustColors[i * 3 + 2] = colour.b;
    }
    const dustGeom = new THREE.BufferGeometry();
    dustGeom.setAttribute('position', new THREE.BufferAttribute(dustPositions, 3));
    dustGeom.setAttribute('color', new THREE.BufferAttribute(dustColors, 3));
    const dustMat = new THREE.PointsMaterial({
        size: 1.5,
        vertexColors: true,
        map: api.helpers.glowTexture(32),
        transparent: true,
        opacity: 0.75,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
    });
    const dust = new THREE.Points(dustGeom, dustMat);
    group.add(dust);

    return { object: group, washes, dust, dustMat };
}

HologramAvatar.registerAvatar({
    id: 'a1',
    label: 'A1',

    build(api) {
        const group = new THREE.Group();

        // Wider, more finely segmented bevel than a chamfer: the cloud's faces fall away
        // in z rather than stopping, which is the third of the three softness carriers
        // described at the top of this file.
        const extrudeSettings = {
            steps: 2,
            depth: 15.6,
            bevelEnabled: true,
            bevelThickness: 3.4,
            bevelSize: 2.1,
            bevelSegments: 6
        };

        // --- 'A': slanted left leg / straight right leg, with a triangular counter so it
        // reads unambiguously as an 'A'. The counter's left edge (from its apex down to
        // where it meets the crossbar) and the crossbar's own bottom-left corner sit on
        // one straight line down to the foot -- not two mismatched slopes either side of
        // the crossbar -- so the whole inner-left boundary reads as a single straight
        // stroke edge, uninterrupted by the crossbar between them. Those corner points
        // are unchanged; roundedOutline is what turns each of them from a mitre into a
        // curve, so the letterform's proportions are exactly what they were. ---
        const shapeA = roundedOutline(new THREE.Shape(), [
            [-54.6, -39.0],  // outer left foot
            [-13.0, 39.0],   // slanted left outer leg
            [15.6, 39.0],    // apex top
            [15.6, -39.0],   // straight vertical right leg outer
            [-2.6, -39.0],   // inner right leg bottom
            [-2.6, -10.4],   // inner crossbar right corner
            [-21.528, -10.4], // inner crossbar bottom-left -- on the apex-to-foot line below
            [-36.4, -39.0],  // inner left foot bottom
        ], 5.2);

        shapeA.holes.push(roundedOutline(new THREE.Path(), [
            [-13.416, 5.2], // counter bottom-left -- on the apex-to-foot line above
            [-2.6, 26.0],   // counter apex
            [-2.6, 5.2],    // counter bottom-right
        ], 3.4));

        // --- '1': the same technique, sparser -- the "digital" half sitting beside the
        // 'A's denser half, same contrast the reference monogram this was built from drew
        // between its two halves. Its flag (the top-left corner, the numeral's "top
        // point") reaches back far enough that it geometrically overlaps the 'A's right
        // leg once both are placed -- see aPointsOffset/onePointsOffset below, and the
        // exclusion passed into the 'A's own sampling. ---
        const shapeOne = roundedOutline(new THREE.Shape(), [
            [15.6, 39.0],
            [0.0, 27.3],   // top-left beak notch
            [0.0, 16.9],   // beak thickness
            [15.6, 27.3],  // beak inner corner
            [15.6, -27.3], // main vertical stem left
            [2.6, -27.3],  // base left extension
            [2.6, -39.0],  // bottom left base
            [41.6, -39.0], // bottom right base
            [41.6, -27.3], // base right extension
            [31.2, -27.3], // main vertical stem right
            [31.2, 39.0],  // top right corner
        ], 4.2);

        // Where each point cloud's own centred geometry ends up once placed in the group
        // -- kept as named vectors (rather than inlined into position.set() calls below)
        // since the 'A's sampling also needs this same pair of offsets to know where the
        // '1' actually lands, to subtract it out correctly.
        const aPointsOffset = new THREE.Vector3(-13, 0, 0);
        const onePointsOffset = new THREE.Vector3(28.6, 0, 0);

        // A soft round dot per point, rather than PointsMaterial's default axis-aligned
        // square -- the second of the three softness carriers. depthWrite is off because
        // a transparent sprite that writes depth punches a hole in whatever it is drawn
        // in front of, which at this density means the far half of the cloud dropping out.
        const dotTexture = api.helpers.glowTexture(32);

        const aPositions = sampleVolumePoints(shapeA, extrudeSettings, 4500, {
            shape: shapeOne,
            offset: new THREE.Vector3().subVectors(aPointsOffset, onePointsOffset)
        });
        const aPointCloudGeo = new THREE.BufferGeometry();
        aPointCloudGeo.setAttribute('position', new THREE.BufferAttribute(aPositions, 3));
        const aPointsMaterial = new THREE.PointsMaterial({
            color: api.palette.hex,
            size: 3.4,
            map: dotTexture,
            transparent: true,
            opacity: 0.92,
            depthWrite: false,
        });
        const aPoints = new THREE.Points(aPointCloudGeo, aPointsMaterial);
        aPoints.position.copy(aPointsOffset);
        group.add(aPoints);

        const onePositions = sampleVolumePoints(shapeOne, extrudeSettings, 1800);
        const onePointCloudGeo = new THREE.BufferGeometry();
        onePointCloudGeo.setAttribute('position', new THREE.BufferAttribute(onePositions, 3));
        const onePointsMaterial = new THREE.PointsMaterial({
            color: api.palette.hex3,
            size: 2.7,
            map: dotTexture,
            transparent: true,
            opacity: 0.88,
            depthWrite: false,
        });
        const onePoints = new THREE.Points(onePointCloudGeo, onePointsMaterial);
        onePoints.position.copy(onePointsOffset);
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
        glow.scale.set(186, 186, 1);
        glow.position.z = -20;
        group.add(glow);

        const way = chosenColourway();
        const curtain = way ? buildCurtain(api, way, 104) : null;
        const veil = way ? buildVeil(api, way) : null;
        if (curtain) group.add(curtain.object);
        if (veil) group.add(veil.object);

        return {
            group, aPoints, aPointsMaterial, onePoints, onePointsMaterial, glow, glowMaterial,
            curtain, veil,
        };
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
        model.aPointsMaterial.size = 3.4 + (isSpeaking ? ctx.audio * 1.1 : 0);

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
        model.onePointsMaterial.size = 2.7 + (isSpeaking ? ctx.audio * 1.5 : 0);

        let glowIntensity;
        if (isSpeaking) {
            glowIntensity = 0.22 + ctx.audio * 0.35;
        } else if (isThinking) {
            glowIntensity = 0.22 + Math.sin(ctx.time * 10) * 0.12;
        } else {
            glowIntensity = 0.18 + ctx.click * 0.25;
        }
        model.glowMaterial.opacity = glowIntensity;

        // The curtain drifts: each sheet's own vertices slide around the circle and lean
        // in and out, further at the tip than at the base, so the sheet shears rather
        // than swaying as a board. A real aurora moves without ever moving as one piece.
        if (model.curtain) {
            const c = model.curtain;
            const sway = isSpeaking ? 1.0 + ctx.audio * 1.6 : isThinking ? 1.5 : 0.7;
            // Read straight off the world matrix rather than through getWorldQuaternion:
            // the vendored three bundle carries only the symbols the frontend already
            // names (see scripts/build_vendor_three.sh), and a Quaternion is not one of
            // them. Columns 2 and 10 are the matrix's z row, which is all this needs.
            const world = c.object.matrixWorld.elements;
            c.ribbons.forEach((ribbon) => {
                // A ribbon swung round to the front of the circle stands between the
                // viewer and the mark, so it is dimmed there -- the monogram is what this
                // avatar is, and an aurora that washes over it has stopped being a
                // backdrop. Behind the mark it burns at full strength.
                const toward = world[2] * Math.cos(ribbon.centreAngle) +
                    world[10] * Math.sin(ribbon.centreAngle);
                const depthFade = 1 - 0.68 * Math.max(0, toward);
                ribbon.beads.forEach((bead) => {
                    const ripple = Math.sin(ctx.time * 0.5 + ribbon.phase + bead.t * 3.4);
                    const angle = ribbon.centreAngle + (ripple * 0.06 + ribbon.lean) * sway * bead.t;
                    const r = c.radius + ripple * 7 * sway * bead.t;
                    bead.sprite.position.set(
                        Math.cos(angle) * r,
                        (-ribbon.height * 0.42 + bead.t * ribbon.height + ripple * 3 * sway) * c.hang,
                        Math.sin(angle) * r
                    );
                    bead.mat.opacity = bead.brightness * depthFade *
                        (0.42 + Math.sin(ctx.time * 0.8 + ribbon.phase) * 0.06 +
                         (isSpeaking ? ctx.audio * 0.22 : 0) + ctx.click * 0.1);
                });
            });
        }

        // The veil only breathes and drifts sideways; it is the backdrop, so anything
        // sharper read as a second avatar competing with the mark.
        if (model.veil) {
            model.veil.washes.forEach(({ sprite, mat, base, phase }) => {
                sprite.position.x = Math.sin(ctx.time * base.speed + phase) * 26;
                mat.opacity = base.opacity + Math.sin(ctx.time * 0.4 + phase) * 0.04 +
                    (isSpeaking ? ctx.audio * 0.12 : 0);
            });
            model.veil.dust.rotation.z = ctx.time * 0.01;
            model.veil.dustMat.opacity = 0.6 + Math.sin(ctx.time * 0.7) * 0.15 + ctx.click * 0.2;
        }
    },

    applyPalette(model, palette) {
        model.aPointsMaterial.color.setHex(palette.hex);
        model.onePointsMaterial.color.setHex(palette.hex3);
        model.glowMaterial.color.setHex(palette.hex);
        // The aurora is a named colourway and deliberately does not follow the theme --
        // see the note at the top of this file.
    },
});
