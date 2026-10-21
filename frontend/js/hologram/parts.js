/* Avatar parts.
 *
 * A kit of pieces an avatar can be assembled from, rather than modelled by hand. The
 * seven hand-built avatars (A1, hAlcy, R.E.D. 9000, A.R.X.LIMES, A.R.X.LOGOS, A1ter_nul,
 * White Rabbit, Operator, and The Nexus / Nexus Sent) stay hand-built -- they do things
 * no kit could anticipate. This is for the other case: someone who wants an avatar of
 * their own and would rather choose than write three.js.
 *
 * Every part answers the same three questions, which is the whole reason they can be
 * mixed freely:
 *
 *   build(api, options) -> { object, applyPalette(palette), animate(ctx) }
 *
 * `object` is a THREE.Object3D to hang in the scene. `applyPalette` retints it when the
 * colour theme changes. `animate` is called once a frame with the same context an avatar
 * gets: { time, audio, audioData, click, state, palette }.
 *
 * Parts never read the palette at build time and keep it -- they are always told, so
 * that a theme switched later reaches every part including the ones not on screen.
 *
 * A core does not turn itself. The recipe has a spin setting, which turns the whole
 * assembly, and a core with a rotation of its own would ignore a spin of zero -- the one
 * setting someone reaches for when they want the thing to sit still. A fixed angle set
 * at build time is not motion and is fine (see the medallion's tilt). Reacting to the
 * voice, to a click or to the state is not motion either: scale, opacity and glow are
 * still every core's to use.
 *
 * Four tiers, matching the workbench's four pickers:
 *
 *   cores       -- the thing at the middle
 *   innerRings  -- a structure wrapping close around the core
 *   outerRings  -- a boundary further out, toward the edge of the shape
 *   effects     -- an ambient layer or background, not a fixed structure
 *
 * Some parts here are generic (built for this kit). Others are adapted from pieces of
 * the hand-built avatars. The rule is opt-out, not opt-in: every hand-built avatar's
 * components belong in this kit by default -- including ones still behind a Trace
 * Protocol unlock -- and a new avatar gets its pieces adapted in here as part of adding
 * it, unless a reason to keep it exclusive is written down next to its exception. The
 * current exceptions are the Nexus / Nexus Sent, A1, and White Rabbit, whose designs
 * stay theirs alone. (Note that avatar-senti.js registers under the id "senti" but its
 * label is "Nexus Sent" -- it *is* the Nexus Sent avatar under a different filename, not
 * a separate "Senti" avatar, so it falls under this same exception rather than being a
 * gap in the catalogue. White Rabbit was in the kit until its silhouette and ears were
 * withdrawn at the owner's request on 2026-09-20; the avatar itself is untouched.) The one
 * place a hand-built avatar and this kit knowingly draw the same thing is the aurora:
 * A1 wears a Borealis curtain and veil of its own, built in its own file because A1 is
 * the default avatar and borrowing from here would pull this whole catalogue into every
 * launch to draw two layers. The colourways themselves are not A1's design -- they are
 * the project's, so every avatar gets them. An adapted part is a fresh, simplified build of the same visual
 * idea using only api.helpers, not the original file's code -- it has to stand on its
 * own next to parts it never met.
 *
 * Used by avatar-custom.js (which renders a saved recipe) and available to any
 * hand-written avatar that wants a piece without writing one from scratch.
 */
(function () {
    'use strict';

    /* Audio comes in as 64 frequency bins, quiet to loud, low to high. Parts that
       react to sound read it through this so they all behave the same way: a value
       from 0 to 1 for a given band, with a floor so an idle avatar still breathes. */
    function band(ctx, index, count) {
        const data = ctx.audioData;
        if (!data || !data.length) return 0;
        const per = Math.max(1, Math.floor(data.length / count));
        const start = Math.min(data.length - 1, index * per);
        let sum = 0;
        for (let i = 0; i < per; i++) sum += data[Math.min(data.length - 1, start + i)] || 0;
        return sum / (per * 255);
    }

    // ---- Small shared geometry helpers ---------------------------------------
    // Hand-rolled from api.helpers.hexVertices rather than the engine's own
    // prototype methods (buildHexFill/buildHexOutline/buildPolygonShard), which a
    // standalone part has no access to -- see js/hologram/README.md's four-helper
    // contract.

    function hexFillMesh(api, radius, rotationOffset, material, cx = 0, cy = 0) {
        const geom = new THREE.CircleGeometry(radius, 6, rotationOffset);
        const mesh = new THREE.Mesh(geom, material);
        mesh.position.set(cx, cy, 0);
        return mesh;
    }

    // A real hex prism rather than a flat pane: a six-sided cylinder laid into the XY
    // plane with its front cap at z=0, so it reads as a solid gem with depth and does not
    // vanish edge-on when the scene is dragged round. Anything positioned in front of a
    // flat hex fill sits in exactly the same place in front of this.
    function hexPrismMesh(api, radius, rotationOffset, material, thickness) {
        const geom = new THREE.CylinderGeometry(radius, radius, thickness, 6, 1, false, rotationOffset);
        geom.rotateX(-Math.PI / 2);
        geom.translate(0, 0, -thickness / 2);
        return new THREE.Mesh(geom, material);
    }

    function hexOutlineLoop(api, radius, rotationOffset, material, cx = 0, cy = 0) {
        const geom = new THREE.BufferGeometry().setFromPoints(api.helpers.hexVertices(radius, rotationOffset, cx, cy));
        return new THREE.LineLoop(geom, material);
    }

    // One flat-ish irregular polygon "shard": points2D form a loop in local space with
    // its near edge at y=0, extending toward +y; the far edge recedes in Z so a cluster
    // of shards reads as facets of a convex dome.
    function polygonShard(points2D, bulge, fillMat, outlineMat) {
        const maxY = Math.max(...points2D.map((p) => p.y), 1);
        const verts = points2D.map((p) => new THREE.Vector3(p.x, p.y, -bulge * (p.y / maxY)));
        const positions = [];
        for (let i = 1; i < verts.length - 1; i++) {
            positions.push(verts[0].x, verts[0].y, verts[0].z);
            positions.push(verts[i].x, verts[i].y, verts[i].z);
            positions.push(verts[i + 1].x, verts[i + 1].y, verts[i + 1].z);
        }
        const geom = new THREE.BufferGeometry();
        geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
        geom.computeVertexNormals();
        const fillMesh = new THREE.Mesh(geom, fillMat);
        const outline = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(verts), outlineMat);
        const group = new THREE.Group();
        group.add(fillMesh, outline);
        return group;
    }

    // A faceted "rounded" cube: a cube whose edges and corners are cut back into facets.
    // Each face is a grid of quads and every grid point is pushed part of the way toward
    // the sphere it sits on, so the corners come in while the flat faces stay put. The
    // facet grid comes back as its own line segments because no EdgesGeometry threshold
    // draws both the shallow facets mid-face and the sharp ones at a corner.
    function roundedCube(radius, segments, round) {
        const faces = [
            { o: [-1, -1,  1], du: [ 2, 0,  0], dv: [0,  2,  0] },
            { o: [ 1, -1, -1], du: [-2, 0,  0], dv: [0,  2,  0] },
            { o: [ 1, -1,  1], du: [ 0, 0, -2], dv: [0,  2,  0] },
            { o: [-1, -1, -1], du: [ 0, 0,  2], dv: [0,  2,  0] },
            { o: [-1,  1,  1], du: [ 2, 0,  0], dv: [0,  0, -2] },
            { o: [-1, -1, -1], du: [ 2, 0,  0], dv: [0,  0,  2] },
        ];
        const corner = new THREE.Vector3(1, 1, 1);
        const scale = radius / corner.clone().lerp(corner.clone().normalize(), round).length();
        const point = (f, u, v) => {
            const p = new THREE.Vector3(
                f.o[0] + f.du[0] * u + f.dv[0] * v,
                f.o[1] + f.du[1] * u + f.dv[1] * v,
                f.o[2] + f.du[2] * u + f.dv[2] * v
            );
            return p.lerp(p.clone().normalize(), round).multiplyScalar(scale);
        };
        const tris = [];
        const lines = [];
        const push = (arr, ...pts) => pts.forEach((q) => arr.push(q.x, q.y, q.z));
        faces.forEach((f) => {
            const grid = [];
            for (let i = 0; i <= segments; i++) {
                grid.push([]);
                for (let j = 0; j <= segments; j++) grid[i].push(point(f, i / segments, j / segments));
            }
            for (let i = 0; i < segments; i++) {
                for (let j = 0; j < segments; j++) {
                    const a = grid[i][j], b = grid[i + 1][j], c = grid[i + 1][j + 1], d = grid[i][j + 1];
                    push(tris, a, b, c);
                    push(tris, a, c, d);
                    push(lines, a, b);
                    push(lines, a, d);
                    if (i === segments - 1) push(lines, b, c);
                    if (j === segments - 1) push(lines, d, c);
                }
            }
        });
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(tris, 3));
        geometry.computeVertexNormals();
        const edges = new THREE.BufferGeometry();
        edges.setAttribute('position', new THREE.Float32BufferAttribute(lines, 3));
        return { geometry, edges };
    }

    /* A cut gemstone, and the setting one sits in. Both are hexagonal and both are turned
       a half-step so the hexagon stands on a point rather than an edge -- which is what
       makes a gem read as tall and cut rather than as a bolt head.

       The gem is faceted with per-vertex colour rather than with light, because every
       material in this kit is a MeshBasicMaterial and the HUD's lighting never reaches
       one. The facet brightnesses are constants lit as if from the upper left, so the
       same facet is the same on every launch and at every angle. `tint` is the stone's
       own colour as a float triple; the table goes past 1 and comes out the pale version
       of it that a flat top catches. Shared by the brow stone here and by TithesA1s, whose
       stone is the same cut (see avatar-tithesa1s.js). */
    function cutGemGeometry(rx, ry, depth, tint) {
        const SIDES = 6;
        const TABLE = 0.46;        // how far in the flat top sits
        const pos = [];
        const col = [];

        const at = (i, scale, z) => {
            const a = (i / SIDES) * Math.PI * 2 + Math.PI / 2;
            return [Math.cos(a) * rx * scale, Math.sin(a) * ry * scale, z];
        };
        const push = (p, level) => {
            pos.push(p[0], p[1], p[2]);
            col.push(tint[0] * level, tint[1] * level, tint[2] * level);
        };

        for (let i = 0; i < SIDES; i++) {
            const j = (i + 1) % SIDES;
            const level = 0.34 + 0.72 * (0.5 + 0.5 * Math.cos((i / SIDES) * Math.PI * 2 - 2.2));
            const o1 = at(i, 1, 0);
            const o2 = at(j, 1, 0);
            const t1 = at(i, TABLE, depth);
            const t2 = at(j, TABLE, depth);
            push(o1, level); push(o2, level); push(t2, level * 1.12);
            push(o1, level); push(t2, level * 1.12); push(t1, level * 1.12);
            // The keel: back to the middle, dimmer, so a dragged view finds a solid and not a hole.
            const k = [0, 0, -depth * 0.55];
            push(o2, level * 0.3); push(o1, level * 0.3); push(k, level * 0.18);
        }
        for (let i = 1; i < SIDES - 1; i++) {
            push(at(0, TABLE, depth), 1.45);
            push(at(i, TABLE, depth), 1.45);
            push(at(i + 1, TABLE, depth), 1.45);
        }

        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
        geometry.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
        return geometry;
    }

    /* The flat hexagonal band the gem sits in. An inner radius of zero fills it in, which
       is how the dark socket under the stone is made from the same function. */
    function gemBezelGeometry(rx, ry, width) {
        const SIDES = 6;
        const pos = [];
        const at = (i, px, py) => {
            const a = (i / SIDES) * Math.PI * 2 + Math.PI / 2;
            return [Math.cos(a) * px, Math.sin(a) * py, 0];
        };
        for (let i = 0; i < SIDES; i++) {
            const j = (i + 1) % SIDES;
            const o1 = at(i, rx + width, ry + width);
            const o2 = at(j, rx + width, ry + width);
            const i1 = at(i, rx, ry);
            const i2 = at(j, rx, ry);
            pos.push(...o1, ...o2, ...i2);
            pos.push(...o1, ...i2, ...i1);
        }
        const geometry = new THREE.BufferGeometry();
        geometry.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
        return geometry;
    }

    // ---- Cores: the thing at the middle -------------------------------------
    //
    // Two things sat here and no longer do, so nobody re-adds them under the opt-out
    // rule: A1ter_nul's broken shard stack, which is a row of equaliser bars rather than
    // a core, and A.R.X.LIMES's old geodesic hub, which was the faceted crystal at a
    // smaller size. Both were withdrawn at the owner's request on 2026-09-20; neither
    // avatar was touched, and both still contribute to other tiers. LIMES's hub is a
    // faceted rounded cube now, a shape the kit did not have, so it is carried below.

    const CORES = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        orb: {
            label: 'Obsidian orb',
            build(api, options) {
                const mat = new THREE.MeshPhongMaterial({
                    color: 0x05070f, emissive: 0x0a1030, shininess: 90, specular: 0x8fa8ff,
                });
                const mesh = new THREE.Mesh(new THREE.SphereGeometry(options.size, 48, 48), mat);
                return {
                    object: mesh,
                    applyPalette() { /* fixed by design: an obsidian core is not tinted */ },
                    animate(ctx) {
                        const pulse = 1 + ctx.audio * 0.12 + ctx.click * 0.1;
                        mesh.scale.setScalar(pulse);
                    },
                };
            },
        },

        // A.R.X.LIMES's faceted hub is this same geodesic at a smaller size, so it is not
        // carried as a second core -- turn the size down instead.
        crystal: {
            label: 'Faceted crystal',
            build(api, options) {
                const geom = new THREE.IcosahedronGeometry(options.size, 1);
                const fill = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.28,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const edge = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.9 });
                const group = new THREE.Group();
                const mesh = new THREE.Mesh(geom, fill);
                const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geom, 12), edge);
                group.add(mesh, wire);
                return {
                    object: group,
                    applyPalette(p) { fill.color.setHex(p.hex); edge.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.scale.setScalar(1 + ctx.audio * 0.2 + ctx.click * 0.15);
                    },
                };
            },
        },

        // Adapted from A.R.X.LIMES's hub: a near-opaque void -- no additive blending, an
        // additive near-black is invisible against the HUD -- inside a bright facet grid.
        roundedCube: {
            label: 'Faceted rounded cube',
            build(api, options) {
                const { geometry, edges } = roundedCube(options.size, 3, 0.45);
                const fill = new THREE.MeshBasicMaterial({
                    color: 0x040209, side: THREE.DoubleSide, transparent: true, opacity: 0.94,
                });
                const edgeMat = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.95 });
                const group = new THREE.Group();
                group.add(new THREE.Mesh(geometry, fill), new THREE.LineSegments(edges, edgeMat));
                return {
                    object: group,
                    applyPalette(p) { edgeMat.color.setHex(p.hex); },
                    animate(ctx) {
                        group.scale.setScalar(1 + ctx.audio * 0.25 + ctx.click * 0.15);
                        edgeMat.opacity = Math.min(1, 0.7 + ctx.audio * 0.5 + ctx.click * 0.3);
                    },
                };
            },
        },

        eye: {
            label: 'Eye lens',
            build(api, options) {
                const group = new THREE.Group();
                const shell = new THREE.Mesh(
                    new THREE.SphereGeometry(options.size, 40, 40),
                    new THREE.MeshPhongMaterial({ color: 0x05070f, shininess: 120, specular: 0x7f95ff })
                );
                const lensMat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.85, side: THREE.DoubleSide,
                });
                const lens = new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.55, 48), lensMat);
                lens.position.z = options.size * 0.86;
                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.radialGlowTexture(128, 'rgba(255,240,180,1)', 'rgba(255,40,20,0)'),
                    transparent: true, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glow = new THREE.Sprite(glowMat);
                glow.scale.setScalar(options.size * 2.4);
                glow.position.z = options.size * 0.95;
                group.add(shell, lens, glow);
                return {
                    object: group,
                    applyPalette(p) { lensMat.color.setHex(p.hex); },
                    animate(ctx) {
                        const heat = 0.7 + ctx.audio * 1.6 + ctx.click * 0.5
                            + (ctx.state === 'THINKING' ? Math.abs(Math.sin(ctx.time * 3)) * 0.4 : 0);
                        glow.scale.setScalar(options.size * 2.4 * (0.8 + heat * 0.5));
                        glowMat.opacity = Math.min(1, 0.45 + heat * 0.4);
                    },
                };
            },
        },

        // Adapted from R.E.D. 9000: an obsidian eye with a hot lens glow on the front
        // face and a second, depth-test-off halo at its own centre so the glow bleeds
        // past the sphere's silhouette on every side, not just the front.
        redEye: {
            label: 'R.E.D. eye',
            build(api, options) {
                const group = new THREE.Group();
                const coreMat = new THREE.MeshPhongMaterial({
                    color: 0x0a0505, emissive: 0x3a0a04, specular: 0xff6a55, shininess: 90,
                    transparent: true, opacity: 0.97,
                });
                const core = new THREE.Mesh(new THREE.SphereGeometry(options.size, 40, 40), coreMat);
                const glowTexture = api.helpers.radialGlowTexture(128, '#fff26b', '#ff2200');
                const lensGlowMat = new THREE.SpriteMaterial({
                    map: glowTexture, transparent: true, opacity: 0.55,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const lensGlow = new THREE.Sprite(lensGlowMat);
                lensGlow.scale.setScalar(options.size * 1.35);
                lensGlow.position.z = options.size * 1.1;
                const innerGlowMat = new THREE.SpriteMaterial({
                    map: glowTexture, transparent: true, opacity: 0.35,
                    blending: THREE.AdditiveBlending, depthWrite: false, depthTest: false,
                });
                const innerGlow = new THREE.Sprite(innerGlowMat);
                innerGlow.scale.setScalar(options.size * 2.3);
                innerGlow.renderOrder = -1;
                group.add(core, lensGlow, innerGlow);
                return {
                    object: group,
                    applyPalette() { /* fixed: a hot lens stays hot regardless of theme */ },
                    animate(ctx) {
                        const heat = 0.5 + ctx.audio * 1.2 + ctx.click * 0.4
                            + (ctx.state === 'THINKING' ? Math.abs(Math.sin(ctx.time * 16)) * 0.35 : 0);
                        core.scale.setScalar(1 + ctx.audio * 0.15 + ctx.click * 0.12);
                        lensGlow.material.opacity = Math.min(1, 0.35 + heat * 0.4);
                        innerGlow.material.opacity = Math.min(0.7, 0.2 + heat * 0.28);
                        innerGlow.scale.setScalar(options.size * 2.3 * (1 + heat * 0.18));
                    },
                };
            },
        },

        // Adapted from A.R.X.LOGOS: its centrepiece, built the same way that avatar builds
        // it -- an extruded hex prism with the whole prism's edges traced (not a flat pane
        // and a flat outline loop), with the same fixed dark pupil and catchlight nested a
        // hair in front of it, at the same proportions and the same breathing scale. What
        // is left out is only what belongs to the rest of that avatar: the six spiral arms
        // that sweep shut over the eye, and the catchlight glint that answers them.
        hexEye: {
            label: 'Hex eye',
            build(api, options) {
                const group = new THREE.Group();
                const radius = options.size;
                const fillMat = new THREE.MeshBasicMaterial({
                    color: 0xe024c3, transparent: true, opacity: 0.22, blending: THREE.AdditiveBlending,
                });
                const outlineMat = new THREE.LineBasicMaterial({ color: 0xe024c3, transparent: true, opacity: 0.95 });
                const fill = hexPrismMesh(api, radius, 0, fillMat, radius * (12 / 26));
                const outline = new THREE.LineSegments(new THREE.EdgesGeometry(fill.geometry, 12), outlineMat);
                const pupilMat = new THREE.MeshBasicMaterial({ color: 0x050208, transparent: true, opacity: 0.92 });
                const pupil = hexFillMesh(api, radius * (9 / 26), 0, pupilMat);
                pupil.position.z = 0.5;
                const catchMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.5 });
                const catchlight = hexFillMesh(api, radius * (2.4 / 26), 0, catchMat, -radius * (4 / 26), radius * (4 / 26));
                catchlight.position.z = 1;
                group.add(fill, outline, pupil, catchlight);
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); outlineMat.color.setHex(p.hex); },
                    animate(ctx) {
                        const scale = ctx.state === 'SPEAKING' ? 1 + ctx.audio * 0.5
                            : ctx.state === 'THINKING' ? 1 + Math.sin(ctx.time * 16) * 0.18 : 1 + ctx.click * 0.15;
                        fill.scale.setScalar(scale);
                        outline.scale.setScalar(scale);
                    },
                };
            },
        },

        // Adapted from A.R.X.LOREGENDA: the squashed outer/inner facet-shell pair with
        // a small fixed-colour face (two lens eyes and a mouth bar) glowing through them,
        // simplified to a single nested-shell core rather than a full head-and-body rig.
        facetedHeadCore: {
            label: 'Faceted head',
            build(api, options) {
                const HOT = 0x00e1ff;
                const scaleY = 1.2, scaleZ = 0.85;
                const group = new THREE.Group();

                const shellMat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, side: THREE.DoubleSide, transparent: true, opacity: 0.22, depthWrite: false,
                });
                const outerWireMat = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.8 });

                const outerGeom = new THREE.IcosahedronGeometry(options.size, 1);
                outerGeom.scale(1, scaleY, scaleZ);
                const outerShell = new THREE.Mesh(outerGeom, shellMat);
                group.add(outerShell);
                group.add(new THREE.LineSegments(new THREE.EdgesGeometry(outerGeom), outerWireMat));

                const innerGeom = new THREE.IcosahedronGeometry(options.size * 0.68, 1);
                innerGeom.scale(1, scaleY, scaleZ);
                const innerShellMat = shellMat.clone();
                innerShellMat.opacity = 0.14;
                group.add(new THREE.Mesh(innerGeom, innerShellMat));

                const faceGroup = new THREE.Group();
                faceGroup.position.z = options.size * 0.68 * scaleZ * 1.1;
                group.add(faceGroup);

                const eyeMat = new THREE.MeshBasicMaterial({ color: HOT, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending });
                const eyeGeom = new THREE.OctahedronGeometry(options.size * 0.22, 0);
                eyeGeom.scale(0.75, 1.5, 0.45);
                const eyeOffset = options.size * 0.35;
                const leftEye = new THREE.Mesh(eyeGeom, eyeMat);
                leftEye.position.set(-eyeOffset, options.size * 0.12, 0);
                faceGroup.add(leftEye);
                const rightEye = new THREE.Mesh(eyeGeom, eyeMat);
                rightEye.position.set(eyeOffset, options.size * 0.12, 0);
                faceGroup.add(rightEye);

                const mouthMat = new THREE.MeshBasicMaterial({ color: HOT, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending });
                const mouth = new THREE.Mesh(new THREE.BoxGeometry(options.size * 0.55, options.size * 0.06, options.size * 0.05), mouthMat);
                mouth.position.set(0, -options.size * 0.32, 0);
                faceGroup.add(mouth);

                return {
                    object: group,
                    applyPalette(p) { shellMat.color.setHex(p.hex); outerWireMat.color.setHex(p.hex2); },
                    animate(ctx) {
                        const blinkCycle = 4.2, blinkWindow = 0.18;
                        const t = ctx.time % blinkCycle;
                        const blink = t < blinkWindow ? 1 - Math.pow(Math.sin((t / blinkWindow) * Math.PI), 2) : 1;
                        faceGroup.scale.y = blink;
                        const pulse = ctx.state === 'SPEAKING' ? 0.7 + ctx.audio * 0.3
                            : ctx.state === 'THINKING' ? 0.7 + Math.abs(Math.sin(ctx.time * 6)) * 0.3 : 0.7 + ctx.click * 0.3;
                        eyeMat.opacity = pulse;
                        mouthMat.opacity = pulse * 0.8;
                    },
                };
            },
        },

        // Adapted from A.R.X.LUCRE: the flattened-diamond shell (a squashed octahedron
        // laid across a plane) on its own, without the vertical cube stack or halo rings
        // that surround it in the original.
        flatDiamondCore: {
            label: 'Flat diamond',
            build(api, options) {
                const geom = new THREE.OctahedronGeometry(options.size, 0);
                geom.scale(1.5, 0.45, 1.5);
                const fillMat = new THREE.MeshBasicMaterial({
                    color: 0xffcc00, side: THREE.DoubleSide, transparent: true, opacity: 0.28, depthWrite: false,
                });
                const wireMat = new THREE.LineBasicMaterial({ color: 0xffcc00, transparent: true, opacity: 0.9 });
                const group = new THREE.Group();
                group.add(new THREE.Mesh(geom, fillMat));
                group.add(new THREE.LineSegments(new THREE.EdgesGeometry(geom), wireMat));
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); wireMat.color.setHex(p.hex2); },
                    animate(ctx) {
                        const breathe = 1 + Math.sin(ctx.time * 2) * 0.05 + ctx.audio * 0.15 + ctx.click * 0.15;
                        group.scale.setScalar(breathe);
                    },
                };
            },
        },

        // Adapted from A.R.X.LEXICO: the core cube at the centre of its cross -- a real
        // box rather than a flat isometric illusion, with a white aperture bead and a
        // radial glow sitting at the group's own origin, so it only ever brightens in
        // place. Keeps LEXICO's fixed hot cyan, which does not retint with the theme.
        apertureCube: {
            label: 'Aperture cube',
            build(api, options) {
                const HOT = 0x00ffff;
                const group = new THREE.Group();
                const fillMat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const wireMat = new THREE.LineBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9 });
                const side = options.size * 0.85;
                const geom = new THREE.BoxGeometry(side, side, side);
                const cube = new THREE.Mesh(geom, fillMat);
                const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geom), wireMat);
                const bead = new THREE.Mesh(
                    new THREE.SphereGeometry(options.size * 0.13, 12, 12),
                    new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.95 })
                );
                group.add(cube, wire, bead);
                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.radialGlowTexture(64, '#e0ffff', '#00ffff'),
                    transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glowScale = options.size * 2.1;
                const glow = new THREE.Sprite(glowMat);
                glow.scale.set(glowScale, glowScale, 1);
                group.add(glow);
                return {
                    object: group,
                    applyPalette() { /* fixed by design: LEXICO's aperture is its one unchanging colour */ },
                    animate(ctx) {
                        const pulse = 0.35 + ctx.audio * 0.5 + ctx.click * 0.3;
                        glowMat.opacity = Math.min(1, 0.3 + pulse * 0.5);
                        const s = glowScale * (1 + pulse * 0.2);
                        glow.scale.set(s, s, 1);
                    },
                };
            },
        },

        // Adapted from A.R.X.LYKSAUM: the HUD medallion as a core -- a real cylinder
        // tilted about 29 degrees off-axis so its rim and edge read as depth rather
        // than a flat sprite, carrying a glowing rim torus, a thin inset structural
        // ring and three accent dots mounted on the housing, which carries them at
        // whatever angle it sits at. Keeps LYKSAUM's fixed hot cyan for the rim and dots.
        discMedallion: {
            label: 'HUD medallion',
            build(api, options) {
                const HOT = 0x00e8ff;
                const BASE_TILT = -0.5;
                const group = new THREE.Group();
                group.rotation.x = BASE_TILT;
                const radius = options.size * 1.15;
                const thickness = radius * 0.24;
                const discMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex3, side: THREE.DoubleSide,
                    transparent: true, opacity: 0.22, depthWrite: false,
                });
                const discGeo = new THREE.CylinderGeometry(radius, radius, thickness, 48);
                discGeo.rotateX(Math.PI / 2);
                const wireMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.6 });
                group.add(new THREE.Mesh(discGeo, discMat));
                group.add(new THREE.LineSegments(new THREE.EdgesGeometry(discGeo), wireMat));

                const rimMat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.85,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                });
                group.add(new THREE.Mesh(new THREE.TorusGeometry(radius, radius * 0.038, 12, 48), rimMat));

                const structMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.55 });
                const structPts = [];
                for (let i = 0; i <= 64; i++) {
                    const a = (i / 64) * Math.PI * 2;
                    structPts.push(new THREE.Vector3(Math.cos(a) * radius * 0.81, Math.sin(a) * radius * 0.81, 0.5));
                }
                group.add(new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(structPts), structMat));

                // Top, left and right only: the bottom of a medallion is where a collar
                // arc usually sits, and a dot there fights it.
                const dotMat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.8,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                });
                [Math.PI / 2, Math.PI, 0].forEach((a) => {
                    const dot = new THREE.Mesh(new THREE.SphereGeometry(radius * 0.038, 10, 10), dotMat);
                    dot.position.set(Math.cos(a) * radius * 0.64, Math.sin(a) * radius * 0.64, thickness / 2 + 1.2);
                    group.add(dot);
                });
                return {
                    object: group,
                    applyPalette(p) {
                        discMat.color.setHex(p.hex3);
                        wireMat.color.setHex(p.hex2);
                        structMat.color.setHex(p.hex3);
                    },
                    animate(ctx) {
                        rimMat.opacity = 0.7 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },

        // Adapted from A.R.X.LOCAS: its core sphere on its own -- a dark shell with a
        // faint theme-tinted wire lattice floating just above it and a fixed hot bead
        // burning at the centre, so the sphere reads as a housing with something live
        // inside rather than a solid ball.
        latticeOrb: {
            label: 'Lattice orb',
            build(api, options) {
                const HOT = 0x00e8ff;
                const group = new THREE.Group();
                const shellMat = new THREE.MeshBasicMaterial({ color: 0x061018, transparent: true, opacity: 0.92 });
                group.add(new THREE.Mesh(new THREE.SphereGeometry(options.size, 24, 18), shellMat));
                const wireMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex3, wireframe: true, transparent: true, opacity: 0.3,
                });
                group.add(new THREE.Mesh(new THREE.SphereGeometry(options.size * 1.02, 14, 10), wireMat));
                const hotMat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(new THREE.SphereGeometry(options.size * 0.32, 16, 12), hotMat));
                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.radialGlowTexture(64, '#c8feff', '#00a8cc'),
                    transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glowScale = options.size * 2.4;
                const glow = new THREE.Sprite(glowMat);
                glow.scale.set(glowScale, glowScale, 1);
                group.add(glow);
                return {
                    object: group,
                    applyPalette(p) { wireMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        const pulse = ctx.audio * 0.5 + ctx.click * 0.3;
                        hotMat.opacity = 0.8 + pulse * 0.2;
                        glowMat.opacity = Math.min(1, 0.45 + pulse * 0.5);
                    },
                };
            },
        },

        // Adapted from A.R.X.LEGIONARE: the crystal hanging inside its pyramid -- a bare
        // icosahedron with no wireframe over it, lit by a fixed ember glow rather than
        // the theme.
        emberCrystal: {
            label: 'Ember crystal',
            build(api, options) {
                const HOT = 0xff3300;
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(new THREE.IcosahedronGeometry(options.size * 0.8, 0), mat));
                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.radialGlowTexture(64, '#ffcf9e', '#ff3300'),
                    transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glowScale = options.size * 2.4;
                const glow = new THREE.Sprite(glowMat);
                glow.scale.set(glowScale, glowScale, 1);
                group.add(glow);
                return {
                    object: group,
                    applyPalette() { /* fixed by design: the ember is LEGIONARE's one constant colour */ },
                    animate(ctx) {
                        const pulse = ctx.audio * 0.6 + ctx.click * 0.35 + (Math.sin(ctx.time * 3) + 1) * 0.12;
                        mat.opacity = 0.75 + Math.min(0.25, pulse * 0.25);
                        glowMat.opacity = Math.min(1, 0.35 + pulse * 0.55);
                    },
                };
            },
        },

        // Adapted from Operator: the caret at the centre of its tunnel. Just the "|>" --
        // no printed line behind it -- breathing with a CRT flicker rather than a hard
        // on/off blink, and holding a steady voice-reactive glow while speaking.
        terminalPrompt: {
            label: 'Terminal prompt',
            build(api, options) {
                function caretTexture(size) {
                    const canvas = document.createElement('canvas');
                    canvas.width = size;
                    canvas.height = size;
                    const ctx2d = canvas.getContext('2d');
                    ctx2d.fillStyle = '#ffffff';
                    ctx2d.font = `bold ${Math.round(size * 0.62)}px monospace`;
                    ctx2d.textAlign = 'center';
                    ctx2d.textBaseline = 'middle';
                    ctx2d.fillText('|>', size / 2, size / 2);
                    const texture = new THREE.CanvasTexture(canvas);
                    texture.needsUpdate = true;
                    return texture;
                }

                const group = new THREE.Group();

                const caretMat = new THREE.SpriteMaterial({
                    map: caretTexture(72), color: api.palette.hex,
                    transparent: true, opacity: 1, depthWrite: false, blending: THREE.AdditiveBlending,
                });
                const caret = new THREE.Sprite(caretMat);
                const caretSize = options.size * 0.6;
                caret.scale.set(caretSize, caretSize, 1);
                caret.position.set(0, 0, 1);
                group.add(caret);

                // A soft halo right behind the caret, purely so it reads clearly against
                // whatever sits around it instead of getting lost in it.
                const haloMat = new THREE.SpriteMaterial({
                    map: api.helpers.glowTexture(48), color: api.palette.hex,
                    transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const halo = new THREE.Sprite(haloMat);
                const haloSize = options.size * 0.8;
                halo.scale.set(haloSize, haloSize, 1);
                halo.position.set(0, 0, 0);
                group.add(halo);

                return {
                    object: group,
                    applyPalette(p) {
                        caretMat.color.setHex(p.hex);
                        haloMat.color.setHex(p.hex);
                    },
                    animate(ctx) {
                        const speed = ctx.state === 'THINKING' ? 3.2 : 1.4;
                        const caretOpacity = ctx.state === 'SPEAKING'
                            ? 0.75 + ctx.audio * 0.4
                            : 0.72 + (Math.sin(ctx.time * speed) * 0.5 + 0.5) * 0.28;
                        caretMat.opacity = caretOpacity;
                        haloMat.opacity = 0.35 + caretOpacity * 0.35;
                    },
                };
            },
        },

        /* Adapted from m.A.I.r.a.d.: its faceted white shell with a lit lattice caged inside
           it and a single optic sunk into the front. The avatar turns the shell and the
           cage against each other; a core may not turn itself (see the note at the top of
           this file), so here the cage breathes and brightens instead and the recipe's
           spin setting is left to decide whether any of it moves. */
        facetedShellOptic: {
            label: 'Faceted shell and optic',
            build(api, options) {
                const OPTIC = 0xff0e8c;
                const CAGE = 0xc02cff;
                const group = new THREE.Group();

                const cageMat = new THREE.MeshBasicMaterial({
                    color: CAGE, wireframe: true, transparent: true, opacity: 0.7,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                });
                group.add(new THREE.Mesh(new THREE.IcosahedronGeometry(options.size * 0.86, 2), cageMat));

                const shellMat = new THREE.MeshPhongMaterial({
                    color: 0xeef3fa, shininess: 150, specular: 0xffffff, flatShading: true,
                    transparent: true, opacity: 0.82,
                });
                group.add(new THREE.Mesh(new THREE.IcosahedronGeometry(options.size, 2), shellMat));

                // The optic stands proud of the shell: a subdivided icosahedron's faces
                // sit inside its radius, so a lens at exactly the radius lands behind
                // them. Dark socket first -- additive over a near-white shell is white.
                const optic = new THREE.Group();
                optic.position.z = options.size * 1.07;
                group.add(optic);
                const socketMat = new THREE.MeshBasicMaterial({ color: 0x0a0410, transparent: true, opacity: 0.92 });
                optic.add(new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.29, 32), socketMat));
                const irisMat = new THREE.MeshBasicMaterial({
                    color: OPTIC, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                optic.add(new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.2, 32), irisMat));
                const pupilMat = new THREE.MeshBasicMaterial({
                    color: 0xffffff, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
                });
                const pupil = new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.075, 24), pupilMat);
                pupil.position.z = 0.6;
                optic.add(pupil);

                return {
                    object: group,
                    applyPalette() {},   // shell, cage and optic are fixed colours
                    animate(ctx) {
                        const heat = ctx.audio * 0.6 + ctx.click * 0.4;
                        cageMat.opacity = 0.55 + heat * 0.4;
                        shellMat.opacity = 0.86 - heat * 0.14;
                        irisMat.opacity = 0.7 + heat * 0.3;
                        pupil.scale.setScalar(0.85 + Math.sin(ctx.time * 4) * 0.15 + heat * 0.4);
                    },
                };
            },
        },

        /* Adapted from Chrono-mAIstresse: the lit dial with hour marks and two hands.
           The hands read the machine's own clock, which is not the self-rotation the note
           at the top of this file rules out -- spin turns the dial, and a clock whose
           hands stop when you set spin to zero is a picture of a clock rather than one. */
        chronoDial: {
            label: 'Chronometer dial',
            build(api, options) {
                const group = new THREE.Group();
                const size = options.size;

                const dialMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.3, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(new THREE.CircleGeometry(size, 64), dialMat));

                const markMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.85 });
                const marks = [];
                for (let i = 0; i < 12; i++) {
                    // Straight up is twelve, running clockwise, the same frame the hands use.
                    const a = Math.PI / 2 - (i / 12) * Math.PI * 2;
                    const inner = size * (i % 3 === 0 ? 0.82 : 0.88);
                    marks.push(new THREE.Vector3(Math.cos(a) * inner, Math.sin(a) * inner, 0));
                    marks.push(new THREE.Vector3(Math.cos(a) * size * 0.95, Math.sin(a) * size * 0.95, 0));
                }
                const ticks = new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(marks), markMat);
                ticks.position.z = 0.5;
                group.add(ticks);

                const handMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
                });
                function hand(length, width) {
                    const pivot = new THREE.Group();
                    const bar = new THREE.Mesh(new THREE.BoxGeometry(width, length, 1), handMat);
                    bar.position.y = length / 2;      // drawn from its centre, so half of it
                    pivot.add(bar);                   // is what puts the tail on the pivot
                    pivot.position.z = 2;
                    group.add(pivot);
                    return pivot;
                }
                const hourHand = hand(size * 0.5, size * 0.06);
                const minuteHand = hand(size * 0.72, size * 0.045);

                return {
                    object: group,
                    applyPalette(pal) {
                        dialMat.color.setHex(pal.hex);
                        markMat.color.setHex(pal.hex3);
                        handMat.color.setHex(pal.hex2);
                    },
                    animate(ctx) {
                        const now = new Date();
                        const seconds = now.getSeconds() + now.getMilliseconds() / 1000;
                        const minutes = now.getMinutes() + seconds / 60;
                        const hours = (now.getHours() % 12) + minutes / 60;
                        // Clock hands run the opposite way to the maths convention.
                        minuteHand.rotation.z = -(minutes / 60) * Math.PI * 2;
                        hourHand.rotation.z = -(hours / 12) * Math.PI * 2;
                        dialMat.opacity = 0.26 + ctx.audio * 0.24 + Math.sin(ctx.time * 30) * 0.03;
                        handMat.opacity = 0.8 + ctx.audio * 0.2;
                    },
                };
            },
        },

        /* Adapted from enXephalon: the shell of thought-points with the brighter
           signatures scattered through it. The avatar wires its neighbours into a synapse
           lattice as well; a core has to stand next to parts it never met, so this is the
           cloud alone, which is the part that reads at any size. */
        mindPointShell: {
            label: 'Mind point shell',
            build(api, options) {
                const SIGNATURE = 0xff2fb0;
                const COUNT = 420;
                const positions = new Float32Array(COUNT * 3);
                const colors = new Float32Array(COUNT * 3);
                const points = [];
                for (let i = 0; i < COUNT; i++) {
                    // acos of a uniform value, not a uniform angle -- that one bunches
                    // everything at the poles.
                    const theta = Math.random() * Math.PI * 2;
                    const phi = Math.acos(2 * Math.random() - 1);
                    const r = options.size * (0.94 + Math.random() * 0.12);
                    positions[i * 3] = r * Math.sin(phi) * Math.cos(theta);
                    positions[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
                    positions[i * 3 + 2] = r * Math.cos(phi);
                    points.push({
                        signature: Math.random() > 0.86,
                        phase: Math.random() * Math.PI * 2,
                        rate: 0.5 + Math.random() * 0.9,
                    });
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                // Mapped, not bare: an unmapped point sprite is a hard square, and several
                // hundred of them read as pixel noise rather than as nodes.
                const mat = new THREE.PointsMaterial({
                    map: api.helpers.glowTexture(32), size: options.size * 0.17, vertexColors: true,
                    transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                    depthWrite: false, sizeAttenuation: true,
                });
                const cloud = new THREE.Points(geom, mat);
                const base = new THREE.Color(api.palette.hex);
                const hot = new THREE.Color(SIGNATURE);
                return {
                    object: cloud,
                    applyPalette(pal) { base.setHex(pal.hex); },
                    animate(ctx) {
                        for (let i = 0; i < points.length; i++) {
                            const pt = points[i];
                            const wave = Math.sin(ctx.time * pt.rate + pt.phase) * 0.5 + 0.5;
                            const c = pt.signature ? hot : base;
                            const b = Math.min(1.4, (pt.signature ? 0.85 + wave * 0.45 : 0.7 + wave * 0.3)
                                + ctx.audio * 0.4 + ctx.click * 0.3);
                            colors[i * 3] = c.r * b;
                            colors[i * 3 + 1] = c.g * b;
                            colors[i * 3 + 2] = c.b * b;
                        }
                        geom.attributes.color.needsUpdate = true;
                    },
                };
            },
        },

        /* Adapted from vA1or: the aiming ring with the four segmented arcs around it.
           The avatar turns the two against each other; a core may not turn itself (see the
           note at the top of this file), so here they hold their angle and the lock pulse
           does the work, leaving the recipe's spin setting to decide whether anything
           moves. Gaps in the arcs rather than a second solid ring, because the gaps are
           what make it read as a sight and not as a target painted on a wall. */
        targetReticle: {
            label: 'Target reticle',
            build(api, options) {
                const group = new THREE.Group();
                const ringMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.9,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(
                    new THREE.RingGeometry(options.size * 0.62, options.size * 0.66, 48), ringMat
                ));
                const arcMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.8,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                for (let i = 0; i < 4; i++) {
                    const step = (Math.PI * 2) / 4;
                    group.add(new THREE.Mesh(
                        new THREE.RingGeometry(options.size * 0.96, options.size, 16, 1, i * step + 0.1, step - 0.2),
                        arcMat
                    ));
                }
                const hubMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
                });
                const hub = new THREE.Mesh(new THREE.CircleGeometry(options.size * 0.1, 24), hubMat);
                hub.position.z = 0.5;
                group.add(hub);
                return {
                    object: group,
                    applyPalette(pal) {
                        ringMat.color.setHex(pal.hex);
                        arcMat.color.setHex(pal.hex2);
                        hubMat.color.setHex(pal.hex);
                    },
                    animate(ctx) {
                        const lock = 1 + Math.sin(ctx.time * 4) * 0.03 - ctx.click * 0.08;
                        group.scale.set(lock, lock, 1);
                        ringMat.opacity = 0.7 + ctx.audio * 0.3;
                        arcMat.opacity = 0.6 + ctx.audio * 0.35;
                        hubMat.opacity = 0.6 + ctx.audio * 0.4 + ctx.click * 0.2;
                    },
                };
            },
        },
        /* Adapted from CorvA1s: the plated shell that opens. Six patches of one sphere
           around a wireframe heart -- shut, the seams are the only sign there is anything
           inside; working, the plates travel out along their own axes and the heart is
           left standing in the gap. Each plate's direction is read from its own geometry
           rather than re-derived from the angles it was cut at, so a re-cut plate cannot
           fly inward. The travel is a reaction to the state, the same class of thing as a
           scale or a glow, and not a rotation -- this core still sits still under a spin
           of zero, which is what the note at the top of this file asks of it. */
        platedShell: {
            label: 'Plated shell',
            build(api, options) {
                const group = new THREE.Group();
                const PLATE = options.size * 0.86;
                const SPREAD = options.size * 0.58;
                const GAP = 0.2;

                const heartMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex2, wireframe: true,
                    transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending,
                });
                const heart = new THREE.Mesh(
                    new THREE.SphereGeometry(options.size * 0.36, 14, 10), heartMat
                );
                group.add(heart);

                const plateMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.16,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const edgeMat = new THREE.LineBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.95,
                    blending: THREE.AdditiveBlending,
                });
                const cuts = [
                    { phi: 0, phiLen: Math.PI * 2, theta: 0, thetaLen: 0.55 },
                    { phi: 0, phiLen: Math.PI * 2, theta: Math.PI - 0.55, thetaLen: 0.55 },
                ];
                for (let i = 0; i < 4; i++) {
                    const step = (Math.PI * 2) / 4;
                    cuts.push({
                        phi: i * step + GAP, phiLen: step - GAP * 2,
                        theta: 0.55 + GAP, thetaLen: Math.PI - 1.1 - GAP * 2,
                    });
                }
                const plates = cuts.map((cut) => {
                    const geom = new THREE.SphereGeometry(
                        PLATE, 14, 8, cut.phi, cut.phiLen, cut.theta, cut.thetaLen
                    );
                    geom.computeBoundingBox();
                    const dir = new THREE.Vector3();
                    geom.boundingBox.getCenter(dir);
                    if (dir.length() < 0.001) dir.set(0, 1, 0);
                    dir.normalize();
                    const mesh = new THREE.Mesh(geom, plateMat);
                    // Forty degrees, not the default one: a sphere patch at this
                    // tessellation bends about twenty degrees a quad, so the default
                    // keeps every internal edge and the plate reads as a wireframe ball
                    // instead of a panel. This leaves the border.
                    mesh.add(new THREE.LineSegments(new THREE.EdgesGeometry(geom, 40), edgeMat));
                    group.add(mesh);
                    return { mesh, dir };
                });

                let open = 0;
                return {
                    object: group,
                    applyPalette(pal) {
                        heartMat.color.setHex(pal.hex2);
                        plateMat.color.setHex(pal.hex);
                        edgeMat.color.setHex(pal.hex);
                    },
                    animate(ctx) {
                        const target = ctx.state === 'THINKING' ? 1
                            : ctx.state === 'SPEAKING' ? 0.35 + ctx.audio * 0.5
                                : ctx.state === 'LISTENING' ? 0 : ctx.click * 0.5;
                        // Eased, never set: the travel between shut and open is the part.
                        open += (target - open) * 0.07;
                        for (let i = 0; i < plates.length; i++) {
                            const lag = Math.max(0, Math.min(1, open * 1.6 - i * 0.1));
                            plates[i].mesh.position.copy(plates[i].dir).multiplyScalar(lag * SPREAD);
                        }
                        heart.scale.setScalar(1 + Math.sin(ctx.time * 3) * 0.08);
                        heartMat.opacity = 0.36 + open * 0.34 + ctx.audio * 0.2;
                        plateMat.opacity = 0.16 - open * 0.04 + ctx.audio * 0.1;
                        edgeMat.opacity = 0.95 - open * 0.05 + ctx.click * 0.12;
                    },
                };
            },
        },
        /* Adapted from Divinai: the plasma core the voice pushes about. Every vertex of a
           wireframe shell travels out along its own rest direction by how loud its band
           is, over a slow carrier that keeps it breathing in a silence -- so it swells
           rather than shears, and a quiet moment is still alive. A second, dimmer copy
           sits inside it a third of a second behind.

           The rest positions are kept because a displaced vertex must never be the input
           to the next frame's displacement: a shell fed its own output walks off the
           screen inside a few seconds. Detail 2 and no normals recomputed, because
           nothing here is lit and the shell is read as a wire drawing. It swells but does
           not turn, so the recipe's spin still owns rotation. */
        plasmaCore: {
            label: 'Plasma core',
            build(api, options) {
                const group = new THREE.Group();
                const FURNACE = 0xff3300;

                const geom = new THREE.IcosahedronGeometry(options.size, 2);
                const rest = new Float32Array(geom.attributes.position.array);
                const mat = new THREE.MeshBasicMaterial({
                    color: FURNACE, wireframe: true,
                    transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(geom, mat));

                const ghostGeom = new THREE.IcosahedronGeometry(options.size * 0.92, 2);
                const ghostRest = new Float32Array(ghostGeom.attributes.position.array);
                const ghostMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex2, wireframe: true,
                    transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.Mesh(ghostGeom, ghostMat));

                const swell = (g, r, ctx, time, scale, gain) => {
                    const attr = g.attributes.position;
                    const arr = attr.array;
                    const data = ctx.audioData;
                    const bins = data && data.length ? data.length : 0;
                    for (let i = 0; i < arr.length; i += 3) {
                        const x = r[i];
                        const y = r[i + 1];
                        const z = r[i + 2];
                        const carrier = Math.sin(time * 3 + x * 0.07 + y * 0.07) * 0.08;
                        let band = 0;
                        if (bins) {
                            // Derived from where the vertex sits, so neighbours share a
                            // band and the shell swells in patches rather than speckling.
                            const idx = Math.abs(Math.floor(x * 0.9 + y * 1.7 + z * 0.4)) % bins;
                            band = (data[idx] || 0) / 255;
                        }
                        const push = 1 + (carrier + band * 0.3 * gain) * scale;
                        arr[i] = x * push;
                        arr[i + 1] = y * push;
                        arr[i + 2] = z * push;
                    }
                    attr.needsUpdate = true;
                };

                return {
                    object: group,
                    applyPalette(pal) {
                        ghostMat.color.setHex(pal.hex2);
                        // The core keeps the furnace's own heat -- see above.
                    },
                    animate(ctx) {
                        swell(geom, rest, ctx, ctx.time, 1, 1);
                        swell(ghostGeom, ghostRest, ctx, ctx.time - 0.33, 0.55, 0.7);
                        mat.opacity = 0.55 + ctx.audio * 0.35 + ctx.click * 0.1;
                        ghostMat.opacity = 0.12 + ctx.audio * 0.08;
                    },
                };
            },
        },
        /* Adapted from TithesA1s: the stone at the brow, the one part of that avatar that is
           never absent. A blue gem in a bezel, sunk into a dark socket and lit from behind.

           The socket is the whole trick. Additively blended hot colour over the dark panel
           is bright, but over anything already bright it is white -- drop this on a busy
           core and without the socket it washes out exactly where it most needs to read.
           The socket is the floor it is seen against. It pulses but does not turn, so the
           recipe's spin still owns rotation. */
        browStone: {
            label: 'Brow stone',
            build(api, options) {
                const group = new THREE.Group();
                const STONE = 0x2b6bff;

                const glowMat = new THREE.SpriteMaterial({
                    map: api.helpers.glowTexture(64), color: STONE,
                    transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const glow = new THREE.Sprite(glowMat);
                glow.scale.set(options.size * 2.2, options.size * 2.2, 1);
                group.add(glow);

                /* Sunk into a dark socket, because the gem is additive and additive over
                   anything already bright comes out white -- the socket is the floor it
                   gets read against whatever a recipe puts behind it. */
                const socketMat = new THREE.MeshBasicMaterial({
                    color: 0x04060f, transparent: true, opacity: 0.92,
                });
                const socket = new THREE.Mesh(gemBezelGeometry(0, options.size * 0.82, 0), socketMat);
                socket.position.z = options.size * 0.2;
                group.add(socket);

                const bezelMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.85,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const bezel = new THREE.Mesh(
                    gemBezelGeometry(options.size * 0.47, options.size * 0.69, options.size * 0.12),
                    bezelMat
                );
                bezel.position.z = options.size * 0.24;
                group.add(bezel);

                // The stone itself: a cut hexagonal gem standing on a point, faceted by
                // vertex colour rather than by light -- see cutGemGeometry above.
                const gemMat = new THREE.MeshBasicMaterial({
                    vertexColors: true, transparent: true, opacity: 0.95, side: THREE.DoubleSide,
                });
                const gem = new THREE.Mesh(
                    cutGemGeometry(options.size * 0.47, options.size * 0.69, options.size * 0.28,
                        [0.26, 0.52, 1]),
                    gemMat
                );
                gem.position.z = options.size * 0.3;
                group.add(gem);

                return {
                    object: group,
                    applyPalette(pal) {
                        bezelMat.color.setHex(pal.hex);
                        // The gem and its glow keep the stone's own blue -- see above.
                    },
                    animate(ctx) {
                        const pulse = 0.8 + Math.sin(ctx.time * 5) * 0.2;
                        glowMat.opacity = (0.6 + ctx.audio * 0.4) * pulse;
                        glow.scale.setScalar(options.size * (2.1 + Math.sin(ctx.time * 5) * 0.2 + ctx.audio * 0.5));
                        gemMat.opacity = 0.85 + ctx.audio * 0.15;
                        bezelMat.opacity = 0.7 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },
    };

    // ---- Inner rings: a structure wrapping close around the core -------------

    const INNER_RINGS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        shards: {
            label: 'Shard stack',
            build(api, options) {
                const group = new THREE.Group();
                const shards = [];
                const COUNT = 9;
                for (let i = 0; i < COUNT; i++) {
                    const t = i / (COUNT - 1);
                    const w = options.radius * (0.5 + Math.sin(t * Math.PI) * 0.8);
                    const geom = new THREE.PlaneGeometry(w, options.radius * 0.16);
                    const fill = new THREE.MeshBasicMaterial({
                        color: 0x05070f, transparent: true, opacity: 0.55, side: THREE.DoubleSide,
                    });
                    const outlineMat = new THREE.LineBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.95,
                    });
                    const mesh = new THREE.Mesh(geom, fill);
                    const outline = new THREE.LineSegments(new THREE.EdgesGeometry(geom), outlineMat);
                    const shard = new THREE.Group();
                    shard.add(mesh, outline);
                    shard.position.y = (t - 0.5) * options.radius * 2.1;
                    shard.rotation.z = (Math.random() - 0.5) * 0.14;
                    group.add(shard);
                    shards.push({ shard, outlineMat, index: i, baseY: shard.position.y });
                }
                return {
                    object: group,
                    applyPalette(p) {
                        const bottom = new THREE.Color(p.hex2);
                        const top = new THREE.Color(p.hex3);
                        shards.forEach(({ outlineMat, index }) => {
                            outlineMat.color.copy(bottom).lerp(top, index / (COUNT - 1));
                        });
                    },
                    animate(ctx) {
                        shards.forEach(({ shard, outlineMat, index, baseY }) => {
                            const level = band(ctx, index, COUNT);
                            shard.scale.x = 1 + level * 1.1 + ctx.click * 0.2;
                            shard.position.y = baseY + Math.sin(ctx.time * 1.2 + index) * 1.5;
                            outlineMat.opacity = 0.5 + level * 0.5;
                        });
                        group.rotation.y = Math.sin(ctx.time * 0.25) * 0.35;
                    },
                };
            },
        },

        // Adapted from hAlcy: a segmented ring hugging the core, each segment its own
        // audio bin, so it reads as a literal ring-shaped equaliser.
        equalizerRing: {
            label: 'Equalizer ring',
            build(api, options) {
                const group = new THREE.Group();
                const segCount = 32;
                const segMat = new THREE.MeshBasicMaterial({
                    color: 0x2b3eff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const segs = [];
                const segLength = options.radius * 0.22;
                for (let i = 0; i < segCount; i++) {
                    const angle = (i / segCount) * Math.PI * 2;
                    const geom = new THREE.BoxGeometry(options.radius * 0.06, segLength, options.radius * 0.045);
                    geom.translate(0, segLength / 2, 0);
                    const seg = new THREE.Mesh(geom, segMat);
                    seg.position.set(Math.cos(angle) * options.radius, Math.sin(angle) * options.radius, 0);
                    seg.rotation.z = angle - Math.PI / 2;
                    group.add(seg);
                    segs.push({ seg, index: i });
                }
                return {
                    object: group,
                    applyPalette(p) { segMat.color.setHex(p.hex2); },
                    animate(ctx) {
                        segs.forEach(({ seg, index }) => {
                            const level = band(ctx, index, segCount);
                            seg.scale.y = 1 + level * 3 + ctx.click * 0.4;
                        });
                        group.rotation.z += 0.0025;
                    },
                };
            },
        },

        // Adapted from R.E.D. 9000: two partial-ring arcs cupping the core from above
        // and below at slightly different depths, closing together in a blink.
        eyelidArcs: {
            label: 'Eyelid arcs',
            build(api, options) {
                const r1 = options.radius, r2 = options.radius + options.radius * 0.06;
                const topMat = new THREE.MeshBasicMaterial({
                    color: 0x0066ff, side: THREE.DoubleSide, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const bottomMat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, side: THREE.DoubleSide, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const top = new THREE.Mesh(new THREE.RingGeometry(r1, r2, 48, 1, 0.35, 2.44), topMat);
                top.position.z = options.radius * 0.1;
                const bottom = new THREE.Mesh(new THREE.RingGeometry(r1, r2, 48, 1, Math.PI + 0.35, 2.44), bottomMat);
                bottom.position.z = options.radius * 0.25;
                const group = new THREE.Group();
                group.add(top, bottom);
                let nextBlink = 3 + Math.random() * 3;
                let blinkStart = -10;
                return {
                    object: group,
                    applyPalette(p) { topMat.color.setHex(p.hex); bottomMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        if (ctx.time > nextBlink) { blinkStart = ctx.time; nextBlink = ctx.time + 3 + Math.random() * 4; }
                        const dt = ctx.time - blinkStart;
                        const closeness = (dt >= 0 && dt < 0.28) ? Math.sin((dt / 0.28) * Math.PI) : 0;
                        const scaleY = 1 - closeness * 0.96;
                        top.scale.y = scaleY;
                        bottom.scale.y = scaleY;
                        const opacity = (ctx.state === 'SPEAKING' ? 0.85 + ctx.audio * 0.15 : 0.85) + closeness * 0.15;
                        topMat.opacity = opacity;
                        bottomMat.opacity = opacity;
                    },
                };
            },
        },

        // Adapted from A.R.X.LIMES: the eyelid-shaped top/bottom plates and four wing
        // blades hugging the hub, read together as a fractured shell.
        plateCluster: {
            label: 'Faceted plate cluster',
            build(api, options) {
                const group = new THREE.Group();
                const s = options.radius / 62;
                const mainFillMat = new THREE.MeshBasicMaterial({
                    color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
                });
                const wingFillMat = new THREE.MeshBasicMaterial({
                    color: 0xff5500, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
                });
                const outlineMat = new THREE.LineBasicMaterial({ color: 0xff3300, transparent: true, opacity: 0.95 });
                const topShape = [{ x: -16, y: 0 }, { x: 16, y: 0 }, { x: 8, y: 13 }, { x: -8, y: 13 }].map((p) => ({ x: p.x * s, y: p.y * s }));
                const bottomShape = [{ x: -19, y: 0 }, { x: 19, y: 0 }, { x: 10, y: 16 }, { x: -10, y: 16 }].map((p) => ({ x: p.x * s, y: p.y * s }));
                const wingRight = [{ x: -2, y: 0 }, { x: 2, y: 0 }, { x: 17, y: 26 }, { x: 4, y: 31 }].map((p) => ({ x: p.x * s, y: p.y * s }));
                const wingLeft = wingRight.map((p) => ({ x: -p.x, y: p.y }));
                const plates = [];
                const addPlate = (shape, fillMat, angleDeg, radius) => {
                    const worldAngle = angleDeg * Math.PI / 180;
                    const plate = polygonShard(shape, 10 * s, fillMat, outlineMat);
                    plate.position.set(Math.cos(worldAngle) * radius, Math.sin(worldAngle) * radius, 0);
                    plate.rotation.z = worldAngle - Math.PI / 2;
                    group.add(plate);
                    plates.push({ plate, baseAngle: worldAngle, phase: angleDeg * 0.03 });
                };
                addPlate(topShape, mainFillMat, 90, options.radius * 0.26);
                addPlate(bottomShape, mainFillMat, -90, options.radius * 0.26);
                addPlate(wingRight, wingFillMat, 15, options.radius * 0.42);
                addPlate(wingRight, wingFillMat, -15, options.radius * 0.42);
                addPlate(wingLeft, wingFillMat, 165, options.radius * 0.42);
                addPlate(wingLeft, wingFillMat, -165, options.radius * 0.42);
                return {
                    object: group,
                    applyPalette(p) { mainFillMat.color.setHex(p.hex); wingFillMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        plates.forEach(({ plate, phase }) => {
                            plate.scale.setScalar(1 + Math.sin(ctx.time * 1.2 + phase) * 0.05 + ctx.audio * 0.1 + ctx.click * 0.1);
                        });
                        group.rotation.z = Math.sin(ctx.time * 0.2) * 0.08;
                    },
                };
            },
        },

        // Adapted from A1ter_nul: the two innermost rings of its firewall field, the
        // ones that carry the avatar's own motion rather than sitting fully static.
        altInnerRings: {
            label: 'Firewall inner rings',
            build(api, options) {
                const group = new THREE.Group();
                const mats = [];
                [0.7, 0.92].forEach((frac, i) => {
                    const radius = options.radius * frac;
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0xfcee0a, transparent: true, opacity: 0.5 - i * 0.15,
                        side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                    });
                    const mesh = new THREE.Mesh(new THREE.RingGeometry(radius, radius + radius * 0.02, 96), mat);
                    group.add(mesh);
                    mats.push(mat);
                });
                return {
                    object: group,
                    applyPalette(p) { mats.forEach((m) => m.color.setHex(p.hex)); },
                    animate(ctx) {
                        const pulse = ctx.state === 'SPEAKING' ? 0.5 + ctx.audio * 0.4
                            : ctx.state === 'THINKING' ? 0.5 + Math.sin(ctx.time * 10) * 0.25 : 0.4 + ctx.click * 0.3;
                        mats.forEach((m, i) => { m.opacity = pulse * (1 - i * 0.3); });
                    },
                };
            },
        },

        // Adapted from A.R.X.LYKSAUM: the fixed, camera-facing "face" cluster -- a
        // wide collar arc below and two shorter shoulder arcs above, gapped from each
        // other -- on its own as an inner ring, without the spinning disc housing it
        // sits in front of in the original.
        brokenCollarRing: {
            label: 'Broken collar ring',
            build(api, options) {
                const group = new THREE.Group();
                const arcMat = new THREE.MeshBasicMaterial({
                    color: 0x00e8ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, side: THREE.DoubleSide,
                });
                const thickness = options.radius * 0.07;
                [{ start: 200, span: 140 }, { start: 110, span: 55 }, { start: 15, span: 55 }].forEach(({ start, span }) => {
                    const geom = new THREE.TorusGeometry(options.radius, thickness, 10, 48, span * Math.PI / 180);
                    const mesh = new THREE.Mesh(geom, arcMat);
                    mesh.rotation.z = start * Math.PI / 180;
                    group.add(mesh);
                });
                return {
                    object: group,
                    applyPalette(p) { arcMat.color.setHex(p.hex); },
                    animate(ctx) {
                        const pulse = ctx.state === 'SPEAKING' ? 0.7 + ctx.audio * 0.3
                            : ctx.state === 'THINKING' ? 0.7 + Math.abs(Math.sin(ctx.time * 6)) * 0.3 : 0.7 + ctx.click * 0.3;
                        arcMat.opacity = pulse;
                        group.scale.setScalar(1 + ctx.click * 0.1);
                    },
                };
            },
        },

        // Adapted from A.R.X.LEXICO: the plus-shaped cross of real cubes joined to the
        // centre by data-beam lines, on its own as an inner ring, dropping the marker
        // nodes, gyro rings and dust field that surround it in the original.
        cubeCrossBeams: {
            label: 'Cube cross',
            build(api, options) {
                const group = new THREE.Group();
                const cubeSize = options.radius * 0.3;
                const fillMat = new THREE.MeshBasicMaterial({ color: 0x00f0ff, side: THREE.DoubleSide, transparent: true, opacity: 0.35 });
                const wireMat = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.9 });
                const beamMat = new THREE.LineBasicMaterial({ color: 0x00f0ff, transparent: true, opacity: 0.7 });
                const geom = new THREE.BoxGeometry(cubeSize, cubeSize, cubeSize);
                const edges = new THREE.EdgesGeometry(geom);
                const axes = [
                    { x: 0, y: options.radius }, { x: 0, y: -options.radius },
                    { x: -options.radius, y: 0 }, { x: options.radius, y: 0 },
                ];
                const beamPositions = new Float32Array(axes.length * 2 * 3);
                const cubes = axes.map((pos, i) => {
                    const cube = new THREE.Mesh(geom, fillMat);
                    cube.position.set(pos.x, pos.y, 0);
                    group.add(cube);
                    const wire = new THREE.LineSegments(edges, wireMat);
                    wire.position.copy(cube.position);
                    group.add(wire);
                    const o = i * 6;
                    beamPositions[o] = 0; beamPositions[o + 1] = 0; beamPositions[o + 2] = 0;
                    beamPositions[o + 3] = pos.x; beamPositions[o + 4] = pos.y; beamPositions[o + 5] = 0;
                    return { cube, phase: i * 1.4 };
                });
                const beamGeom = new THREE.BufferGeometry();
                beamGeom.setAttribute('position', new THREE.BufferAttribute(beamPositions, 3));
                group.add(new THREE.LineSegments(beamGeom, beamMat));
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); wireMat.color.setHex(p.hex2); beamMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.15;
                        cubes.forEach(({ cube, phase }) => {
                            const s = 1 + Math.sin(ctx.time * 2 + phase) * 0.08 + ctx.click * 0.15;
                            cube.scale.setScalar(s);
                        });
                        beamMat.opacity = 0.5 + ctx.audio * 0.3 + ctx.click * 0.15;
                    },
                };
            },
        },

        // Adapted from A.R.X.LUCRE: the column of cubes stacked along the vertical axis,
        // each turned 45 degrees so it meets the viewer corner-on, with the dashed data
        // beam running the full height of the stack through them.
        stackedCubes: {
            label: 'Stacked cubes',
            build(api, options) {
                const group = new THREE.Group();
                const unit = options.radius * 0.36;
                const fillMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, side: THREE.DoubleSide,
                    transparent: true, opacity: 0.22, depthWrite: false,
                });
                const wireMatA = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });
                const wireMatB = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.85 });
                const geom = new THREE.BoxGeometry(unit, unit, unit);
                const edges = new THREE.EdgesGeometry(geom);
                const layout = [
                    { y: options.radius * 1.36, scale: 0.65 },
                    { y: options.radius * 0.75, scale: 0.85 },
                    { y: -options.radius * 0.75, scale: 0.85 },
                    { y: -options.radius * 1.36, scale: 0.65 },
                ];
                const cubes = layout.map((cfg, i) => {
                    const cube = new THREE.Mesh(geom, fillMat);
                    cube.scale.setScalar(cfg.scale);
                    cube.position.set(0, cfg.y, 0);
                    cube.rotation.y = Math.PI / 4;
                    const wire = new THREE.LineSegments(edges, i % 2 === 0 ? wireMatA : wireMatB);
                    wire.scale.copy(cube.scale);
                    wire.position.copy(cube.position);
                    wire.rotation.copy(cube.rotation);
                    group.add(cube, wire);
                    return { cube, wire, baseY: cfg.y, phase: (i + 1) * 0.5 };
                });
                const beamMat = new THREE.LineDashedMaterial({
                    color: api.palette.hex3, transparent: true, opacity: 0.75, dashSize: 3, gapSize: 1.8,
                });
                const reach = options.radius * 1.55;
                const beam = new THREE.Line(
                    new THREE.BufferGeometry().setFromPoints([
                        new THREE.Vector3(0, -reach, 0), new THREE.Vector3(0, reach, 0),
                    ]),
                    beamMat
                );
                beam.computeLineDistances();
                group.add(beam);
                return {
                    object: group,
                    applyPalette(p) {
                        fillMat.color.setHex(p.hex);
                        wireMatA.color.setHex(p.hex2);
                        wireMatB.color.setHex(p.hex3);
                        beamMat.color.setHex(p.hex3);
                    },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.2;
                        cubes.forEach(({ cube, wire, baseY, phase }) => {
                            const drift = Math.sin(ctx.time * 1.4 + phase) * options.radius * 0.03;
                            cube.position.y = baseY + drift;
                            wire.position.y = cube.position.y;
                        });
                        beamMat.opacity = 0.55 + ctx.audio * 0.35 + ctx.click * 0.2;
                    },
                };
            },
        },

        /* Adapted from C.I.C.E.R.O.: the two chrome tape reels that turn where its eyes
           would be. Spokes are what make a turning disc read as turning -- a plain disc
           spinning about its own axis is indistinguishable from a still one -- and the
           reels answer the voice, which is what they do on the avatar too. */
        tapeReels: {
            label: 'Tape reels',
            build(api, options) {
                const group = new THREE.Group();
                const faceMat = new THREE.MeshPhongMaterial({ color: 0x8d9ab0, shininess: 110, specular: 0xdfe8f5 });
                const spokeMat = new THREE.MeshBasicMaterial({ color: 0x1b2330 });
                const hubMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending,
                });
                // Sized well under the ring radius: these flank whatever core is in the
                // middle, and a reel as big as the core reads as two more cores.
                const r = options.radius * 0.17;
                const reels = [];
                [-1, 1].forEach((side) => {
                    const reel = new THREE.Group();
                    reel.position.set(side * options.radius * 0.62, 0, options.radius * 0.3);
                    const disc = new THREE.Mesh(new THREE.CylinderGeometry(r, r, r * 0.3, 20), faceMat);
                    disc.rotation.x = Math.PI / 2;
                    reel.add(disc);
                    for (let i = 0; i < 3; i++) {
                        // Each spoke in its own arm, offset along the arm's own y and the
                        // arm then turned: rotating the mesh in place and solving for
                        // where it should sit is the same picture by a longer road.
                        const arm = new THREE.Group();
                        arm.rotation.z = (i / 3) * Math.PI * 2;
                        const spoke = new THREE.Mesh(new THREE.BoxGeometry(r * 0.2, r * 0.85, r * 0.09), spokeMat);
                        spoke.position.set(0, r * 0.42, r * 0.19);
                        arm.add(spoke);
                        reel.add(arm);
                    }
                    const hub = new THREE.Mesh(new THREE.SphereGeometry(r * 0.42, 16, 12), hubMat);
                    hub.position.z = r * 0.3;
                    reel.add(hub);
                    group.add(reel);
                    reels.push({ reel, direction: side });
                });
                let spin = 0;
                return {
                    object: group,
                    applyPalette(pal) { hubMat.color.setHex(pal.hex); },
                    animate(ctx) {
                        // Accumulated rather than set from ctx.time, so a change of speed
                        // never snaps the reels to a new angle mid-turn.
                        spin += (1.1 + ctx.audio * 9) * 0.016 + ctx.click * 0.25;
                        reels.forEach(({ reel, direction }) => { reel.rotation.z = spin * direction; });
                        hubMat.opacity = 0.75 + ctx.audio * 0.25;
                    },
                };
            },
        },

        /* Adapted from vA1or: the readout graticule wrapping close around whatever it
           is measuring -- short ticks all the way round with a longer one at each quarter,
           and a cardinal pip standing off the top. It is the scale an instrument is read
           against, so it sits still and the pip does the moving; the quarters are what let
           an eye find the same place on it twice. */
        graticuleTicks: {
            label: 'Readout graticule',
            build(api, options) {
                const group = new THREE.Group();
                const COUNT = 36;
                const mat = new THREE.LineBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.8,
                });
                const pts = [];
                for (let i = 0; i < COUNT; i++) {
                    const a = (i / COUNT) * Math.PI * 2;
                    const long = i % 9 === 0;
                    const inner = options.radius * (long ? 0.8 : 0.88);
                    pts.push(new THREE.Vector3(Math.cos(a) * inner, Math.sin(a) * inner, 0));
                    pts.push(new THREE.Vector3(Math.cos(a) * options.radius, Math.sin(a) * options.radius, 0));
                }
                group.add(new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(pts), mat));

                const pipMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                const pip = new THREE.Mesh(
                    new THREE.RingGeometry(options.radius * 1.02, options.radius * 1.09, 6, 1, Math.PI / 2 - 0.022, 0.044),
                    pipMat
                );
                group.add(pip);

                return {
                    object: group,
                    applyPalette(pal) { mat.color.setHex(pal.hex2); pipMat.color.setHex(pal.hex); },
                    animate(ctx) {
                        // The pip runs the scale while there is something to read, and parks
                        // at the top when there is not -- an instrument at rest reads zero.
                        const live = ctx.state === 'THINKING' || ctx.state === 'SPEAKING';
                        pip.rotation.z = live ? -ctx.time * 1.6 : 0;
                        mat.opacity = 0.65 + ctx.audio * 0.3;
                        pipMat.opacity = 0.7 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },
        /* Adapted from CorvA1s: the dimension ring an opened assembly is measured
           against -- a witness line with an extension tick at each quarter and an arrow
           at each end of the span, the way a drawing calls out a distance. The span
           stretches while there is something being worked on and closes again after, so
           the ring reads as a measurement being taken rather than a decoration. */
        dimensionRing: {
            label: 'Dimension ring',
            build(api, options) {
                const group = new THREE.Group();
                const lineMat = new THREE.LineBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.75,
                });
                const pts = [];
                const R = options.radius;
                const SEG = 64;
                for (let i = 0; i < SEG; i++) {
                    const a = (i / SEG) * Math.PI * 2;
                    const b = ((i + 0.62) / SEG) * Math.PI * 2;   // dashed, so it reads as a witness line
                    pts.push(new THREE.Vector3(Math.cos(a) * R, Math.sin(a) * R, 0));
                    pts.push(new THREE.Vector3(Math.cos(b) * R, Math.sin(b) * R, 0));
                }
                for (let i = 0; i < 4; i++) {
                    const a = (i / 4) * Math.PI * 2;
                    pts.push(new THREE.Vector3(Math.cos(a) * R * 0.86, Math.sin(a) * R * 0.86, 0));
                    pts.push(new THREE.Vector3(Math.cos(a) * R * 1.12, Math.sin(a) * R * 1.12, 0));
                }
                group.add(new THREE.LineSegments(new THREE.BufferGeometry().setFromPoints(pts), lineMat));

                /* The span: two arrowheads facing each other across the top of the ring,
                   held in their own groups so only their angle has to change. */
                const spanMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.9,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const heads = [0, 1].map((side) => {
                    const pivot = new THREE.Group();
                    const head = new THREE.Mesh(new THREE.ConeGeometry(R * 0.04, R * 0.11, 3), spanMat);
                    head.position.y = R;
                    head.rotation.z = side ? Math.PI : 0;       // pointing at each other
                    pivot.add(head);
                    group.add(pivot);
                    return pivot;
                });

                return {
                    object: group,
                    applyPalette(pal) { lineMat.color.setHex(pal.hex2); spanMat.color.setHex(pal.hex); },
                    animate(ctx) {
                        const live = ctx.state === 'THINKING' || ctx.state === 'SPEAKING';
                        const span = (live ? 0.5 : 0.12) + ctx.audio * 0.25;
                        heads[0].rotation.z = span;
                        heads[1].rotation.z = -span;
                        lineMat.opacity = 0.6 + ctx.audio * 0.3;
                        spanMat.opacity = 0.7 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },
        /* Adapted from Divinai: five closed ribbons on their own tilted planes, each one's
           radius pushed out by the stretch of spectrum it answers to.

           Two things keep it a ribbon rather than a scribble. The wobble off the plane is
           built from whole multiples of the angle, so the loop arrives back where it
           started instead of leaving a step; and the loudness is read at six points around
           the loop and blended between them, because neighbouring bins of a spectrum
           differ freely and a radius taken straight from them is a sawtooth. */
        voiceRibbons: {
            label: 'Voice ribbons',
            build(api, options) {
                const group = new THREE.Group();
                const COUNT = 5;
                const POINTS = 72;
                const LOBES = 6;
                const matA = new THREE.LineBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.7, blending: THREE.AdditiveBlending,
                });
                const matB = new THREE.LineBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
                });
                const ribbons = [];
                for (let i = 0; i < COUNT; i++) {
                    const radius = options.radius * (0.62 + i * 0.09);
                    const wobble = new Float32Array(POINTS);
                    const pos = new Float32Array(POINTS * 3);
                    for (let j = 0; j < POINTS; j++) {
                        const a = (j / POINTS) * Math.PI * 2;
                        wobble[j] = (Math.sin(a * 2 + i * 2.3) * 7 + Math.sin(a * 3 + i) * 4)
                            * (options.radius / 64);
                    }
                    const geom = new THREE.BufferGeometry();
                    geom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
                    const line = new THREE.LineLoop(geom, i % 2 === 0 ? matA : matB);
                    line.rotation.x = i * 0.7;
                    line.rotation.y = i * 1.1;
                    group.add(line);
                    ribbons.push({ line, geom, pos, wobble, radius });
                }

                const shape = (ctx, time, gain) => {
                    const data = ctx && ctx.audioData;
                    const bins = data && data.length ? data.length : 0;
                    for (let r = 0; r < ribbons.length; r++) {
                        const rib = ribbons[r];
                        for (let j = 0; j < POINTS; j++) {
                            const a = (j / POINTS) * Math.PI * 2;
                            let band = 0;
                            if (bins) {
                                const at = (j / POINTS) * LOBES;
                                const lo = Math.floor(at);
                                const mix = at - lo;
                                const binOf = (k) =>
                                    (data[Math.floor(((k % LOBES) / LOBES) * bins)] || 0) / 255;
                                band = binOf(lo) * (1 - mix) + binOf(lo + 1) * mix;
                            }
                            const breath = Math.sin(time * 2 + a * 3 + r) * 0.02;
                            const radius = rib.radius * (1 + breath + band * 0.22 * gain);
                            rib.pos[j * 3] = Math.cos(a) * radius;
                            rib.pos[j * 3 + 1] = Math.sin(a) * radius;
                            rib.pos[j * 3 + 2] = rib.wobble[j];
                        }
                        rib.geom.attributes.position.needsUpdate = true;
                    }
                };
                shape(null, 0, 0);

                return {
                    object: group,
                    applyPalette(pal) { matA.color.setHex(pal.hex); matB.color.setHex(pal.hex2); },
                    animate(ctx) {
                        shape(ctx, ctx.time, 1);
                        for (let i = 0; i < ribbons.length; i++) {
                            // A function of elapsed time, not advanced per frame: otherwise
                            // how fast it turns depends on how fast the machine draws.
                            ribbons[i].line.rotation.x = i * 0.7 + ctx.time * 0.16 * (i + 1);
                            ribbons[i].line.rotation.y = i * 1.1 + ctx.time * 0.22 * (i + 1);
                        }
                        matA.opacity = 0.6 + ctx.audio * 0.3;
                        matB.opacity = 0.5 + ctx.audio * 0.28;
                    },
                };
            },
        },
        /* Adapted from TithesA1s: a shell cut into forty horizontal slices, of which only a
           travelling band is ever present. The rest are not dimmed, they are gone, and the
           dim framework saying where they would have been is what keeps the silhouette
           readable while most of it is absent. The band never stops, so something is always
           materialising and something else dematerialising; the rim it is travelling
           towards glows red, because that is the edge where matter is arriving.

           One geometry for all forty slices, with only their colours written per frame --
           the points never move, so a frame costs a colour write and nothing else. */
        phaseSlices: {
            label: 'Phase slices',
            build(api, options) {
                const group = new THREE.Group();
                const SLICES = 40;
                const SEG = 32;
                // Inside the outer ring's own radius, so a recipe that pairs this with a
                // boundary part has the slices wrapping the core rather than sharing a
                // surface with it.
                const R = options.radius * 0.72;
                const FRAME = 0xffcc00;
                const FLUX = 0xff2244;

                const pos = [];
                const heights = [];
                for (let i = 0; i < SLICES; i++) {
                    // The poles are left off: a slice of radius zero is a point.
                    const t = (i + 0.5) / SLICES;
                    const y = (t * 2 - 1) * R * 0.97;
                    const radius = Math.sqrt(Math.max(0, R * R - y * y));
                    heights.push({ y, radius });
                    for (let j = 0; j < SEG; j++) {
                        const a = (j / SEG) * Math.PI * 2;
                        const b = ((j + 1) / SEG) * Math.PI * 2;
                        pos.push(Math.cos(a) * radius, y, Math.sin(a) * radius);
                        pos.push(Math.cos(b) * radius, y, Math.sin(b) * radius);
                    }
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
                geom.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(pos.length), 3));
                const mat = new THREE.LineBasicMaterial({
                    vertexColors: true, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.LineSegments(geom, mat));

                const frame = new THREE.Color(FRAME);
                const flux = new THREE.Color(FLUX);
                const perSlice = SEG * 2 * 3;
                const EDGE = 0.26;
                let at = 0.5;
                let dir = 1;

                return {
                    object: group,
                    applyPalette() {
                        // The framework keeps its own yellow and the arriving rim its red.
                    },
                    animate(ctx) {
                        const live = ctx.state === 'THINKING' || ctx.state === 'SPEAKING';
                        const half = (ctx.state === 'SPEAKING' ? 0.42 + ctx.audio * 0.3
                            : ctx.state === 'THINKING' ? 0.09
                                : ctx.state === 'LISTENING' ? 0.16 : 0.17) + ctx.click * 0.1;
                        const speed = ctx.state === 'LISTENING' ? 0 : live ? 0.5 : 0.26;
                        at += dir * speed * 0.016;
                        // Turned at the ends, not wrapped: a band that wraps vanishes from
                        // the top and reappears at the bottom, which reads as a glitch.
                        if (at > 1) { at = 1; dir = -1; }
                        if (at < 0) { at = 0; dir = 1; }

                        const colours = geom.attributes.color.array;
                        for (let i = 0; i < SLICES; i++) {
                            const t = (i + 0.5) / SLICES;
                            const d = Math.abs(t - at) / half;
                            const presence = d >= 1 ? 0 : d <= 1 - EDGE ? 1 : (1 - d) / EDGE;
                            const leading = dir > 0 ? t > at : t < at;
                            const arriving = leading && presence > 0 && presence < 1 ? 1 - presence : 0;
                            const level = 0.26 + presence * 0.74 + ctx.audio * 0.2;
                            const r = frame.r * level + flux.r * arriving;
                            const g = frame.g * level + flux.g * arriving;
                            const b = frame.b * level + flux.b * arriving;
                            const from = i * perSlice;
                            for (let k = 0; k < perSlice; k += 3) {
                                colours[from + k] = r;
                                colours[from + k + 1] = g;
                                colours[from + k + 2] = b;
                            }
                        }
                        geom.attributes.color.needsUpdate = true;
                        mat.opacity = 0.75 + ctx.audio * 0.25;
                    },
                };
            },
        },
    };

    // ---- Outer rings: a boundary further out ---------------------------------

    // ---- Aurora colourways ---------------------------------------------------
    /* Two named colourways, shared by the curtain (tier 3) and the veil (tier 4) so a
       Borealis curtain and a Borealis veil are the same light at two distances rather
       than two greens that nearly match. Bottom of the curtain to tip.

       Borealis is the northern reading: a dark base, the green that does most of the
       work in a real aurora, teal through the middle, and the rare violet crown at the
       tips. Australis is its opposite rather than a recolour -- the accent and the
       active colour trade ends, so the warm rose Borealis never reaches carries the body
       and the cold aqua arrives at the tips, and `hang` flips so the curtain descends
       from above instead of rising from below.

       These are the one kind of part here that does not follow the colour theme. A named
       colourway whose colours moved with the theme would not be that colourway any more
       -- the same reasoning the README's "hot accents stay fixed" convention already
       applies to R.E.D. 9000's lens and the Nexus's eyes -- so their applyPalette is
       deliberately a no-op. Anyone wanting theme-tinted layers has every other part in
       these two tiers to choose from. */
    const AURORA = {
        borealis: { stops: [0x04221a, 0x16d978, 0x2fe3ff, 0x8f6bff], hang: 1 },
        australis: { stops: [0x260a33, 0xff3d7f, 0xc86bff, 0x58ffe0], hang: -1 },
    };

    // Samples a colourway's four stops as one continuous gradient, 0 at the curtain's
    // base to 1 at its tip, times `brightness` -- which is how every fade here is done.
    // These layers blend additively, so fading a colour toward black *is* fading it
    // toward invisible; there is no per-vertex alpha to reach for.
    const auroraScratch = new THREE.Color();
    function auroraGradient(stops, t, brightness) {
        const span = stops.length - 1;
        const scaled = Math.min(span - 0.0001, Math.max(0, t) * span);
        const index = Math.floor(scaled);
        const colour = new THREE.Color(stops[index]);
        colour.lerp(auroraScratch.setHex(stops[index + 1]), scaled - index);
        return colour.multiplyScalar(brightness);
    }

    /* The curtain: six ribbons of light standing on a circle at the edge of the shape,
       each a column of soft overlapping sprites running up the colourway's gradient.
       Sprites rather than a quad per ribbon, which is what this started as and is worth
       not going back to: the renderer's canvas has an alpha channel (see core.js), and an
       additively blended quad writes its material's alpha over its whole rectangle
       whatever its vertex colours say -- so a ribbon faded to black at its edges still
       composited as a visible grey slab with hard corners against the page behind the
       canvas. A sprite's alpha comes from its own soft texture, so a ribbon has no edge
       anywhere. */
    function buildAuroraCurtain(api, options, way) {
        const BANDS = 6;
        const BEADS = 14;
        const radius = options.radius * 1.35;
        const span = options.radius * 2.1;

        const group = new THREE.Group();
        const ribbons = [];
        const texture = api.helpers.glowTexture(64);

        for (let b = 0; b < BANDS; b++) {
            const centreAngle = (b / BANDS) * Math.PI * 2 + (b % 2) * 0.12;
            // Three heights and three leans, so six ribbons at one size do not read as a fence.
            const height = span * (0.78 + (b % 4) * 0.12);
            const lean = ((b % 3) - 1) * 0.1;
            const beads = [];
            for (let i = 0; i < BEADS; i++) {
                const t = i / (BEADS - 1);
                // Out at the tip, in at the foot, and gone at both -- a ribbon that simply
                // stops at its lowest bead draws a hard line under the whole curtain.
                const brightness = (1 - Math.pow(t, 3.4)) * Math.min(1, t / 0.18);
                const mat = new THREE.SpriteMaterial({
                    map: texture,
                    color: auroraGradient(way.stops, t, 1).getHex(),
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

        return {
            object: group,
            applyPalette() {},
            animate(ctx) {
                const sway = 0.7 + ctx.audio * 1.6 + ctx.click * 0.6;
                // Read straight off the world matrix rather than through
                // getWorldQuaternion: the vendored three bundle carries only the symbols
                // the frontend already names (see scripts/build_vendor_three.sh), and a
                // Quaternion is not one of them. It also picks up the recipe's own spin,
                // which a part is never told about directly.
                const world = group.matrixWorld.elements;
                ribbons.forEach((ribbon) => {
                    // A ribbon swung round to the front of the circle stands between the
                    // viewer and the core, so it is dimmed there; behind the core it burns
                    // at full strength.
                    const toward = world[2] * Math.cos(ribbon.centreAngle) +
                        world[10] * Math.sin(ribbon.centreAngle);
                    const depthFade = 1 - 0.68 * Math.max(0, toward);
                    ribbon.beads.forEach((bead) => {
                        const ripple = Math.sin(ctx.time * 0.5 + ribbon.phase + bead.t * 3.4);
                        const angle = ribbon.centreAngle + (ripple * 0.06 + ribbon.lean) * sway * bead.t;
                        const r = radius + ripple * span * 0.055 * sway * bead.t;
                        bead.sprite.position.set(
                            Math.cos(angle) * r,
                            (-ribbon.height * 0.42 + bead.t * ribbon.height + ripple * 3 * sway) * way.hang,
                            Math.sin(angle) * r
                        );
                        bead.mat.opacity = bead.brightness * depthFade *
                            (0.42 + Math.sin(ctx.time * 0.8 + ribbon.phase) * 0.06 + ctx.audio * 0.22);
                    });
                });
            },
        };
    }

    /* The veil: three wide soft washes behind everything and a thin scatter of points in
       front of them. An ambient layer rather than a structure, which is the distinction
       between the effects tier and the outer-ring tier above it. It only breathes and
       drifts sideways; anything sharper read as a second avatar competing with the core. */
    function buildAuroraVeil(api, options, way) {
        const group = new THREE.Group();
        const washes = [];
        const unit = options.radius;
        [
            { colour: way.stops[1], scale: [unit * 4.8, unit * 1.9], y: unit * 0.42 * way.hang, opacity: 0.20, speed: 0.11 },
            { colour: way.stops[2], scale: [unit * 3.9, unit * 1.4], y: -unit * 0.06 * way.hang, opacity: 0.16, speed: -0.08 },
            { colour: way.stops[3], scale: [unit * 2.9, unit * 0.9], y: unit * 0.71 * way.hang, opacity: 0.12, speed: 0.15 },
        ].forEach((band, i) => {
            const mat = new THREE.SpriteMaterial({
                map: api.helpers.glowTexture(64), color: band.colour, transparent: true,
                opacity: band.opacity, blending: THREE.AdditiveBlending, depthWrite: false,
            });
            const sprite = new THREE.Sprite(mat);
            sprite.scale.set(band.scale[0], band.scale[1], 1);
            sprite.position.set(0, band.y, -unit * 1.4 + i * 6);
            group.add(sprite);
            washes.push({ sprite, mat, base: band, phase: i * 2.1 });
        });

        const DUST = 160;
        const positions = new Float32Array(DUST * 3);
        const colors = new Float32Array(DUST * 3);
        for (let i = 0; i < DUST; i++) {
            positions[i * 3] = (Math.random() - 0.5) * unit * 4.8;
            positions[i * 3 + 1] = (Math.random() - 0.5) * unit * 3.0;
            positions[i * 3 + 2] = -unit * 1.1 + Math.random() * unit * 0.8;
            const colour = auroraGradient(way.stops, 0.35 + Math.random() * 0.65, 0.6 + Math.random() * 0.4);
            colors[i * 3] = colour.r;
            colors[i * 3 + 1] = colour.g;
            colors[i * 3 + 2] = colour.b;
        }
        const geom = new THREE.BufferGeometry();
        geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
        geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
        const dustMat = new THREE.PointsMaterial({
            size: 1.5, vertexColors: true, map: api.helpers.glowTexture(32),
            transparent: true, opacity: 0.75, blending: THREE.AdditiveBlending, depthWrite: false,
        });
        const dust = new THREE.Points(geom, dustMat);
        group.add(dust);

        return {
            object: group,
            applyPalette() {},
            animate(ctx) {
                washes.forEach(({ sprite, mat, base, phase }) => {
                    sprite.position.x = Math.sin(ctx.time * base.speed + phase) * unit * 0.42;
                    mat.opacity = base.opacity + Math.sin(ctx.time * 0.4 + phase) * 0.04 + ctx.audio * 0.12;
                });
                dust.rotation.z = ctx.time * 0.01;
                dustMat.opacity = 0.6 + Math.sin(ctx.time * 0.7) * 0.15 + ctx.click * 0.2;
            },
        };
    }

    const OUTER_RINGS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        auroraBorealis: {
            label: 'Aurora curtain (Borealis)',
            build(api, options) { return buildAuroraCurtain(api, options, AURORA.borealis); },
        },

        auroraAustralis: {
            label: 'Aurora curtain (Australis)',
            build(api, options) { return buildAuroraCurtain(api, options, AURORA.australis); },
        },

        rings: {
            label: 'Orbital rings',
            build(api, options) {
                const group = new THREE.Group();
                const mats = [];
                const rings = [];
                for (let i = 0; i < 3; i++) {
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.75,
                        side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                    });
                    const r = options.radius * (1 + i * 0.22);
                    const ring = new THREE.Mesh(new THREE.TorusGeometry(r, 0.9, 8, 96), mat);
                    ring.rotation.x = Math.PI / 2 * (i === 0 ? 1 : 0.4 * i);
                    ring.rotation.y = i * 0.5;
                    mats.push(mat);
                    rings.push({ ring, speed: 0.2 + i * 0.15, tilt: i });
                    group.add(ring);
                }
                return {
                    object: group,
                    applyPalette(p) {
                        mats[0].color.setHex(p.hex);
                        mats[1].color.setHex(p.hex2);
                        mats[2].color.setHex(p.hex3);
                    },
                    animate(ctx) {
                        rings.forEach(({ ring, speed, tilt }) => {
                            ring.rotation.z = ctx.time * speed;
                            ring.rotation.y = Math.sin(ctx.time * 0.3 + tilt) * 0.6 + tilt * 0.5;
                            ring.scale.setScalar(1 + ctx.audio * 0.18 + ctx.click * 0.12);
                        });
                    },
                };
            },
        },

        spikes: {
            label: 'Radial spikes',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.LineBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                const SPIKES = 64;
                const geom = new THREE.BufferGeometry();
                const positions = new Float32Array(SPIKES * 2 * 3);
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                const lines = new THREE.LineSegments(geom, mat);
                group.add(lines);
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex3); },
                    animate(ctx) {
                        for (let i = 0; i < SPIKES; i++) {
                            const a = (i / SPIKES) * Math.PI * 2 + ctx.time * 0.15;
                            const level = band(ctx, i % 32, 32);
                            const inner = options.radius;
                            const outer = inner + 6 + level * options.radius * 0.9 + ctx.click * 8;
                            positions.set([Math.cos(a) * inner, Math.sin(a) * inner, 0], i * 6);
                            positions.set([Math.cos(a) * outer, Math.sin(a) * outer, 0], i * 6 + 3);
                        }
                        geom.attributes.position.needsUpdate = true;
                    },
                };
            },
        },

        // Adapted from hAlcy: an outer ring with one broad swell baked into its band,
        // which rides around the circumference as it spins -- reads as motion rather
        // than a static disc.
        bulgingRing: {
            label: 'Bulging ring',
            build(api, options) {
                const baseRadius = options.radius * 1.25;
                const thickness = options.radius * 0.045;
                const geom = new THREE.RingGeometry(baseRadius - thickness / 2, baseRadius + thickness / 2, 128);
                const bulgeHalfAngle = Math.PI / 2.6;
                const bulgeMax = options.radius * 0.06;
                const pos = geom.attributes.position;
                for (let vi = 0; vi < pos.count; vi++) {
                    const vx = pos.getX(vi), vy = pos.getY(vi);
                    const angle = Math.atan2(vy, vx);
                    const baseR = Math.sqrt(vx * vx + vy * vy);
                    let bulge = 0;
                    if (baseR > baseRadius) {
                        const diff = Math.atan2(Math.sin(angle), Math.cos(angle));
                        if (Math.abs(diff) < bulgeHalfAngle) bulge = bulgeMax * 0.5 * (1 + Math.cos((diff / bulgeHalfAngle) * Math.PI));
                    }
                    const r = baseR + bulge;
                    pos.setXY(vi, Math.cos(angle) * r, Math.sin(angle) * r);
                }
                pos.needsUpdate = true;
                const mat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, side: THREE.DoubleSide, transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending,
                });
                const mesh = new THREE.Mesh(geom, mat);
                return {
                    object: mesh,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        mesh.rotation.z = ctx.time * 0.25;
                        mat.opacity = 0.4 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },

        // Adapted from A1ter_nul: many concentric true circles, each fainter than the
        // one inside it, static -- the field never rotates, only its brightness answers
        // the voice.
        altFirewallField: {
            label: 'Concentric firewall field',
            build(api, options) {
                const group = new THREE.Group();
                const count = 16;
                const inner = options.radius * 1.05;
                const outer = options.radius * 2.6;
                const rings = [];
                for (let i = 0; i < count; i++) {
                    const t = i / (count - 1);
                    const radius = inner + (outer - inner) * t;
                    const fade = Math.pow(1 - t, 1.6);
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0xfcee0a, transparent: true, opacity: 0.5, side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                    });
                    const mesh = new THREE.Mesh(new THREE.RingGeometry(radius, radius + radius * 0.012, 96), mat);
                    group.add(mesh);
                    rings.push({ mat, fade });
                }
                return {
                    object: group,
                    applyPalette(p) { rings.forEach(({ mat }) => mat.color.setHex(p.hex)); },
                    animate(ctx) {
                        const base = ctx.state === 'SPEAKING' ? 0.5 + ctx.audio * 0.4
                            : ctx.state === 'THINKING' ? 0.5 + Math.sin(ctx.time * 10) * 0.25 : 0.4 + ctx.click * 0.3;
                        rings.forEach(({ mat, fade }) => { mat.opacity = base * fade; });
                    },
                };
            },
        },

        // Adapted from A.R.X.LOGOS: two hex-outline shells at different radii, each
        // tilted onto its own axis and tumbling at its own speed.
        hexShells: {
            label: 'Hex shells',
            build(api, options) {
                const group = new THREE.Group();
                const configs = [
                    { radius: options.radius * 1.1, tiltX: 0.5, tiltY: 0.15, color: 0x9d00ff, speed: { x: 0.006, y: 0.01 } },
                    { radius: options.radius * 1.35, tiltX: -0.35, tiltY: 0.4, color: 0xff00ff, speed: { x: -0.004, y: 0.008 } },
                ];
                const mats = [];
                const shells = [];
                configs.forEach((cfg) => {
                    const mat = new THREE.LineBasicMaterial({ color: cfg.color, transparent: true, opacity: 0.5 });
                    const ring = hexOutlineLoop(api, cfg.radius, 0, mat);
                    ring.rotation.x = cfg.tiltX;
                    ring.rotation.y = cfg.tiltY;
                    group.add(ring);
                    mats.push(mat);
                    shells.push({ ring, speed: cfg.speed });
                });
                return {
                    object: group,
                    applyPalette(p) { mats[0].color.setHex(p.hex2); mats[1].color.setHex(p.hex3); },
                    animate() {
                        shells.forEach(({ ring, speed }) => {
                            ring.rotation.x += speed.x;
                            ring.rotation.y += speed.y;
                        });
                    },
                };
            },
        },

        // Adapted from A.R.X.LOCAS: the fractured cube shell that surrounds the core
        // with a clear gap, simplified from 24 shard panels (six faces x four
        // quadrants) down to one shard per cube face, each drifting along its own
        // face normal as the whole shell tumbles.
        fracturedShardShell: {
            label: 'Fractured shard shell',
            build(api, options) {
                const group = new THREE.Group();
                const half = options.radius;
                const size = half * 0.9;
                const fillMat = new THREE.MeshBasicMaterial({
                    color: 0xe024c3, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending,
                });
                const edgeMat = new THREE.LineBasicMaterial({ color: 0xe024c3, transparent: true, opacity: 0.85 });
                const normals = [
                    new THREE.Vector3(1, 0, 0), new THREE.Vector3(-1, 0, 0),
                    new THREE.Vector3(0, 1, 0), new THREE.Vector3(0, -1, 0),
                    new THREE.Vector3(0, 0, 1), new THREE.Vector3(0, 0, -1),
                ];
                const geom = new THREE.PlaneGeometry(size, size);
                const edges = new THREE.EdgesGeometry(geom);
                const shards = normals.map((n, i) => {
                    const shard = new THREE.Mesh(geom, fillMat);
                    shard.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), n);
                    const basePos = n.clone().multiplyScalar(half);
                    shard.position.copy(basePos);
                    shard.add(new THREE.LineSegments(edges, edgeMat));
                    group.add(shard);
                    return { shard, normal: n, basePos, phase: i * 1.1 };
                });
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); edgeMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.12;
                        group.rotation.x = Math.sin(ctx.time * 0.18) * 0.12;
                        shards.forEach(({ shard, normal, basePos, phase }) => {
                            const drift = Math.sin(ctx.time * 0.6 + phase) * (half * 0.08) + ctx.click * (half * 0.15);
                            shard.position.copy(basePos).addScaledVector(normal, drift);
                        });
                    },
                };
            },
        },

        // Adapted from A.R.X.LEGIONARE: the three tiered frustum shells sliced from
        // one cone's silhouette with a clear horizontal air gap between each, hung
        // apex-down, simplified from four gapped wall panels per tier to one open
        // four-sided band per tier.
        slicedPyramidShell: {
            label: 'Sliced pyramid shell',
            build(api, options) {
                const group = new THREE.Group();
                const height = options.radius * 2;
                const baseRadius = options.radius;
                const tierCount = 3;
                const slotHeight = height / tierCount;
                const tierHeight = slotHeight * 0.7;
                const radiusAt = (y) => baseRadius * (height / 2 - y) / height;
                const fillMat = new THREE.MeshBasicMaterial({ color: 0xff6a00, side: THREE.DoubleSide, transparent: true, opacity: 0.3 });
                const wireMat = new THREE.LineBasicMaterial({ color: 0xff9955, transparent: true, opacity: 0.85 });
                const invertGroup = new THREE.Group();
                invertGroup.rotation.z = Math.PI;
                group.add(invertGroup);
                for (let i = 0; i < tierCount; i++) {
                    const center = -height / 2 + (i + 0.5) * slotHeight;
                    const topY = center + tierHeight / 2;
                    const bottomY = center - tierHeight / 2;
                    const isTip = i === tierCount - 1;
                    const rTop = isTip ? 0.001 : radiusAt(topY);
                    const rBottom = radiusAt(bottomY);
                    const geom = new THREE.CylinderGeometry(rTop, rBottom, tierHeight, 4, 1, true);
                    const mesh = new THREE.Mesh(geom, fillMat);
                    mesh.position.y = center;
                    mesh.rotation.y = Math.PI / 4;
                    invertGroup.add(mesh);
                    const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geom), wireMat);
                    wire.position.copy(mesh.position);
                    wire.rotation.copy(mesh.rotation);
                    invertGroup.add(wire);
                }
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); wireMat.color.setHex(p.hex2); },
                    animate(ctx) {
                        invertGroup.rotation.y += 0.003 + ctx.click * 0.02;
                        wireMat.opacity = 0.7 + ctx.click * 0.3;
                    },
                };
            },
        },

        // Adapted from A.R.X.L'KEMI: the cut-corner hexagonal panel with a wedge
        // rebuilt at each of its three cut corners to close the gap back into one
        // solid downward-triangle silhouette, simplified to the shell alone without
        // the core aperture, halo rings or particle column.
        triangleShell: {
            label: 'Triangle shell',
            build(api, options) {
                const group = new THREE.Group();
                const r = options.radius;
                const A = new THREE.Vector2(-r * 0.85, r * 0.5);
                const B = new THREE.Vector2(r * 0.85, r * 0.5);
                const C = new THREE.Vector2(0, -r);
                const cut = r * 0.78;
                const towards = (from, to, dist) => from.clone().add(to.clone().sub(from).normalize().multiplyScalar(dist));
                const pts = [
                    towards(A, B, cut), towards(B, A, cut),
                    towards(B, C, cut), towards(C, B, cut),
                    towards(C, A, cut), towards(A, C, cut),
                ];
                const shape = new THREE.Shape();
                shape.moveTo(pts[0].x, pts[0].y);
                for (let i = 1; i < pts.length; i++) shape.lineTo(pts[i].x, pts[i].y);
                shape.closePath();
                const thickness = r * 0.15;
                const panelGeom = new THREE.ExtrudeGeometry(shape, { depth: thickness, bevelEnabled: false });
                const fillMat = new THREE.MeshBasicMaterial({ color: 0x33ffb2, side: THREE.DoubleSide, transparent: true, opacity: 0.26, depthWrite: false });
                const wireMat = new THREE.LineBasicMaterial({ color: 0x33ffb2, transparent: true, opacity: 0.85 });
                group.add(new THREE.Mesh(panelGeom, fillMat));
                group.add(new THREE.LineSegments(new THREE.EdgesGeometry(panelGeom, 20), wireMat));

                function buildCornerWedge(cutPt1, corner, cutPt2) {
                    const ox = (cutPt1.x + cutPt2.x) / 2, oy = (cutPt1.y + cutPt2.y) / 2, oz = thickness / 2;
                    const p1 = new THREE.Vector3(cutPt1.x - ox, cutPt1.y - oy, -oz);
                    const p2 = new THREE.Vector3(cutPt1.x - ox, cutPt1.y - oy, oz);
                    const p3 = new THREE.Vector3(cutPt2.x - ox, cutPt2.y - oy, oz);
                    const p4 = new THREE.Vector3(cutPt2.x - ox, cutPt2.y - oy, -oz);
                    const apex = new THREE.Vector3(corner.x - ox, corner.y - oy, 0);
                    const tris = [p1, p2, apex, p2, p3, apex, p3, p4, apex, p4, p1, apex];
                    const positions = new Float32Array(tris.length * 3);
                    tris.forEach((v, i) => { positions[i * 3] = v.x; positions[i * 3 + 1] = v.y; positions[i * 3 + 2] = v.z; });
                    const geo = new THREE.BufferGeometry();
                    geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                    geo.computeVertexNormals();
                    return { geo, center: new THREE.Vector3(ox, oy, oz) };
                }
                [
                    buildCornerWedge(pts[5], A, pts[0]),
                    buildCornerWedge(pts[1], B, pts[2]),
                    buildCornerWedge(pts[3], C, pts[4]),
                ].forEach(({ geo, center }) => {
                    const mesh = new THREE.Mesh(geo, fillMat);
                    mesh.position.copy(center);
                    group.add(mesh);
                    const wire = new THREE.LineSegments(new THREE.EdgesGeometry(geo), wireMat);
                    wire.position.copy(center);
                    group.add(wire);
                });

                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); wireMat.color.setHex(p.hex2); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.12;
                        group.rotation.x = Math.sin(ctx.time * 0.4) * 0.1;
                        wireMat.opacity = 0.7 + ctx.click * 0.3;
                    },
                };
            },
        },

        // Adapted from A.R.X.LOREGENDA: the tilted halo standing in for a UI ring --
        // a single sprite ring rather than geometry, so it stays a clean circle at any
        // angle, and it leans rather than lying flat.
        tiltedHalo: {
            label: 'Tilted halo',
            build(api, options) {
                const mat = new THREE.SpriteMaterial({
                    map: api.helpers.ringTexture(64, 0.05), color: api.palette.hex3,
                    transparent: true, opacity: 0.5, depthWrite: false,
                });
                const sprite = new THREE.Sprite(mat);
                const size = options.radius * 2.1;
                sprite.scale.set(size, size, 1);
                sprite.rotation.z = Math.PI / 5;
                const group = new THREE.Group();
                group.add(sprite);
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.15;
                        group.rotation.x = Math.sin(ctx.time * 0.5) * 0.08;
                        mat.opacity = 0.42 + ctx.audio * 0.25 + ctx.click * 0.2;
                    },
                };
            },
        },

        // Adapted from A.R.X.LUCRE: the two halo bands, real ring geometry rather than
        // camera-facing sprites, tilted partway so a fixed front-on camera still sees
        // them as rings, and each tumbling at its own rate so they never lock together.
        tumblingHalos: {
            label: 'Tumbling halos',
            build(api, options) {
                const group = new THREE.Group();
                const configs = [
                    { inner: 1.0, tiltX: 1.15, tiltY: 0.1, opacity: 0.4, speed: 0.18, hot: false },
                    { inner: 1.14, tiltX: 1.3, tiltY: -0.15, opacity: 0.28, speed: -0.24, hot: true },
                ];
                const rings = configs.map((cfg) => {
                    const mat = new THREE.MeshBasicMaterial({
                        color: cfg.hot ? api.palette.hex3 : api.palette.hex2, side: THREE.DoubleSide,
                        transparent: true, opacity: cfg.opacity, depthWrite: false,
                    });
                    const inner = options.radius * cfg.inner;
                    const ring = new THREE.Mesh(new THREE.RingGeometry(inner, inner * 1.032, 64), mat);
                    ring.rotation.x = cfg.tiltX;
                    ring.rotation.y = cfg.tiltY;
                    group.add(ring);
                    return { ring, mat, cfg };
                });
                return {
                    object: group,
                    applyPalette(p) {
                        rings.forEach(({ mat, cfg }) => mat.color.setHex(cfg.hot ? p.hex3 : p.hex2));
                    },
                    animate(ctx) {
                        rings.forEach(({ ring, mat, cfg }) => {
                            ring.rotation.y += cfg.speed * 0.016;
                            mat.opacity = cfg.opacity + ctx.audio * 0.2 + ctx.click * 0.15;
                        });
                    },
                };
            },
        },

        // Adapted from A.R.X.LEXICO: the pair of gyro rings lying flat in the picture
        // plane, one broad and faint, one tighter and brighter. Sprites rather than
        // geometry on purpose -- a billboard always faces the camera, so these stay
        // true circles whatever the part they wrap is doing. Their counter-spin goes
        // through material.rotation, the only rotation the sprite shader reads.
        gyroRings: {
            label: 'Gyro rings',
            build(api, options) {
                const group = new THREE.Group();
                const outerMat = new THREE.SpriteMaterial({
                    map: api.helpers.ringTexture(64, 0.035), color: api.palette.hex2,
                    transparent: true, opacity: 0.45, depthWrite: false,
                });
                const outer = new THREE.Sprite(outerMat);
                const outerSize = options.radius * 2.6;
                outer.scale.set(outerSize, outerSize, 1);
                const innerMat = new THREE.SpriteMaterial({
                    map: api.helpers.ringTexture(64, 0.06), color: api.palette.hex3,
                    transparent: true, opacity: 0.6, depthWrite: false,
                });
                const inner = new THREE.Sprite(innerMat);
                const innerSize = options.radius * 2.1;
                inner.scale.set(innerSize, innerSize, 1);
                group.add(outer, inner);
                return {
                    object: group,
                    applyPalette(p) { outerMat.color.setHex(p.hex2); innerMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        outer.material.rotation = ctx.time * 0.25;
                        inner.material.rotation = -ctx.time * 0.4;
                        outerMat.opacity = 0.4 + ctx.audio * 0.25;
                        innerMat.opacity = 0.5 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },

        // Adapted from A.R.X.LOCAS: the two broken-circle rings that sit around it like
        // gauge markings -- dashed lines rather than solid bands, tilted well off the
        // picture plane and counter-rotating, so the gaps in them are what shows the
        // motion.
        dashedGaugeRings: {
            label: 'Dashed gauge rings',
            build(api, options) {
                const group = new THREE.Group();
                group.rotation.x = Math.PI / 2.4;
                function dashedRing(radius, dashSize, gapSize, mat, segments = 72) {
                    const pts = [];
                    for (let i = 0; i <= segments; i++) {
                        const a = (i / segments) * Math.PI * 2;
                        pts.push(new THREE.Vector3(Math.cos(a) * radius, Math.sin(a) * radius, 0));
                    }
                    const line = new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(pts), mat);
                    line.computeLineDistances();
                    return line;
                }
                const outerMat = new THREE.LineDashedMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.5, dashSize: 9, gapSize: 5,
                });
                const innerMat = new THREE.LineDashedMaterial({
                    color: api.palette.hex3, transparent: true, opacity: 0.4, dashSize: 5, gapSize: 4,
                });
                const outer = dashedRing(options.radius * 1.3, 9, 5, outerMat);
                const inner = dashedRing(options.radius * 1.13, 5, 4, innerMat);
                group.add(outer, inner);
                return {
                    object: group,
                    applyPalette(p) { outerMat.color.setHex(p.hex); innerMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        outer.rotation.z = ctx.time * 0.12;
                        inner.rotation.z = -ctx.time * 0.19;
                        outerMat.opacity = 0.45 + ctx.audio * 0.25;
                        innerMat.opacity = 0.35 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },

        /* Adapted from m.A.I.r.a.d. and enXephalon, which both sweep what they are looking at
           with rings on crossed tilts. Neither is edge-on: at exactly a quarter turn the
           level camera sees a ring as a straight line drawn through the avatar rather
           than as a ring around it. Four ticks in the picture plane turn the pair into a
           sight -- a ring on its own is decoration, a ring with marks on it is an
           instrument reading something. */
        crossedTargetRings: {
            label: 'Crossed target rings',
            build(api, options) {
                const HOT = 0xff0e8c;
                const group = new THREE.Group();
                const innerMat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const inner = new THREE.Mesh(
                    new THREE.TorusGeometry(options.radius, options.radius * 0.014, 8, 96), innerMat
                );
                inner.rotation.x = Math.PI / 2.55;
                const outerMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
                });
                const outer = new THREE.Mesh(
                    new THREE.TorusGeometry(options.radius * 1.22, options.radius * 0.01, 8, 96), outerMat
                );
                outer.rotation.x = -Math.PI / 2.9;
                group.add(inner, outer);

                const tickMat = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
                });
                const ticks = new THREE.Group();
                ticks.position.z = options.radius * 0.9;
                for (let i = 0; i < 4; i++) {
                    ticks.add(new THREE.Mesh(
                        new THREE.RingGeometry(options.radius * 0.6, options.radius * 0.8, 8, 1, i * Math.PI / 2 - 0.06, 0.12),
                        tickMat
                    ));
                }
                group.add(ticks);

                return {
                    object: group,
                    applyPalette(pal) { outerMat.color.setHex(pal.hex); tickMat.color.setHex(pal.hex); },
                    animate(ctx) {
                        inner.rotation.z = -ctx.time * 0.6;
                        outer.rotation.y = ctx.time * 0.3;
                        ticks.rotation.z = ctx.time * 0.25;
                        const tighten = 1 + Math.sin(ctx.time * 6) * 0.02 - ctx.click * 0.06;
                        group.scale.setScalar(tighten);
                        innerMat.opacity = 0.7 + ctx.audio * 0.3;
                        outerMat.opacity = 0.45 + ctx.audio * 0.3;
                        tickMat.opacity = 0.6 + ctx.audio * 0.3;
                    },
                };
            },
        },

        /* Adapted from vA1or: the four corner brackets that frame whatever is being
           looked at. Each corner is two strokes meeting at a right angle, built from one
           corner's numbers mirrored into the other three -- four hand-written corners is
           four chances to typo a sign. They draw in the picture plane rather than around
           the avatar, so they frame it instead of orbiting it. */
        cornerBrackets: {
            label: 'Corner brackets',
            build(api, options) {
                const mat = new THREE.LineBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.95,
                });
                const bx = options.radius * 0.95;
                const by = options.radius * 0.74;
                const arm = options.radius * 0.2;
                const pts = [];
                [[-1, 1], [1, 1], [-1, -1], [1, -1]].forEach(([sx, sy]) => {
                    const corner = new THREE.Vector3(sx * bx, sy * by, 0);
                    pts.push(corner, new THREE.Vector3(sx * (bx - arm), sy * by, 0));
                    pts.push(corner.clone(), new THREE.Vector3(sx * bx, sy * (by - arm), 0));
                });
                const brackets = new THREE.LineSegments(
                    new THREE.BufferGeometry().setFromPoints(pts), mat
                );
                return {
                    object: brackets,
                    applyPalette(pal) { mat.color.setHex(pal.hex); },
                    animate(ctx) {
                        // Breathing out at rest and snapping in on a click: the gesture is
                        // the frame closing on something, so it is a scale, not a spin.
                        brackets.scale.setScalar(1 + Math.sin(ctx.time * 2) * 0.015 - ctx.click * 0.1);
                        mat.opacity = 0.75 + ctx.audio * 0.25 + ctx.click * 0.2;
                    },
                };
            },
        },
        /* Adapted from CorvA1s: the three hoops it hangs in, each turning on a
           different pair of axes. Nothing else in that avatar turns on more than one
           axis, and the three of them disagreeing is what keeps a shut assembly from
           reading as a ball sitting still. Wireframe rather than solid, so a hoop caught
           edge-on is a line of mesh and not a bar across the picture. */
        gimbalHoops: {
            label: 'Gimbal hoops',
            build(api, options) {
                const group = new THREE.Group();
                const R = options.radius;
                const mats = [
                    new THREE.MeshBasicMaterial({ color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending }),
                    new THREE.MeshBasicMaterial({ color: api.palette.hex2, wireframe: true, transparent: true, opacity: 0.42, blending: THREE.AdditiveBlending }),
                    new THREE.MeshBasicMaterial({ color: api.palette.hex, wireframe: true, transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending }),
                ];
                const hoops = [
                    new THREE.Mesh(new THREE.TorusGeometry(R * 0.56, R * 0.008, 5, 64), mats[0]),
                    new THREE.Mesh(new THREE.TorusGeometry(R * 0.77, R * 0.006, 5, 72), mats[1]),
                    new THREE.Mesh(new THREE.TorusGeometry(R, R * 0.004, 5, 80), mats[2]),
                ];
                hoops.forEach((h) => group.add(h));
                return {
                    object: group,
                    applyPalette(pal) {
                        mats[0].color.setHex(pal.hex);
                        mats[1].color.setHex(pal.hex2);
                        mats[2].color.setHex(pal.hex);
                    },
                    animate(ctx) {
                        const spin = ctx.state === 'THINKING' ? 1.9 : ctx.state === 'LISTENING' ? 0.45 : 1;
                        const t = ctx.time * spin;
                        hoops[0].rotation.set(t * 0.5, t * 0.3, 0);
                        hoops[1].rotation.set(0, -t * 0.4, t * 0.2);
                        hoops[2].rotation.set(-t * 0.2, 0, -t * 0.6);
                        mats[0].opacity = 0.5 + ctx.audio * 0.22;
                        mats[1].opacity = 0.42 + ctx.audio * 0.2;
                        mats[2].opacity = 0.32 + ctx.audio * 0.16;
                    },
                };
            },
        },
        /* Adapted from Divinai: two dashed tactical rings counter-turning, the inner one
           coarse and the outer one fine. Every other segment is dropped rather than a gap
           being drawn, which is what makes it read as a readout and not as a ring with
           bites out of it. */
        dashedTacticalRings: {
            label: 'Dashed tactical rings',
            build(api, options) {
                const group = new THREE.Group();
                const R = options.radius;
                const matA = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.8,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const matB = new THREE.MeshBasicMaterial({
                    color: api.palette.hex, transparent: true, opacity: 0.5,
                    side: THREE.DoubleSide, blending: THREE.AdditiveBlending,
                });
                const dashed = (radius, dashes, mat) => {
                    const ring = new THREE.Group();
                    const step = (Math.PI * 2) / dashes;
                    for (let i = 1; i < dashes; i += 2) {
                        ring.add(new THREE.Mesh(
                            new THREE.RingGeometry(radius, radius + R * 0.019, 16, 1, i * step, step * 0.8),
                            mat
                        ));
                    }
                    group.add(ring);
                    return ring;
                };
                const inner = dashed(R * 0.86, 12, matA);
                const outer = dashed(R, 24, matB);
                return {
                    object: group,
                    applyPalette(pal) { matA.color.setHex(pal.hex); matB.color.setHex(pal.hex); },
                    animate(ctx) {
                        inner.rotation.z = ctx.time * 0.3;
                        outer.rotation.z = -ctx.time * 0.15;
                        matA.opacity = 0.65 + ctx.audio * 0.3;
                        matB.opacity = 0.4 + ctx.audio * 0.25;
                    },
                };
            },
        },
        /* Adapted from TithesA1s: the teal lattice inside it -- the part that never phases.
           Two rotations on different axes, and nothing else; it is the still point the
           matter comes and goes around, so it does not react to anything but the voice. */
        neuralLattice: {
            label: 'Neural lattice',
            build(api, options) {
                const LATTICE = 0x00ffcc;
                const mat = new THREE.MeshBasicMaterial({
                    color: LATTICE, wireframe: true,
                    transparent: true, opacity: 0.45, blending: THREE.AdditiveBlending,
                });
                const mesh = new THREE.Mesh(new THREE.IcosahedronGeometry(options.radius, 2), mat);
                return {
                    object: mesh,
                    applyPalette() {
                        // Keeps its own teal.
                    },
                    animate(ctx) {
                        mesh.rotation.y = -ctx.time * 0.15;
                        mesh.rotation.x = ctx.time * 0.05;
                        mat.opacity = 0.35 + ctx.audio * 0.3;
                    },
                };
            },
        },
    };

    // ---- Effects: an ambient layer or background ------------------------------

    const EFFECTS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
        },

        auroraBorealis: {
            label: 'Aurora veil (Borealis)',
            build(api, options) { return buildAuroraVeil(api, options, AURORA.borealis); },
        },

        auroraAustralis: {
            label: 'Aurora veil (Australis)',
            build(api, options) { return buildAuroraVeil(api, options, AURORA.australis); },
        },

        lattice: {
            label: 'Particle lattice',
            build(api, options) {
                const count = 2200;
                const geom = new THREE.BufferGeometry();
                const positions = new Float32Array(count * 3);
                const colors = new Float32Array(count * 3);
                const base = [];
                for (let i = 0; i < count; i++) {
                    const theta = Math.random() * Math.PI * 2;
                    const phi = Math.acos(2 * Math.random() - 1);
                    const r = options.radius * (0.85 + Math.random() * 0.15);
                    const v = new THREE.Vector3(
                        r * Math.sin(phi) * Math.cos(theta),
                        r * Math.sin(phi) * Math.sin(theta),
                        r * Math.cos(phi)
                    );
                    base.push(v);
                    positions.set([v.x, v.y, v.z], i * 3);
                }
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                const mat = new THREE.PointsMaterial({
                    size: 1.7, vertexColors: true, transparent: true, opacity: 0.9,
                    blending: THREE.AdditiveBlending, depthWrite: false,
                    map: api.helpers.glowTexture(32),
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) {
                        for (let i = 0; i < count; i++) {
                            colors[i * 3] = p.r;
                            colors[i * 3 + 1] = Math.min(1, p.g + Math.random() * 0.15);
                            colors[i * 3 + 2] = p.b;
                        }
                        geom.attributes.color.needsUpdate = true;
                    },
                    animate(ctx) {
                        const push = 1 + ctx.audio * 0.5 + ctx.click * 0.3;
                        for (let i = 0; i < count; i++) {
                            const v = base[i];
                            const wave = Math.sin(ctx.time * 1.5 + v.x * 0.05 + v.y * 0.05) * 3;
                            positions[i * 3] = v.x * push + wave * 0.3;
                            positions[i * 3 + 1] = v.y * push + wave * 0.3;
                            positions[i * 3 + 2] = v.z * push;
                        }
                        geom.attributes.position.needsUpdate = true;
                        points.rotation.y = ctx.time * 0.12;
                    },
                };
            },
        },

        swarm: {
            label: 'Orbiting swarm',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: 0x00f0ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const nodes = [];
                for (let i = 0; i < 26; i++) {
                    const mesh = new THREE.Mesh(new THREE.OctahedronGeometry(options.radius * 0.06), mat);
                    nodes.push({
                        mesh,
                        radius: options.radius * (1.05 + Math.random() * 0.5),
                        angle: Math.random() * Math.PI * 2,
                        height: (Math.random() - 0.5) * options.radius * 1.2,
                        speed: 0.15 + Math.random() * 0.35,
                        bob: Math.random() * Math.PI * 2,
                    });
                    group.add(mesh);
                }
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        const spread = 1 + ctx.audio * 0.35 + ctx.click * 0.25;
                        nodes.forEach((n) => {
                            const a = n.angle + ctx.time * n.speed;
                            n.mesh.position.set(
                                Math.cos(a) * n.radius * spread,
                                n.height + Math.sin(ctx.time + n.bob) * 4,
                                Math.sin(a) * n.radius * spread
                            );
                            n.mesh.rotation.y = ctx.time * n.speed * 2;
                        });
                    },
                };
            },
        },

        stack: {
            label: 'Vertical bars',
            build(api, options) {
                const group = new THREE.Group();
                const BARS = 12;
                const bars = [];
                const mats = [];
                const span = Math.min(options.radius, 76);
                for (let i = 0; i < BARS; i++) {
                    const mat = new THREE.MeshBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                    });
                    const bar = new THREE.Mesh(new THREE.BoxGeometry(span * 0.09, 2, 2), mat);
                    bar.position.x = (i - (BARS - 1) / 2) * span * 0.13;
                    group.add(bar);
                    bars.push({ bar, index: i });
                    mats.push(mat);
                }
                group.position.y = -Math.min(options.radius * 1.15, 76);
                return {
                    object: group,
                    applyPalette(p) {
                        const lo = new THREE.Color(p.hex2);
                        const hi = new THREE.Color(p.hex3);
                        mats.forEach((m, i) => m.color.copy(lo).lerp(hi, i / (BARS - 1)));
                    },
                    animate(ctx) {
                        bars.forEach(({ bar, index }) => {
                            const level = band(ctx, index, BARS);
                            const h = 2 + level * span * 1.1 + ctx.click * 4;
                            bar.scale.y = h / 2;
                            bar.position.y = h / 2;
                        });
                    },
                };
            },
        },

        // Adapted from Operator: short vertical dash sprites in fixed angular slots,
        // each sliding from a clear centre out past the edges and fading near the rim
        // -- a static frame with a field of code constantly flowing outward inside it.
        codeRainField: {
            label: 'Code-rain field',
            build(api, options) {
                const inner = options.radius * 0.4;
                const outer = options.radius * 3.4;
                const span = outer - inner;
                function tick(size, widthFrac, heightFrac) {
                    const canvas = document.createElement('canvas');
                    canvas.width = size; canvas.height = size;
                    const c = canvas.getContext('2d');
                    const w = size * widthFrac, h = size * heightFrac;
                    const x = (size - w) / 2, y = (size - h) / 2, r = w / 2;
                    const grad = c.createLinearGradient(0, y, 0, y + h);
                    grad.addColorStop(0, 'rgba(255,255,255,0)');
                    grad.addColorStop(0.18, 'rgba(255,255,255,1)');
                    grad.addColorStop(0.82, 'rgba(255,255,255,1)');
                    grad.addColorStop(1, 'rgba(255,255,255,0)');
                    c.fillStyle = grad;
                    c.beginPath();
                    c.moveTo(x + r, y);
                    c.arcTo(x + w, y, x + w, y + h, r);
                    c.arcTo(x + w, y + h, x, y + h, r);
                    c.arcTo(x, y + h, x, y, r);
                    c.arcTo(x, y, x + w, y, r);
                    c.closePath();
                    c.fill();
                    return new THREE.CanvasTexture(canvas);
                }
                function stream(texture, count, color, size, opacity) {
                    const positions = new Float32Array(count * 3);
                    const colors = new Float32Array(count * 3);
                    const state = new Array(count);
                    for (let i = 0; i < count; i++) {
                        const brightness = 0.75 + Math.random() * 0.55;
                        state[i] = {
                            angle: Math.random() * Math.PI * 2,
                            speed: (10 + Math.random() * 16) * (options.radius / 62),
                            offset: Math.random() * span,
                            z: (-14 + Math.random() * 28) * (options.radius / 62),
                        };
                        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = brightness;
                    }
                    const geom = new THREE.BufferGeometry();
                    geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                    geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                    const mat = new THREE.PointsMaterial({
                        color, map: texture, size, vertexColors: true, transparent: true, opacity,
                        depthWrite: false, blending: THREE.AdditiveBlending, sizeAttenuation: true,
                    });
                    return { points: new THREE.Points(geom, mat), mat, positions, colors, state };
                }
                const primary = stream(tick(64, 0.34, 0.92), Math.round(900 * (options.radius / 62)), api.palette.hex, 8, 0.9);
                const accent = stream(tick(64, 0.2, 0.6), Math.round(350 * (options.radius / 62)), api.palette.hex2, 5.5, 0.7);
                const group = new THREE.Group();
                group.add(primary.points, accent.points);
                function updateStream(s, ctx, speedMult) {
                    const { positions, colors, state } = s;
                    for (let i = 0; i < state.length; i++) {
                        const st = state[i];
                        const flow = (ctx.time * st.speed * speedMult + st.offset) % span;
                        const radius = inner + flow;
                        const p = flow / span;
                        const fade = p < 0.12 ? p / 0.12 : (p > 0.72 ? Math.max(0, (1 - p) / 0.28) : 1);
                        positions[i * 3] = Math.cos(st.angle) * radius;
                        positions[i * 3 + 1] = Math.sin(st.angle) * radius;
                        positions[i * 3 + 2] = st.z;
                        colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = fade;
                    }
                    s.points.geometry.attributes.position.needsUpdate = true;
                    s.points.geometry.attributes.color.needsUpdate = true;
                }
                return {
                    object: group,
                    applyPalette(p) { primary.mat.color.setHex(p.hex); accent.mat.color.setHex(p.hex2); },
                    animate(ctx) {
                        let speedMult = 1;
                        if (ctx.state === 'THINKING') speedMult = 2.2;
                        else if (ctx.state === 'LISTENING') speedMult = 1.4;
                        else if (ctx.state === 'SPEAKING') speedMult = 1.1 + ctx.audio * 0.8;
                        speedMult += ctx.click * 1.2;
                        updateStream(primary, ctx, speedMult);
                        updateStream(accent, ctx, speedMult);
                    },
                };
            },
        },

        // Adapted from hAlcy: a sphere of points distributed by golden-angle spiral
        // (an even spread with no clustering at the poles), breathing with the voice.
        harmonicLattice: {
            label: 'Harmonic lattice',
            build(api, options) {
                const count = 1400;
                const radius = options.radius * 0.85;
                const geom = new THREE.BufferGeometry();
                const positions = new Float32Array(count * 3);
                const colors = new Float32Array(count * 3);
                const base = [];
                for (let i = 0; i < count; i++) {
                    const phi = Math.acos(-1 + (2 * i) / count);
                    const theta = Math.sqrt(count * Math.PI) * phi;
                    const v = new THREE.Vector3(
                        radius * Math.cos(theta) * Math.sin(phi),
                        radius * Math.sin(theta) * Math.sin(phi),
                        radius * Math.cos(phi)
                    );
                    base.push(v);
                    positions.set([v.x, v.y, v.z], i * 3);
                }
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                const mat = new THREE.PointsMaterial({
                    size: 2.4, vertexColors: true, map: api.helpers.glowTexture(32),
                    transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) {
                        for (let i = 0; i < count; i++) {
                            colors[i * 3] = p.r; colors[i * 3 + 1] = Math.min(1, p.g + 0.1); colors[i * 3 + 2] = p.b;
                        }
                        geom.attributes.color.needsUpdate = true;
                    },
                    animate(ctx) {
                        const push = 1 + ctx.audio * 0.18 + ctx.click * 0.1;
                        points.scale.setScalar(push);
                        points.rotation.y = ctx.time * 0.08;
                    },
                };
            },
        },

        // Adapted from A.R.X.LOGOS: small hex nodes drifting freely in 3D around the
        // whole structure, unlike anything locked to a flat plane.
        hexSwarm: {
            label: 'Hex swarm',
            build(api, options) {
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: 0xe024c3, transparent: true, opacity: 0.65, blending: THREE.AdditiveBlending,
                });
                const nodes = [];
                const count = 20;
                for (let i = 0; i < count; i++) {
                    const radius = options.radius * (1.1 + Math.random() * 1.1);
                    const angle = Math.random() * Math.PI * 2;
                    const heightOffset = (Math.random() - 0.5) * options.radius * 1.1;
                    const node = hexFillMesh(api, 2.5 + Math.random() * 1.8, Math.random() * Math.PI, mat);
                    node.position.set(Math.cos(angle) * radius, heightOffset, Math.sin(angle) * radius);
                    node.userData = { radius, angle, heightOffset, speed: 0.15 + Math.random() * 0.25, bob: Math.random() * Math.PI * 2 };
                    nodes.push(node);
                    group.add(node);
                }
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        nodes.forEach((n) => {
                            const d = n.userData;
                            const a = d.angle + ctx.time * d.speed;
                            n.position.set(Math.cos(a) * d.radius, d.heightOffset + Math.sin(ctx.time + d.bob) * 4, Math.sin(a) * d.radius);
                        });
                    },
                };
            },
        },

        // Adapted from A.R.X.LOREGENDA: the ambient dust -- a thin flattened shell of
        // points well outside the body, turning slowly enough to read as drift rather
        // than rotation.
        driftingDust: {
            label: 'Drifting dust',
            build(api, options) {
                const count = 70;
                const positions = new Float32Array(count * 3);
                for (let i = 0; i < count * 3; i += 3) {
                    const r = options.radius * (1.4 + Math.random() * 0.8);
                    const theta = Math.random() * Math.PI * 2;
                    const phi = Math.acos(Math.random() * 2 - 1);
                    positions[i] = r * Math.sin(phi) * Math.cos(theta);
                    positions[i + 1] = r * Math.cos(phi) * 0.6;
                    positions[i + 2] = r * Math.sin(phi) * Math.sin(theta);
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                const mat = new THREE.PointsMaterial({
                    color: api.palette.hex, size: 1.4, transparent: true, opacity: 0.55,
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        points.rotation.y = ctx.time * 0.05;
                        mat.opacity = 0.5 + ctx.audio * 0.3 + ctx.click * 0.2;
                    },
                };
            },
        },

        // Adapted from A.R.X.LYKSAUM: the soft horizontal bar that sweeps up and down
        // the face of its medallion on a six-second cycle. A billboard, so it stays a
        // flat bar across whatever it passes in front of.
        scanSweep: {
            label: 'Scan sweep',
            build(api, options) {
                const mat = new THREE.SpriteMaterial({
                    map: api.helpers.glowTexture(64), color: api.palette.hex3,
                    transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const bar = new THREE.Sprite(mat);
                const reach = options.radius * 1.2;
                bar.scale.set(reach * 2.1, reach * 0.16, 1);
                bar.position.z = options.radius * 0.5;
                const group = new THREE.Group();
                group.add(bar);
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex3); },
                    animate(ctx) {
                        const t = (ctx.time % 6) / 6;
                        bar.position.y = -reach + t * reach * 2;
                        mat.opacity = 0.28 + ctx.audio * 0.25 + ctx.click * 0.15;
                    },
                };
            },
        },

        // Adapted from A.R.X.L'KEMI: the column of data particles rising past it and
        // wrapping back to the bottom -- a hollow cylinder of points rather than a
        // sphere, so it reads as a stream running through whatever it surrounds rather
        // than an atmosphere around it.
        risingDataColumn: {
            label: 'Rising data column',
            build(api, options) {
                const half = options.radius * 1.55;
                const count = 80;
                const positions = new Float32Array(count * 3);
                for (let i = 0; i < count; i++) {
                    const angle = Math.random() * Math.PI * 2;
                    const r = options.radius * (1.12 + Math.random() * 0.7);
                    positions[i * 3] = Math.cos(angle) * r;
                    positions[i * 3 + 1] = (Math.random() * 2 - 1) * half;
                    positions[i * 3 + 2] = Math.sin(angle) * r;
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                const mat = new THREE.PointsMaterial({
                    color: api.palette.hex, size: 1.3, transparent: true, opacity: 0.55,
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        const array = points.geometry.attributes.position.array;
                        const rise = 0.35 + ctx.audio * 0.9;
                        for (let i = 1; i < array.length; i += 3) {
                            array[i] += rise;
                            if (array[i] > half) array[i] -= half * 2;
                        }
                        points.geometry.attributes.position.needsUpdate = true;
                        mat.opacity = 0.5 + ctx.audio * 0.3 + ctx.click * 0.15;
                    },
                };
            },
        },

        /* Adapted from PRAXIS: the lit floor the whole avatar stands on, running away
           underneath and wrapping at one cell. Every vertex is dimmed by its own distance
           from the centre, so the rows the seam passes through are already black by the
           time it gets there and the wrap never shows. The vignette is baked into the
           geometry's own vertex colours rather than drawn over the top, which is what
           lets the theme reach it and what keeps it off whatever stands in the middle. */
        hardLightFloor: {
            label: 'Hard-light floor',
            build(api, options) {
                const HALF = options.radius * 2.6;
                const CELL = HALF / 12;
                const STEPS = 6;
                const y = -options.radius * 1.4;
                const positions = [];
                const factors = [];
                const push = (x1, z1, x2, z2) => {
                    positions.push(x1, y, z1, x2, y, z2);
                    [[x1, z1], [x2, z2]].forEach(([x, z]) => {
                        // Falls away faster than distance does, so the far rows are gone
                        // well before the rim rather than ending on a visible edge.
                        const r = Math.sqrt(x * x + z * z) / HALF;
                        factors.push(Math.pow(Math.max(0, 1 - r), 1.2));
                    });
                };
                for (let n = -12; n <= 12; n++) {
                    const fixed = n * CELL;
                    for (let s = 0; s < STEPS * 2; s++) {
                        const a = -HALF + (s / (STEPS * 2)) * HALF * 2;
                        const b = -HALF + ((s + 1) / (STEPS * 2)) * HALF * 2;
                        push(fixed, a, fixed, b);
                        push(a, fixed, b, fixed);
                    }
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.Float32BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(factors.length * 3), 3));
                const mat = new THREE.LineBasicMaterial({
                    vertexColors: true, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                });
                const floor = new THREE.LineSegments(geom, mat);
                const tint = new THREE.Color(api.palette.hex);
                function paint() {
                    const attr = geom.attributes.color;
                    for (let i = 0; i < factors.length; i++) {
                        attr.array[i * 3] = tint.r * factors[i];
                        attr.array[i * 3 + 1] = tint.g * factors[i];
                        attr.array[i * 3 + 2] = tint.b * factors[i];
                    }
                    attr.needsUpdate = true;
                }
                paint();
                let scroll = 0;
                return {
                    object: floor,
                    applyPalette(pal) { tint.setHex(pal.hex); paint(); },
                    animate(ctx) {
                        // Wrapping at one cell is what makes a finite grid look endless,
                        // and accumulating the offset means a change of pace never jumps
                        // the floor sideways.
                        scroll = (scroll + (22 + ctx.audio * 30) * 0.016) % CELL;
                        floor.position.z = scroll;
                        mat.opacity = 0.8 + ctx.audio * 0.2 + ctx.click * 0.1;
                    },
                };
            },
        },

        /* Adapted from vA1or: the drone telemetry network drifting behind the overlay,
           each node wired to whichever neighbours it is near. Which pairs those are really
           does change as the nodes drift, so unlike a fixed lattice it has to be re-derived
           every frame -- but the line buffer is allocated once here and only its draw range
           moves, and the test runs on squared distances, since a square root per pair buys
           nothing when the only question is which side of a threshold the pair falls on. */
        droneNetwork: {
            label: 'Drone network',
            build(api, options) {
                const TELEMETRY = 0x00ffaa;
                const COUNT = 40;
                const MAX_LINKS = 320;
                const box = {
                    x: options.radius * 1.1, y: options.radius * 0.75, z: options.radius * 0.4,
                };
                const link = options.radius * 0.42;
                const pos = new Float32Array(COUNT * 3);
                const drift = [];
                for (let i = 0; i < COUNT; i++) {
                    pos[i * 3] = (Math.random() - 0.5) * box.x * 2;
                    pos[i * 3 + 1] = (Math.random() - 0.5) * box.y * 2;
                    pos[i * 3 + 2] = (Math.random() - 0.5) * box.z * 2;
                    drift.push({
                        x: (Math.random() - 0.5) * 0.14,
                        y: (Math.random() - 0.5) * 0.14,
                        z: (Math.random() - 0.5) * 0.07,
                    });
                }
                const group = new THREE.Group();
                const nodeGeom = new THREE.BufferGeometry();
                nodeGeom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
                const nodeMat = new THREE.PointsMaterial({
                    map: api.helpers.glowTexture(32), color: TELEMETRY, size: options.radius * 0.055,
                    transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
                    depthWrite: false, sizeAttenuation: true,
                });
                group.add(new THREE.Points(nodeGeom, nodeMat));

                const linkGeom = new THREE.BufferGeometry();
                const linkPos = new Float32Array(MAX_LINKS * 6);
                linkGeom.setAttribute('position', new THREE.BufferAttribute(linkPos, 3));
                linkGeom.setDrawRange(0, 0);
                const linkMat = new THREE.LineBasicMaterial({
                    color: api.palette.hex2, transparent: true, opacity: 0.3, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.LineSegments(linkGeom, linkMat));

                const limit = link * link;
                return {
                    object: group,
                    applyPalette(pal) { linkMat.color.setHex(pal.hex2); },
                    animate(ctx) {
                        const pace = 1 + ctx.audio * 1.2;
                        for (let i = 0; i < COUNT; i++) {
                            pos[i * 3] += drift[i].x * pace;
                            pos[i * 3 + 1] += drift[i].y * pace;
                            pos[i * 3 + 2] += drift[i].z * pace;
                            // Turned at the wall rather than wrapped: a node that wraps
                            // crosses the field in one frame and drags a line with it.
                            if (Math.abs(pos[i * 3]) > box.x) drift[i].x *= -1;
                            if (Math.abs(pos[i * 3 + 1]) > box.y) drift[i].y *= -1;
                            if (Math.abs(pos[i * 3 + 2]) > box.z) drift[i].z *= -1;
                        }
                        nodeGeom.attributes.position.needsUpdate = true;

                        let n = 0;
                        for (let i = 0; i < COUNT && n < MAX_LINKS; i++) {
                            for (let j = i + 1; j < COUNT; j++) {
                                const dx = pos[i * 3] - pos[j * 3];
                                const dy = pos[i * 3 + 1] - pos[j * 3 + 1];
                                const dz = pos[i * 3 + 2] - pos[j * 3 + 2];
                                if (dx * dx + dy * dy + dz * dz > limit) continue;
                                const at = n * 6;
                                linkPos[at] = pos[i * 3];
                                linkPos[at + 1] = pos[i * 3 + 1];
                                linkPos[at + 2] = pos[i * 3 + 2];
                                linkPos[at + 3] = pos[j * 3];
                                linkPos[at + 4] = pos[j * 3 + 1];
                                linkPos[at + 5] = pos[j * 3 + 2];
                                if (++n >= MAX_LINKS) break;
                            }
                        }
                        linkGeom.setDrawRange(0, n * 2);
                        linkGeom.attributes.position.needsUpdate = true;
                        linkMat.opacity = 0.22 + ctx.audio * 0.25;
                        nodeMat.opacity = 0.75 + ctx.audio * 0.25 + ctx.click * 0.2;
                    },
                };
            },
        },
        /* Adapted from CorvA1s: the volume the readings are taken from, and the
           callouts reaching into it. A shell of points with six leaders drawn from the
           middle out to anchors fixed in it, each one carrying a tick that sits on its
           anchor at rest and runs the length of the leader while there is a voice -- a
           line being read off rather than six lights blinking together.

           The anchors are fixed at build. A callout that wandered would be an animation
           and not a measurement, which is the whole difference between this and a swarm.
           Leaders and ticks keep one fixed colour through a theme change for the same
           reason: a measurement that matched the furniture would read as furniture. */
        annotationShell: {
            label: 'Annotation shell',
            build(api, options) {
                const group = new THREE.Group();
                const R = options.radius;
                const POINTS = 420;
                const LEADERS = 6;
                const ANNOTATION = 0xffaa00;

                const pos = new Float32Array(POINTS * 3);
                for (let i = 0; i < POINTS; i++) {
                    const r = R * (0.58 + Math.random() * 0.42);
                    const theta = Math.random() * Math.PI * 2;
                    // Arc cosine of a uniform number, so the points spread evenly over
                    // the sphere instead of bunching at its poles.
                    const phi = Math.acos(Math.random() * 2 - 1);
                    pos[i * 3] = r * Math.sin(phi) * Math.cos(theta);
                    pos[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
                    pos[i * 3 + 2] = r * Math.cos(phi);
                }
                const shellGeom = new THREE.BufferGeometry();
                shellGeom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
                // Mapped, not bare: an unmapped point sprite is a hard square.
                const shellMat = new THREE.PointsMaterial({
                    map: api.helpers.glowTexture(32), color: api.palette.hex2, size: R * 0.037,
                    transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending,
                    depthWrite: false, sizeAttenuation: true,
                });
                const shell = new THREE.Points(shellGeom, shellMat);
                group.add(shell);

                const starts = [];
                const anchors = [];
                for (let i = 0; i < LEADERS; i++) {
                    const a = (i / LEADERS) * Math.PI * 2;
                    const tilt = (i % 2 ? 1 : -1) * 0.45;
                    const dir = new THREE.Vector3(
                        Math.cos(a) * Math.cos(tilt), Math.sin(tilt), Math.sin(a) * Math.cos(tilt)
                    );
                    starts.push(dir.clone().multiplyScalar(R * 0.3));
                    anchors.push(dir.clone().multiplyScalar(R * 0.84));
                }
                const annGeom = new THREE.BufferGeometry();
                const annPos = new Float32Array(LEADERS * 2 * 6);
                annGeom.setAttribute('position', new THREE.BufferAttribute(annPos, 3));
                const annMat = new THREE.LineBasicMaterial({
                    color: ANNOTATION, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
                });
                group.add(new THREE.LineSegments(annGeom, annMat));

                const tick = new THREE.Vector3();
                const arm = R * 0.06;
                return {
                    object: group,
                    applyPalette(pal) {
                        shellMat.color.setHex(pal.hex2);
                        // The leaders and ticks keep their fixed colour -- see above.
                    },
                    animate(ctx) {
                        shell.rotation.y = ctx.time * 0.05;
                        shellMat.opacity = 0.5 + ctx.audio * 0.3 + ctx.click * 0.15;
                        const speaking = ctx.state === 'SPEAKING';
                        let n = 0;
                        const put = (a, b) => {
                            const at = n * 6;
                            annPos[at] = a.x; annPos[at + 1] = a.y; annPos[at + 2] = a.z;
                            annPos[at + 3] = b.x; annPos[at + 4] = b.y; annPos[at + 5] = b.z;
                            n++;
                        };
                        for (let i = 0; i < LEADERS; i++) {
                            put(starts[i], anchors[i]);
                            const u = speaking ? (ctx.time * 0.6 + i * 0.16) % 1 : 1;
                            tick.lerpVectors(starts[i], anchors[i], u);
                            put(
                                tick.clone().add(new THREE.Vector3(-arm, arm * 0.6, 0)),
                                tick.clone().add(new THREE.Vector3(arm, -arm * 0.6, 0))
                            );
                        }
                        annGeom.setDrawRange(0, n * 2);
                        annGeom.attributes.position.needsUpdate = true;
                        annMat.opacity = 0.6 + ctx.audio * 0.3;
                    },
                };
            },
        },
        /* Adapted from Divinai: the warm ember field behind it, turning slowly on one axis
           and rocking on another. It keeps its own amber through a theme change, because
           an ember that came out blue is not an ember. */
        emberField: {
            label: 'Ember field',
            build(api, options) {
                const COUNT = 520;
                const R = options.radius;
                const pos = new Float32Array(COUNT * 3);
                for (let i = 0; i < COUNT; i++) {
                    const r = R * (0.47 + Math.random() * 0.53);
                    const theta = Math.random() * Math.PI * 2;
                    // Arc cosine of a uniform number, so the embers spread evenly over the
                    // sphere instead of bunching at its poles.
                    const phi = Math.acos(Math.random() * 2 - 1);
                    pos[i * 3] = r * Math.sin(phi) * Math.cos(theta);
                    pos[i * 3 + 1] = r * Math.sin(phi) * Math.sin(theta);
                    pos[i * 3 + 2] = r * Math.cos(phi);
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
                // Mapped, not bare: an unmapped point sprite is a hard square.
                const mat = new THREE.PointsMaterial({
                    map: api.helpers.glowTexture(32), color: 0xffaa00, size: R * 0.033,
                    transparent: true, opacity: 0.75, blending: THREE.AdditiveBlending,
                    depthWrite: false, sizeAttenuation: true,
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette() {
                        // The embers keep their own amber -- see above.
                    },
                    animate(ctx) {
                        points.rotation.y = ctx.time * 0.03;
                        points.rotation.x = Math.sin(ctx.time * 0.2) * 0.1;
                        mat.opacity = 0.6 + ctx.audio * 0.3 + ctx.click * 0.15;
                    },
                };
            },
        },
        /* Adapted from TithesA1s: the matter that is not currently being used. Each mote has
           a place on a shell and a place out in the room, and sits at one or the other
           depending on whether the band of presence has reached its height yet -- so the
           space around the avatar fills with the parts of it that are elsewhere.

           Lerped between two fixed points rather than simulated. What it has to say is
           "this is elsewhere", not "this is falling", and a drift with its own physics
           would wander off and never come back. */
        displacedMatter: {
            label: 'Displaced matter',
            build(api, options) {
                const COUNT = 220;
                const R = options.radius;
                const pos = new Float32Array(COUNT * 3);
                const home = new Float32Array(COUNT * 3);
                const away = new Float32Array(COUNT * 3);
                const height = new Float32Array(COUNT);
                for (let i = 0; i < COUNT; i++) {
                    const t = Math.random();
                    const y = (t * 2 - 1) * R * 0.95;
                    const radius = Math.sqrt(Math.max(0, R * R - y * y));
                    const a = Math.random() * Math.PI * 2;
                    height[i] = t;
                    home[i * 3] = Math.cos(a) * radius;
                    home[i * 3 + 1] = y;
                    home[i * 3 + 2] = Math.sin(a) * radius;
                    const out = 1.25 + Math.random() * 0.5;
                    away[i * 3] = Math.cos(a) * radius * out;
                    away[i * 3 + 1] = y + (Math.random() - 0.5) * R * 0.42;
                    away[i * 3 + 2] = Math.sin(a) * radius * out;
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(pos, 3));
                // Mapped, not bare: an unmapped point sprite is a hard square.
                const mat = new THREE.PointsMaterial({
                    map: api.helpers.glowTexture(32), color: api.palette.hex2, size: R * 0.05,
                    transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending,
                    depthWrite: false, sizeAttenuation: true,
                });
                const points = new THREE.Points(geom, mat);
                let at = 0.5;
                let dir = 1;
                return {
                    object: points,
                    applyPalette(pal) { mat.color.setHex(pal.hex2); },
                    animate(ctx) {
                        const live = ctx.state === 'THINKING' || ctx.state === 'SPEAKING';
                        const half = (ctx.state === 'SPEAKING' ? 0.42 + ctx.audio * 0.3
                            : ctx.state === 'THINKING' ? 0.09
                                : ctx.state === 'LISTENING' ? 0.16 : 0.17) + ctx.click * 0.1;
                        const speed = ctx.state === 'LISTENING' ? 0 : live ? 0.5 : 0.26;
                        at += dir * speed * 0.016;
                        if (at > 1) { at = 1; dir = -1; }
                        if (at < 0) { at = 0; dir = 1; }
                        for (let i = 0; i < COUNT; i++) {
                            const d = Math.abs(height[i] - at) / half;
                            const here = d >= 1 ? 0 : d <= 0.74 ? 1 : (1 - d) / 0.26;
                            const k = i * 3;
                            pos[k] = away[k] + (home[k] - away[k]) * here;
                            pos[k + 1] = away[k + 1] + (home[k + 1] - away[k + 1]) * here;
                            pos[k + 2] = away[k + 2] + (home[k + 2] - away[k + 2]) * here;
                        }
                        geom.attributes.position.needsUpdate = true;
                        mat.opacity = 0.45 + ctx.audio * 0.3;
                    },
                };
            },
        },
    };

    /* One place for a picker to read, so adding a part here makes it appear in the
       builder without the builder knowing anything about it. */
    function catalogue(kinds) {
        return Object.keys(kinds).map((id) => ({ id, label: kinds[id].label }));
    }

    window.AvatarParts = {
        cores: CORES,
        innerRings: INNER_RINGS,
        outerRings: OUTER_RINGS,
        effects: EFFECTS,
        band,
        options: {
            cores: () => catalogue(CORES),
            innerRings: () => catalogue(INNER_RINGS),
            outerRings: () => catalogue(OUTER_RINGS),
            effects: () => catalogue(EFFECTS),
        },
    };
})();
