/* Avatar: PRAXIS -- The eXcelsior Class's third. A training room that woke up, which is
 * why the room is most of the avatar: a lit floor grid running away underneath, a fainter
 * one overhead closing it in, hard-light blocks rising out of the floor and sinking back
 * as it runs drills, and a dark faceted core hanging in the middle of its own room with a
 * wireframe exoshell and two rings around it.
 *
 * The other avatars are a thing in empty space. This one is a place, and the thing in the
 * middle of it is what the place is thinking with. That is the whole idea: the drills are
 * not something it does to you, they are what it looks like from inside.
 *
 * The grid runs without ever moving very far. It scrolls by one cell and wraps, and every
 * vertex is dimmed by its own distance from the centre, so the rows the seam passes
 * through are already black by the time it gets there and the wrap never shows. That
 * vignette is baked into the geometry's own vertex colours rather than drawn over the top
 * of everything, which is what lets the theme reach it and what keeps it from dimming the
 * core standing in the middle of it.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

/* Paints one grid's per-vertex brightness: the theme colour scaled by each vertex's own
   falloff, worked out once when the geometry was built. Shared by the first paint and by
   every theme change after it, because they are the same operation. */
function praxisPaintGrids(model) {
    [[model.floor, model.tint], [model.ceiling, model.tint2]].forEach(([grid, colour]) => {
        const attr = grid.geom.attributes.color;
        for (let i = 0; i < grid.factors.length; i++) {
            attr.array[i * 3] = colour.r * grid.factors[i];
            attr.array[i * 3 + 1] = colour.g * grid.factors[i];
            attr.array[i * 3 + 2] = colour.b * grid.factors[i];
        }
        attr.needsUpdate = true;
    });
}

HologramAvatar.registerAvatar({
    id: 'praxis',
    label: 'PRAXIS',
    // The eXcelsior Class -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu, which groups the same way.
    group: 'The eXcelsior Class',

    build(api) {
        const group = new THREE.Group();

        const HALF = 72;        // the floor runs this far out in x and z
        const CELL = 6;         // one grid square -- fine enough that the vignette below has
                                // a dozen rings to fade across, instead of ending on one
                                // dim line that reads as a border
        const STEPS = 6;        // segments per grid line, so the fade can vary along it
        const FLOOR_Y = -50;
        const CEILING_Y = 50;

        // Fixed, not theme-tinted: the core is obsidian, the same convention every other
        // core in this engine follows. The room is what carries the theme.
        const OBSIDIAN = 0x111622;

        /* One surface of the room. Built as loose segments rather than whole lines so each
           vertex can carry its own brightness: a point's colour is the theme colour scaled
           by how far out it sits, reaching black at the rim. Drawn additively, a black
           vertex contributes nothing, so the lit part of the floor fades out on its own
           instead of being cut off. The platform still has a visible extent where the far
           rows pile up in perspective -- which is wanted here, since this is a room. */
        function grid(y, strength) {
            const positions = [];
            const factors = [];
            const push = (x1, z1, x2, z2) => {
                positions.push(x1, y, z1, x2, y, z2);
                [[x1, z1], [x2, z2]].forEach(([x, z]) => {
                    // Falls away faster than distance does, so the far rows are gone
                    // well before the rim rather than dimming evenly out to a visible edge.
                    const r = Math.sqrt(x * x + z * z) / HALF;
                    factors.push(Math.pow(Math.max(0, 1 - r), 1.2) * strength);
                });
            };
            for (let n = -HALF / CELL; n <= HALF / CELL; n++) {
                const fixed = n * CELL;
                for (let s = 0; s < STEPS * 2; s++) {
                    const a = -HALF + (s / (STEPS * 2)) * HALF * 2;
                    const b = -HALF + ((s + 1) / (STEPS * 2)) * HALF * 2;
                    push(fixed, a, fixed, b);   // lines running away from the viewer
                    push(a, fixed, b, fixed);   // lines running across
                }
            }
            const geom = new THREE.BufferGeometry();
            geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
            geom.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(factors.length * 3), 3));
            const mat = new THREE.LineBasicMaterial({
                vertexColors: true, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
            });
            return { mesh: new THREE.LineSegments(geom, mat), geom, mat, factors };
        }

        const floor = grid(FLOOR_Y, 1);
        const ceiling = grid(CEILING_Y, 0.45);
        group.add(floor.mesh, ceiling.mesh);

        /* Hard-light blocks: the room building something to throw at you. Wireframe only,
           so they read as projected rather than placed, each sitting on its own grid cell
           and rising out of the floor on its own phase. */
        // x, z and height in world units, not grid cells: where a block stands is a
        // composition decision and should not move every time the grid is made finer.
        const BLOCK_PLACES = [
            [-38, -26, 26], [26, -38, 34], [-14, 24, 20],
            [38, 12, 28], [-50, 12, 22], [14, 38, 30],
        ];
        const blockMat = new THREE.LineBasicMaterial({
            color: api.palette.hex2, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
        });
        const blocks = BLOCK_PLACES.map(([x, z, height], i) => {
            const box = new THREE.LineSegments(
                new THREE.EdgesGeometry(new THREE.BoxGeometry(11, height, 11)),
                blockMat
            );
            box.position.set(x, FLOOR_Y, z);
            return { box, height, phase: i * 1.1, rate: 0.5 + (i % 3) * 0.22 };
        });
        blocks.forEach(({ box }) => group.add(box));

        // --- The core: a dark faceted solid, lit by the scene rather than glowing, so it
        // reads as the one hard object in a room made of light. ---
        const coreMat = new THREE.MeshPhongMaterial({
            color: OBSIDIAN, shininess: 90, specular: 0x8fa4ff, flatShading: true,
        });
        const core = new THREE.Mesh(new THREE.IcosahedronGeometry(22, 1), coreMat);
        group.add(core);

        // The exoshell around it, turning the other way so the two never lock together.
        const exoMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.4,
        });
        const exo = new THREE.Mesh(new THREE.IcosahedronGeometry(31, 2), exoMat);
        group.add(exo);

        // A tight bloom inside the shell, so the core is not a dead lump when the room is
        // busy around it.
        const heartMat = new THREE.SpriteMaterial({
            map: api.helpers.radialGlowTexture(64, 'rgba(255,255,255,0.9)', 'rgba(120,60,255,0)'),
            transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const heart = new THREE.Sprite(heartMat);
        heart.scale.set(70, 70, 1);
        group.add(heart);

        // --- Two rings, each fixed on its own tilt and spinning within it. ---
        const ringAMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
        });
        const ringA = new THREE.Mesh(new THREE.TorusGeometry(40, 0.5, 8, 96), ringAMat);
        ringA.rotation.x = Math.PI / 3;
        const ringBMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
        });
        const ringB = new THREE.Mesh(new THREE.TorusGeometry(48, 0.7, 8, 96), ringBMat);
        ringB.rotation.y = Math.PI / 4;
        group.add(ringA, ringB);

        const model = {
            group, floor, ceiling, blocks, blockMat, core, coreMat, exo, exoMat,
            heart, heartMat, ringA, ringAMat, ringB, ringBMat,
            cell: CELL, floorY: FLOOR_Y,
            // Held so the per-vertex brightness below can be rebuilt on a theme change
            // without touching where any of the geometry sits.
            tint: new THREE.Color(api.palette.hex),
            tint2: new THREE.Color(api.palette.hex3),
        };
        praxisPaintGrids(model);
        return model;
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const isListening = ctx.state === 'LISTENING';

        // The room runs toward you. Wrapping at one cell is what makes a finite grid look
        // like an endless one, and the accumulated offset (rather than ctx.time scaled)
        // means a change of pace never jumps the floor sideways.
        const pace = (isThinking ? 46 : isListening ? 12 : 22) + ctx.audio * 30;
        model.scroll = ((model.scroll || 0) + pace * 0.016) % model.cell;
        model.floor.mesh.position.z = model.scroll;
        model.ceiling.mesh.position.z = -model.scroll;

        // Blocks rise out of the floor and sink back, faster when it is thinking, all the
        // way up on a click. A block is drawn from its own centre, so half its height is
        // what puts its base on the floor.
        const drill = isThinking ? 2.2 : isListening ? 0.5 : 1;
        model.blocks.forEach(({ box, height, phase, rate }) => {
            const lift = Math.max(0, Math.sin(ctx.time * rate * drill + phase));
            const raised = Math.min(1, lift + ctx.click);
            box.scale.y = 0.06 + raised * 0.94;
            box.position.y = model.floorY + (height * box.scale.y) / 2;
        });
        model.blockMat.opacity = 0.45 + (isThinking ? 0.3 : 0.15) + ctx.click * 0.25;

        // The core and its shell, turning against each other.
        model.core.rotation.y = ctx.time * 0.3;
        model.core.rotation.x = Math.sin(ctx.time * 0.2) * 0.2;
        model.exo.rotation.y = -ctx.time * 0.2;
        model.exo.rotation.z = ctx.time * 0.15;

        const heat = isSpeaking ? ctx.audio : isThinking ? Math.abs(Math.sin(ctx.time * 6)) * 0.55 : 0;
        model.core.scale.setScalar(1 + heat * 0.12 + ctx.click * 0.1);
        model.exo.scale.setScalar(1 + heat * 0.18 + ctx.click * 0.15);
        model.exoMat.opacity = 0.3 + heat * 0.35 + ctx.click * 0.2;
        model.heartMat.opacity = 0.35 + heat * 0.5;
        model.heart.scale.setScalar(70 * (1 + heat * 0.2));

        model.ringA.rotation.z = ctx.time * 0.5;
        model.ringB.rotation.x = ctx.time * 0.4;
        model.ringAMat.opacity = 0.5 + heat * 0.35;
        model.ringBMat.opacity = 0.6 + heat * 0.3;

        model.floor.mat.opacity = 0.8 + heat * 0.2;
        model.ceiling.mat.opacity = isListening ? 0.5 : 0.75 + heat * 0.25;
    },

    applyPalette(model, palette) {
        model.tint.setHex(palette.hex);
        model.tint2.setHex(palette.hex3);
        praxisPaintGrids(model);
        model.blockMat.color.setHex(palette.hex2);
        model.exoMat.color.setHex(palette.hex);
        model.ringAMat.color.setHex(palette.hex3);
        model.ringBMat.color.setHex(palette.hex);
        // The obsidian core keeps its own colour -- see build().
    },
});
