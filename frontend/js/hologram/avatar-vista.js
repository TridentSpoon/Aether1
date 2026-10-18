/* Avatar: v.I.S.t.A. -- visual Intelligence, Surveillance, targeting and Assistance. The
 * eXcelsior Class's sixth, and the first that is not an object at all.
 *
 * Everything else in this line is a thing standing in space: a shell, a chassis, a dial, a
 * room. This one is the overlay painted on the glass you are looking through -- a tactical
 * grid behind, a reticle stack and four corner brackets framing it, and a drifting network
 * of drone nodes wiring themselves to whichever neighbours they happen to be near.
 *
 * Which is why the overlay cancels the HUD's drag-yaw and keeps facing you however far the
 * view is turned (the same trick Chrono-mAIstresse's dial uses; see core.js's pointermove
 * handler for where ctx.viewRotationY comes from). An overlay that swings away from the
 * viewer is not an overlay, it is a sign hanging in a room. The drone network does NOT get
 * that treatment -- it is out there in the world being watched, so it turns with the view,
 * and the two moving differently is what sells the glass between them.
 *
 * The one piece of real work here is the constellation lines. Which nodes are close enough
 * to wire together changes every frame, because the nodes drift, so unlike enXephalon's
 * fixed lattice this genuinely has to be re-derived -- but the buffer it is written into is
 * allocated once at build and only its draw range changes. Rebuilding the attribute every
 * frame, as the sketch this came from did, hands the GPU a new buffer sixty times a second
 * for data that fits in the one it already has.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'vista',
    label: 'v.I.S.t.A.',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const SPAN = 86;            // how far the overlay reaches from the middle
        const NODES = 45;
        const LINK = 24;            // nodes closer than this get wired together
        const MAX_LINKS = 420;      // the ceiling the line buffer is sized for
        // Fixed, not theme-tinted: the telemetry colour. The reticle and the brackets are
        // the theme's -- see applyPalette -- so the HUD's own colour reaches the overlay,
        // while what the network reports back stays the one colour that means telemetry.
        const TELEMETRY = 0x00ffaa;

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({
            map: api.helpers.glowTexture(64), color: api.palette.hex,
            transparent: true, opacity: 0.12, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(240, 240, 1);
        glow.position.z = -60;
        group.add(glow);

        /* The overlay: everything drawn on the glass. Held apart from the group so it can
           be turned back against the view each frame while the world behind it turns. */
        const overlay = new THREE.Group();
        group.add(overlay);

        /* The tactical grid behind the reticle. Built as loose segments so every vertex can
           carry its own brightness, dimmed by its distance from the middle: drawn additively
           a black vertex contributes nothing, so the grid fades out instead of ending on a
           visible edge. (GridHelper would be the obvious tool and is not in the vendored
           three.js bundle -- see scripts/build_vendor_three.sh.) */
        const REACH = SPAN * 1.3;      // the grid runs out to here
        const CELL = SPAN / 7;
        const gridPos = [];
        const gridFactors = [];
        const pushLine = (x1, y1, x2, y2) => {
            gridPos.push(x1, y1, -34, x2, y2, -34);
            [[x1, y1], [x2, y2]].forEach(([x, y]) => {
                // Measured against a radius well inside the grid's own extent, so every
                // vertex out at the rim is already black and the square the lines are
                // actually drawn on never shows as an edge.
                const r = Math.sqrt(x * x + y * y) / (SPAN * 0.82);
                gridFactors.push(Math.pow(Math.max(0, 1 - r), 1.6));
            });
        };
        const RINGS = Math.ceil(REACH / CELL);
        for (let n = -RINGS; n <= RINGS; n++) {
            const fixed = n * CELL;
            for (let s = 0; s < 16; s++) {
                const a = -REACH + (s / 16) * REACH * 2;
                const b = -REACH + ((s + 1) / 16) * REACH * 2;
                pushLine(fixed, a, fixed, b);
                pushLine(a, fixed, b, fixed);
            }
        }
        const gridGeom = new THREE.BufferGeometry();
        gridGeom.setAttribute('position', new THREE.Float32BufferAttribute(gridPos, 3));
        gridGeom.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(gridFactors.length * 3), 3));
        const gridMat = new THREE.LineBasicMaterial({
            vertexColors: true, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending,
        });
        const grid = new THREE.LineSegments(gridGeom, gridMat);
        overlay.add(grid);

        // --- The reticle stack: an aiming ring, a segmented outer reticle, and the four
        // corner brackets that frame the whole thing. ---
        const reticle = new THREE.Group();
        overlay.add(reticle);

        const ringMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.9,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const ring = new THREE.Mesh(new THREE.RingGeometry(SPAN * 0.3, SPAN * 0.318, 48), ringMat);
        reticle.add(ring);

        // Four arcs with gaps, not a second ring: the gaps are what shows it turning.
        const arcMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.8,
            side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
        });
        const arcs = new THREE.Group();
        for (let i = 0; i < 4; i++) {
            const step = (Math.PI * 2) / 4;
            arcs.add(new THREE.Mesh(
                new THREE.RingGeometry(SPAN * 0.47, SPAN * 0.495, 16, 1, i * step + 0.1, step - 0.2),
                arcMat
            ));
        }
        reticle.add(arcs);

        /* The brackets, in their own group so the lock below can draw them in without
           touching the rings. Each corner is two strokes meeting at a right angle, built
           from one corner's numbers mirrored into the other three -- four hand-written
           corners is four chances to typo a sign. */
        const bracketMat = new THREE.LineBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.95 });
        const bx = SPAN * 0.64;
        const by = SPAN * 0.5;
        const arm = SPAN * 0.13;
        const bracketPts = [];
        [[-1, 1], [1, 1], [-1, -1], [1, -1]].forEach(([sx, sy]) => {
            const corner = new THREE.Vector3(sx * bx, sy * by, 0);
            bracketPts.push(corner, new THREE.Vector3(sx * (bx - arm), sy * by, 0));
            bracketPts.push(corner.clone(), new THREE.Vector3(sx * bx, sy * (by - arm), 0));
        });
        const brackets = new THREE.LineSegments(
            new THREE.BufferGeometry().setFromPoints(bracketPts), bracketMat
        );
        reticle.add(brackets);

        /* The drone network: nodes adrift inside a box, bouncing off its walls. Outside the
           overlay group, so it stays out in the world and turns with the view. */
        const world = new THREE.Group();
        world.position.z = -18;      // behind the glass the overlay is painted on
        group.add(world);

        const BOX = { x: SPAN * 0.62, y: SPAN * 0.42, z: SPAN * 0.22 };
        const nodePos = new Float32Array(NODES * 3);
        const drift = [];
        for (let i = 0; i < NODES; i++) {
            nodePos[i * 3] = (Math.random() - 0.5) * BOX.x * 2;
            nodePos[i * 3 + 1] = (Math.random() - 0.5) * BOX.y * 2;
            nodePos[i * 3 + 2] = (Math.random() - 0.5) * BOX.z * 2;
            drift.push({
                x: (Math.random() - 0.5) * 0.14,
                y: (Math.random() - 0.5) * 0.14,
                z: (Math.random() - 0.5) * 0.07,
            });
        }
        const nodeGeom = new THREE.BufferGeometry();
        nodeGeom.setAttribute('position', new THREE.BufferAttribute(nodePos, 3));
        // Mapped, not bare: an unmapped point sprite is a hard square.
        const nodeMat = new THREE.PointsMaterial({
            map: api.helpers.glowTexture(32), color: TELEMETRY, size: 4.6,
            transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
            depthWrite: false, sizeAttenuation: true,
        });
        world.add(new THREE.Points(nodeGeom, nodeMat));

        // The constellation lines. One buffer, allocated here for the busiest frame the
        // ceiling allows; animate() rewrites its front and moves the draw range.
        const linkGeom = new THREE.BufferGeometry();
        const linkPos = new Float32Array(MAX_LINKS * 6);
        linkGeom.setAttribute('position', new THREE.BufferAttribute(linkPos, 3));
        linkGeom.setDrawRange(0, 0);
        const linkMat = new THREE.LineBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.3, blending: THREE.AdditiveBlending,
        });
        world.add(new THREE.LineSegments(linkGeom, linkMat));

        const model = {
            group, overlay, world, grid, gridGeom, gridMat, gridFactors,
            reticle, ring, ringMat, arcs, arcMat, brackets, bracketMat,
            nodeGeom, nodePos, nodeMat, drift, linkGeom, linkPos, linkMat,
            glow, glowMat, box: BOX, nodes: NODES, link: LINK, maxLinks: MAX_LINKS,
            tint: new THREE.Color(api.palette.hex3),
        };
        vistaPaintGrid(model);
        return model;
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        // The overlay stays on the glass: turned back by exactly what the drag turned the
        // view, so it faces the viewer at any angle. The world behind it is not corrected,
        // and the parallax between the two is what puts glass between them.
        model.overlay.rotation.y = -ctx.viewRotationY;
        model.world.rotation.y = Math.sin(ctx.time * 0.5) * 0.05;
        model.world.rotation.x = Math.cos(ctx.time * 0.3) * 0.03;

        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.55 : 0;

        // The rings turn against each other, and faster when it is working.
        const spin = isThinking ? 2.4 : isListening ? 0.6 : 1;
        model.ring.rotation.z = -ctx.time * 0.4 * spin;
        model.arcs.rotation.z = ctx.time * 0.2 * spin;
        model.ringMat.opacity = 0.7 + heat * 0.3;
        model.arcMat.opacity = 0.6 + heat * 0.35;

        /* The lock. Listening pulls the whole reticle in and brings the brackets tight
           around it, which is what acquiring something looks like; a click snaps it harder
           still for a moment. Everything else is the slow breath of a sight at rest. */
        const lock = (isListening ? 0.82 : 1) + Math.sin(ctx.time * 4) * 0.03 - ctx.click * 0.08;
        model.reticle.scale.set(lock, lock, 1);
        model.brackets.scale.setScalar(isListening ? 0.92 : 1 + Math.sin(ctx.time * 2) * 0.015);
        model.bracketMat.opacity = 0.75 + (isListening ? 0.25 : heat * 0.25) + ctx.click * 0.2;
        model.gridMat.opacity = 0.45 + heat * 0.25 + ctx.click * 0.15;

        /* The drones drift and bounce off the walls of their box. Faster while it is
           thinking: the network is being worked, not just watched. */
        const pace = (isThinking ? 2.2 : isListening ? 0.5 : 1) + ctx.audio * 0.8;
        const { nodePos, drift, box } = model;
        for (let i = 0; i < model.nodes; i++) {
            nodePos[i * 3] += drift[i].x * pace;
            nodePos[i * 3 + 1] += drift[i].y * pace;
            nodePos[i * 3 + 2] += drift[i].z * pace;
            // Turned at the wall rather than wrapped: a node that wraps crosses the whole
            // field in one frame and drags a constellation line across the picture with it.
            if (Math.abs(nodePos[i * 3]) > box.x) drift[i].x *= -1;
            if (Math.abs(nodePos[i * 3 + 1]) > box.y) drift[i].y *= -1;
            if (Math.abs(nodePos[i * 3 + 2]) > box.z) drift[i].z *= -1;
        }
        model.nodeGeom.attributes.position.needsUpdate = true;

        /* Rewire. The pairs really do change as the nodes drift, so this is re-derived --
           but on squared distances, because a square root per pair buys nothing when the
           only question is which side of a threshold the pair falls on. */
        const limit = model.link * model.link;
        const link = model.linkPos;
        let n = 0;
        for (let i = 0; i < model.nodes && n < model.maxLinks; i++) {
            for (let j = i + 1; j < model.nodes; j++) {
                const dx = nodePos[i * 3] - nodePos[j * 3];
                const dy = nodePos[i * 3 + 1] - nodePos[j * 3 + 1];
                const dz = nodePos[i * 3 + 2] - nodePos[j * 3 + 2];
                if (dx * dx + dy * dy + dz * dz > limit) continue;
                const at = n * 6;
                link[at] = nodePos[i * 3];
                link[at + 1] = nodePos[i * 3 + 1];
                link[at + 2] = nodePos[i * 3 + 2];
                link[at + 3] = nodePos[j * 3];
                link[at + 4] = nodePos[j * 3 + 1];
                link[at + 5] = nodePos[j * 3 + 2];
                if (++n >= model.maxLinks) break;
            }
        }
        model.linkGeom.setDrawRange(0, n * 2);
        model.linkGeom.attributes.position.needsUpdate = true;
        model.linkMat.opacity = 0.22 + heat * 0.25 + (isThinking ? 0.12 : 0);
        model.nodeMat.opacity = 0.75 + heat * 0.25 + ctx.click * 0.2;

        model.glowMat.opacity = 0.1 + (isSpeaking ? ctx.audio * 0.18 : Math.sin(ctx.time * 1.2) * 0.03 + 0.03);
    },

    applyPalette(model, palette) {
        model.ringMat.color.setHex(palette.hex);
        model.arcMat.color.setHex(palette.hex2);
        model.bracketMat.color.setHex(palette.hex);
        model.linkMat.color.setHex(palette.hex2);
        model.glowMat.color.setHex(palette.hex);
        model.tint.setHex(palette.hex3);
        vistaPaintGrid(model);
        // The drone nodes keep their fixed telemetry colour -- see build().
    },
});

/* Paints the grid's per-vertex brightness: the theme colour scaled by each vertex's own
   falloff, worked out once when the geometry was built. Shared by the first paint and by
   every theme change after it, because they are the same operation. */
function vistaPaintGrid(model) {
    const attr = model.gridGeom.attributes.color;
    for (let i = 0; i < model.gridFactors.length; i++) {
        attr.array[i * 3] = model.tint.r * model.gridFactors[i];
        attr.array[i * 3 + 1] = model.tint.g * model.gridFactors[i];
        attr.array[i * 3 + 2] = model.tint.b * model.gridFactors[i];
    }
    attr.needsUpdate = true;
}
