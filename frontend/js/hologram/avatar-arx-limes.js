/* Avatar: A.R.X.LIMES -- a black-hole hub inside a fractured dome of floating plates.
 *
 * The centre is a void: a faceted rounded cube, near-opaque and almost black, ringed by a
 * hot event horizon and a swirling accretion bloom. Six plates float around it -- two wide
 * ones hugging it above and below like eyelids, four wing blades further out at the
 * corners -- so the cluster reads as a wide almond eye. A click sends the wings diving
 * toward the hub and flinging back out past their resting radius, like grazing infalling
 * matter.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-limes',
    label: 'A.R.X.LIMES',
    group: 'The Umbrals',

    build(api) {
        const THREE = api.THREE;
        const h = api.helpers;
        const group = new THREE.Group();

        /* Shared materials -- one bright outline tone threads through every shard. Plates
           are real extruded slabs (see polygonShard's thickness), so depthWrite stays at
           its default true: each plate needs to occlude its own back face and side walls
           correctly, and to occlude and be occluded by the hub and its neighbours. Unlike
           a thin glow overlay this is genuine solid geometry. */
        const outlineMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.95 });
        const mainFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
        });
        const wingFillMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
        });
        const PLATE_THICKNESS = 10;

        /* --- Central hub: a black hole. A near-opaque void (no additive blending -- an
           additive near-black is invisible against the HUD's dark backdrop, and this needs
           to read as a solid absence, not a faint haze) ringed by a fixed hot event-horizon
           rim that, like every other avatar's one hot accent, never retints with the theme,
           plus a swirling accretion bloom standing in for infalling matter.

           A faceted "rounded" cube: a cube with its edges and corners cut back into facets,
           so the void reads as a cut block of nothing rather than a gem. The flat facets
           still catch the rim line. */
        const hubFillMat = new THREE.MeshBasicMaterial({
            color: 0x040209, side: THREE.DoubleSide, transparent: true, opacity: 0.94,
        });
        const hubRimMat = new THREE.LineBasicMaterial({ color: 0xffb347, transparent: true, opacity: 0.95 });
        const hub = h.roundedCube(14, 3, 0.45);
        const hubMesh = new THREE.Mesh(hub.geometry, hubFillMat);
        /* Drawn after the accretion glow (renderOrder -1 below), so the void stays a void:
           the glow's own depth test is off, and without an order the two would sort by
           distance and the bloom would wash straight over the hub. */
        hubMesh.renderOrder = 1;
        /* The facet grid comes back with the shape rather than from EdgesGeometry -- no
           single edge threshold draws both the shallow facets on a face and the sharp ones
           at a corner. See buildRoundedCube in geometry-helpers.js. */
        const hubOutline = new THREE.LineSegments(hub.edges, hubRimMat);
        hubOutline.renderOrder = 2;
        group.add(hubMesh);
        group.add(hubOutline);

        /* --- Accretion glow: a soft radial bloom around the hub rather than banded annuli,
           the same treatment as the kit's eye-lens core -- a sprite of a radial gradient,
           additive and depth-test off so it bleeds past the hub's silhouette on every side
           instead of being cut in half by it. Two of them, a tight hot one and a wider faint
           one, so the falloff has a shoulder rather than reading as one blob. The hotspots
           orbit on their own tilted plane, which is what makes the glow swirl rather than
           just sit there. A fixed hot accent, not theme-tinted -- it reads as infalling
           matter, not a UI element. */
        const accretionGroup = new THREE.Group();
        accretionGroup.rotation.x = 1.15;
        group.add(accretionGroup);

        const ring1Mat = new THREE.SpriteMaterial({
            map: h.radialGlowTexture(256, 'rgba(255,205,125,0.8)', 'rgba(255,105,20,0.26)'),
            transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
            depthWrite: false, depthTest: false,
        });
        const ring1 = new THREE.Sprite(ring1Mat);
        ring1.renderOrder = -1;
        ring1.scale.setScalar(52);
        group.add(ring1);

        const ring2Mat = new THREE.SpriteMaterial({
            map: h.radialGlowTexture(256, 'rgba(255,130,50,0.5)', 'rgba(255,55,0,0.14)'),
            transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending,
            depthWrite: false, depthTest: false,
        });
        const ring2 = new THREE.Sprite(ring2Mat);
        ring2.renderOrder = -1;
        ring2.scale.setScalar(86);
        group.add(ring2);

        const hotspotMat = new THREE.MeshBasicMaterial({
            color: 0xfff2c0, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const hotspots = [];
        for (let i = 0; i < 5; i++) {
            const dot = new THREE.Mesh(new THREE.SphereGeometry(0.9, 8, 8), hotspotMat);
            accretionGroup.add(dot);
            hotspots.push({ mesh: dot, angle: (i / 5) * Math.PI * 2, radius: 16 + (i % 2) * 5, speed: 0.01 + i * 0.002 });
        }

        /* --- Plate shapes, all defined locally extending toward +Y ("outward"), so every
           plate can share the same angle-to-world-orientation formula whatever its type. */
        const topShape = [{ x: -16, y: 0 }, { x: 16, y: 0 }, { x: 8, y: 13 }, { x: -8, y: 13 }];
        // The bottom anchor is larger than the top one, per spec. Both are wide and shallow
        // -- shaped like eyelid arcs hugging the hub -- rather than tall narrow trapezoids.
        const bottomShape = [{ x: -19, y: 0 }, { x: 19, y: 0 }, { x: 10, y: 16 }, { x: -10, y: 16 }];
        // An elongated, irregular wing blade swept toward +X; mirrored for the left side.
        const wingShapeRight = [{ x: -2, y: 0 }, { x: 2, y: 0 }, { x: 17, y: 26 }, { x: 4, y: 31 }];
        const wingShapeLeft = wingShapeRight.map((p) => ({ x: -p.x, y: p.y }));

        const plates = [];
        const addPlate = (shape, fillMat, worldAngleDeg, tier, radius) => {
            const worldAngle = worldAngleDeg * Math.PI / 180;
            // polygonShard already angles the far (outward) edge backward in Z while
            // keeping the near (hub-facing) edge flush -- no extra rotation needed here.
            const plateGroup = h.polygonShard(shape, 10, fillMat, outlineMat, PLATE_THICKNESS);
            plateGroup.position.set(Math.cos(worldAngle) * radius, Math.sin(worldAngle) * radius, 0);
            plateGroup.rotation.z = worldAngle - Math.PI / 2;
            group.add(plateGroup);
            plates.push({
                group: plateGroup,
                baseAngle: worldAngle,
                baseRadius: radius,
                phase: worldAngleDeg * 0.03,
                tier,
            });
        };

        // Top and bottom hug close to the hub like eyelids; the four wings sit further out
        // near the horizontal corners (0 and 180 degrees) like the pointed outer corners of
        // an eye -- together giving the cluster a wide, almond-eye silhouette.
        addPlate(topShape, mainFillMat, 90, 'main', 16);
        addPlate(bottomShape, mainFillMat, -90, 'main', 16);
        addPlate(wingShapeRight, wingFillMat, 15, 'wing', 26);
        addPlate(wingShapeRight, wingFillMat, -15, 'wing', 26);
        addPlate(wingShapeLeft, wingFillMat, 165, 'wing', 26);
        addPlate(wingShapeLeft, wingFillMat, -165, 'wing', 26);

        return {
            group,
            hubMesh, hubOutline, hubFillMat, hubRimMat,
            accretionGroup, ring1, ring2, ring1Mat, ring2Mat, hotspots, hotspotMat,
            plates, mainFillMat, wingFillMat, outlineMat,
        };
    },

    applyPalette(model, p) {
        /* The hub is a fixed black void, and its event-horizon rim and accretion bloom are
           this avatar's fixed hot accent -- none of the three retint with the theme, the
           same convention every other avatar's one constant identity colour follows. */
        model.mainFillMat.color.setHex(p.hex);
        model.wingFillMat.color.setHex(p.hex2);
        model.outlineMat.color.setHex(p.hex3);
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const t = ctx.time;

        if (isThinking || isSpeaking) {
            // No Y-axis spin -- the eye stays facing forward, only tilting to look around.
            model.group.rotation.x = Math.sin(t * 0.4) * 0.12;
            model.group.rotation.z = Math.sin(t * 0.3) * 0.1;
        } else {
            // Front-facing and still at rest -- only a faint click-triggered nod.
            model.group.rotation.x += (ctx.click * 0.1 - model.group.rotation.x) * 0.15;
            model.group.rotation.z += (ctx.click * 0.08 - model.group.rotation.z) * 0.15;
        }

        /* The hub's swell is deliberately small: it is a rounded cube at radius 14, not the
           geodesic at 9 that sat here once, so a larger peak would put the void outside the
           accretion disc entirely. These land it inside the outer bloom at full voice. */
        let hubScale;
        if (isSpeaking) hubScale = 1 + ctx.audio * 0.5;
        else if (isThinking) hubScale = 1 + Math.sin(t * 18) * 0.22;
        else hubScale = 1 + ctx.click * 0.3;
        model.hubOutline.scale.setScalar(hubScale);
        model.hubMesh.scale.setScalar(hubScale);

        const hubSpin = isThinking ? 0.03 : (isSpeaking ? 0.02 : ctx.click * 0.02);
        model.hubOutline.rotation.x += hubSpin;
        model.hubMesh.rotation.x = model.hubOutline.rotation.x;

        // The accretion glow: two camera-facing blooms around the hub that brighten with
        // speech and thought, plus the hotspots orbiting on the tilted plane.
        let glow = 1;
        if (isSpeaking) glow = 1 + ctx.audio * 0.6;
        else if (isThinking) glow = 1 + Math.abs(Math.sin(t * 10)) * 0.3;
        glow += ctx.click * 0.5;
        model.ring1Mat.opacity = Math.min(0.62 * glow, 1);
        model.ring2Mat.opacity = Math.min(0.38 * glow, 1);
        /* The blooms hang off the avatar group rather than the tilted hotspot plane, so a
           click has to reach both -- and a sprite's scale is its size, so it is set outright
           from the base rather than multiplied in. */
        const discScale = 1 + ctx.click * 0.15;
        model.accretionGroup.scale.setScalar(discScale);
        model.ring1.scale.setScalar(52 * discScale * (0.94 + glow * 0.08));
        model.ring2.scale.setScalar(86 * discScale * (0.94 + glow * 0.08));

        model.hotspots.forEach((spot) => {
            spot.angle += spot.speed;
            spot.mesh.position.set(Math.cos(spot.angle) * spot.radius, Math.sin(spot.angle) * spot.radius, 0);
        });
        model.hotspotMat.opacity = 0.7 + (glow - 1) * 0.3;

        /* A click sends the wing plates diving toward the black hole and flinging back out
           past their resting radius before settling, like grazing infalling matter, rather
           than a plain squash-to-sliver blink. ctx.click cannot drive this: it is a
           symmetric rise-then-decay bell curve (see the main animate loop), so the same
           value occurs once on the way up and once on the way down and cannot tell "falling
           in" from "shooting out". ctx.clickAge -- seconds since the click, monotonic --
           does. Keyframed radius multiplier: rest, sucked toward the hub, overshoot outward
           past rest, settled back, cosine-eased between each pair so the direction reversals
           are not abrupt. */
        const WING_REACT_DURATION = 0.7;
        const WING_RADIUS_KEYS = [
            { at: 0.0, v: 1.0 },
            { at: 0.32, v: 0.15 },
            { at: 0.62, v: 1.35 },
            { at: 1.0, v: 1.0 },
        ];
        let wingRadiusMult = 1;
        let wingScaleMult = 1;
        if (ctx.clickAge >= 0 && ctx.clickAge < WING_REACT_DURATION) {
            const tt = ctx.clickAge / WING_REACT_DURATION;
            for (let i = 0; i < WING_RADIUS_KEYS.length - 1; i++) {
                const a = WING_RADIUS_KEYS[i], b = WING_RADIUS_KEYS[i + 1];
                if (tt >= a.at && tt <= b.at) {
                    const localT = (tt - a.at) / (b.at - a.at);
                    const eased = 0.5 - 0.5 * Math.cos(localT * Math.PI);
                    wingRadiusMult = a.v + (b.v - a.v) * eased;
                    break;
                }
            }
            // Thins toward a sliver as it nears the hub (getting swallowed edge-on), then
            // swells past full width on the outward launch before settling back to normal.
            wingScaleMult = wingRadiusMult < 1
                ? Math.max(0.12, wingRadiusMult)
                : 1 + (wingRadiusMult - 1) * 0.4;
        }

        // Plates stay put -- static, floating in fixed position -- with only a faint
        // audio-reactive nudge while speaking, or the dive-and-launch above on a click.
        model.plates.forEach((plate, idx) => {
            let radiusMult = 1;
            if (plate.tier === 'wing') {
                radiusMult = wingRadiusMult;
            } else if (isSpeaking) {
                const fVal = (ctx.audioData[idx % 16] || 0) / 255;
                radiusMult = 1 + fVal * 0.06;
            } else if (ctx.click > 0) {
                radiusMult = 1 + ctx.click * 0.05;
            }
            const r = plate.baseRadius * radiusMult;
            plate.group.position.set(Math.cos(plate.baseAngle) * r, Math.sin(plate.baseAngle) * r, 0);

            if (plate.tier === 'wing') {
                plate.group.scale.setScalar(wingScaleMult);
            }
        });
    },
});
