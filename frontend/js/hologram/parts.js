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
 * only current exceptions are the Nexus / Nexus Sent and A1, whose designs stay theirs
 * alone. (Note that avatar-senti.js registers under the id "senti" but its label is
 * "Nexus Sent" -- it *is* the Nexus Sent avatar under a different filename, not a
 * separate "Senti" avatar, so it falls under this same exception rather than being a
 * gap in the catalogue.) An adapted part is a fresh, simplified build of the same visual
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

    // Rejection-samples `count` points inside the union of ellipses {cx,cy,rx,ry} --
    // the same plain 2D containment math White Rabbit uses for its silhouette.
    function sampleEllipseCluster(ellipses, count, zJitter) {
        let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
        ellipses.forEach(({ cx, cy, rx, ry }) => {
            minX = Math.min(minX, cx - rx); maxX = Math.max(maxX, cx + rx);
            minY = Math.min(minY, cy - ry); maxY = Math.max(maxY, cy + ry);
        });
        const positions = new Float32Array(count * 3);
        let filled = 0, attempts = 0;
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

    // ---- Cores: the thing at the middle -------------------------------------

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
                        group.rotation.y = ctx.time * 0.4;
                        group.rotation.x = Math.sin(ctx.time * 0.3) * 0.3;
                        group.scale.setScalar(1 + ctx.audio * 0.2 + ctx.click * 0.15);
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

        // Adapted from A1ter_nul: a row of dark-glass shard bars, tallest in the
        // middle, each answering its own audio bin like a broken-glass equaliser.
        altShards: {
            label: 'Broken shard stack',
            build(api, options) {
                const group = new THREE.Group();
                const count = 7;
                const totalSpan = options.size * 3.2;
                const maxLength = options.size * 2.1;
                const minLength = options.size * 0.9;
                const shards = [];
                for (let i = 0; i < count; i++) {
                    const t = count > 1 ? i / (count - 1) : 0.5;
                    const taper = 1 - Math.pow(Math.abs(t - 0.5) * 2, 1.6);
                    const length = minLength + (maxLength - minLength) * taper;
                    const x = -totalSpan / 2 + (i / (count - 1)) * totalSpan;
                    const fillMat = new THREE.MeshBasicMaterial({
                        color: 0x0a0a0c, transparent: true, opacity: 0.55, side: THREE.DoubleSide,
                    });
                    const outlineMat = new THREE.LineBasicMaterial({
                        color: 0x00f0ff, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending,
                    });
                    const geom = new THREE.PlaneGeometry(options.size * 0.34, length);
                    const mesh = new THREE.Mesh(geom, fillMat);
                    const outline = new THREE.LineSegments(new THREE.EdgesGeometry(geom), outlineMat);
                    const shard = new THREE.Group();
                    shard.add(mesh, outline);
                    shard.position.x = x;
                    group.add(shard);
                    shards.push({ outlineMat, index: i });
                }
                return {
                    object: group,
                    applyPalette(p) {
                        const bottom = new THREE.Color(p.hex2), top = new THREE.Color(p.hex3);
                        shards.forEach(({ outlineMat, index }) => outlineMat.color.copy(bottom).lerp(top, index / (count - 1)));
                    },
                    animate(ctx) {
                        shards.forEach(({ outlineMat, index }) => {
                            const level = band(ctx, index, count);
                            outlineMat.opacity = 0.35 + level * 0.55 + ctx.click * 0.2;
                        });
                        group.rotation.y = Math.sin(ctx.time * 0.2) * 0.1;
                    },
                };
            },
        },

        // Adapted from White Rabbit: the same rejection-sampled point cloud, drawn as a
        // blank front-facing silhouette rather than a solid body -- here just the
        // rounded head/body shape, sized by options.size.
        rabbitSilhouette: {
            label: 'Rabbit silhouette',
            build(api, options) {
                const s = options.size / 20;
                const positions = sampleEllipseCluster(
                    [{ cx: 0, cy: -0.4 * s * 20, rx: 0.95 * s * 20, ry: 1.05 * s * 20 },
                     { cx: 0, cy: 0.75 * s * 20, rx: 0.7 * s * 20, ry: 0.75 * s * 20 }],
                    1600, 8 * s
                );
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                const mat = new THREE.PointsMaterial({
                    color: api.palette.hex, map: api.helpers.glowTexture(24), size: 2.2,
                    transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const points = new THREE.Points(geom, mat);
                return {
                    object: points,
                    applyPalette(p) { mat.color.setHex(p.hex); },
                    animate(ctx) {
                        const scale = 1 + Math.sin(ctx.time * 0.8) * 0.015 + ctx.audio * 0.06 + ctx.click * 0.04;
                        points.scale.setScalar(scale);
                    },
                };
            },
        },

        // Adapted from A.R.X.LOGOS: a central hexagon with a fixed dark pupil and a
        // catchlight, reading as an eye rather than a plain panel.
        hexEye: {
            label: 'Hex eye',
            build(api, options) {
                const group = new THREE.Group();
                const radius = options.size;
                const fillMat = new THREE.MeshBasicMaterial({
                    color: 0xe024c3, transparent: true, opacity: 0.22, blending: THREE.AdditiveBlending,
                });
                const outlineMat = new THREE.LineBasicMaterial({ color: 0xe024c3, transparent: true, opacity: 0.95 });
                const fill = hexFillMesh(api, radius, 0, fillMat);
                const outline = hexOutlineLoop(api, radius, 0, outlineMat);
                const pupilMat = new THREE.MeshBasicMaterial({ color: 0x050208, transparent: true, opacity: 0.92 });
                const pupil = hexFillMesh(api, radius * 0.35, 0, pupilMat);
                pupil.position.z = 0.5;
                const catchMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.5 });
                const catchlight = hexFillMesh(api, radius * 0.09, 0, catchMat, -radius * 0.16, radius * 0.16);
                catchlight.position.z = 1;
                group.add(fill, outline, pupil, catchlight);
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex); outlineMat.color.setHex(p.hex); },
                    animate(ctx) {
                        const scale = ctx.state === 'SPEAKING' ? 1 + ctx.audio * 0.5
                            : ctx.state === 'THINKING' ? 1 + Math.sin(ctx.time * 16) * 0.15 : 1 + ctx.click * 0.15;
                        fill.scale.setScalar(scale);
                        outline.scale.setScalar(scale);
                    },
                };
            },
        },

        // Adapted from A.R.X.LIMES: the small faceted anchor its plates orbit, on its
        // own here as a compact geodesic core.
        facetedHub: {
            label: 'Faceted hub',
            build(api, options) {
                const geom = new THREE.IcosahedronGeometry(options.size * 0.4, 1);
                const fillMat = new THREE.MeshBasicMaterial({
                    color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending,
                });
                const outlineMat = new THREE.LineBasicMaterial({ color: 0xff3300, transparent: true, opacity: 0.95 });
                const mesh = new THREE.Mesh(geom, fillMat);
                const outline = new THREE.LineSegments(new THREE.EdgesGeometry(geom, 12), outlineMat);
                const group = new THREE.Group();
                group.add(mesh, outline);
                return {
                    object: group,
                    applyPalette(p) { fillMat.color.setHex(p.hex2); outlineMat.color.setHex(p.hex3); },
                    animate(ctx) {
                        group.rotation.y = ctx.time * 0.25;
                        group.scale.setScalar(1 + ctx.audio * 0.15 + ctx.click * 0.12);
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
                        group.rotation.y = ctx.time * 0.2;
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
        // laid across a plane) on its own as a tumbling core, without the vertical
        // cube stack or halo rings that surround it in the original.
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
                        group.rotation.y = ctx.time * 0.3;
                        const breathe = 1 + Math.sin(ctx.time * 2) * 0.05 + ctx.audio * 0.15 + ctx.click * 0.15;
                        group.scale.setScalar(breathe);
                    },
                };
            },
        },

        // Adapted from A.R.X.LEXICO: the core cube at the centre of its cross -- a real
        // box rather than a flat isometric illusion, with a white aperture bead and a
        // radial glow sitting at the rotation origin so it only ever brightens in place.
        // Keeps LEXICO's fixed hot cyan, which does not retint with the theme.
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
                        group.rotation.y = ctx.time * 0.5;
                        group.rotation.x = Math.sin(ctx.time * 0.4) * 0.25;
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
        // ring and three accent dots mounted on the housing so they turn with it.
        // Keeps LYKSAUM's fixed hot cyan for the rim and dots.
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
                        group.rotation.y = ctx.time * (Math.PI * 2 / 14);
                        group.rotation.x = BASE_TILT + Math.sin(ctx.time * 0.3) * 0.06;
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
                        group.rotation.y = -ctx.time * 0.25;
                        group.rotation.x = Math.sin(ctx.time * 0.35) * 0.15;
                        const pulse = ctx.audio * 0.5 + ctx.click * 0.3;
                        hotMat.opacity = 0.8 + pulse * 0.2;
                        glowMat.opacity = Math.min(1, 0.45 + pulse * 0.5);
                    },
                };
            },
        },

        // Adapted from A.R.X.LEGIONARE: the crystal hanging inside its pyramid -- a bare
        // icosahedron with no wireframe over it, lit by a fixed ember glow rather than
        // the theme, spinning faster than whatever shell surrounds it so the two never
        // look welded together.
        emberCrystal: {
            label: 'Ember crystal',
            build(api, options) {
                const HOT = 0xff3300;
                const group = new THREE.Group();
                const mat = new THREE.MeshBasicMaterial({
                    color: HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
                });
                const crystal = new THREE.Mesh(new THREE.IcosahedronGeometry(options.size * 0.8, 0), mat);
                group.add(crystal);
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
                        crystal.rotation.x = ctx.time * 0.4;
                        crystal.rotation.y = ctx.time * 0.6;
                        const pulse = ctx.audio * 0.6 + ctx.click * 0.35 + (Math.sin(ctx.time * 3) + 1) * 0.12;
                        mat.opacity = 0.75 + Math.min(0.25, pulse * 0.25);
                        glowMat.opacity = Math.min(1, 0.35 + pulse * 0.55);
                    },
                };
            },
        },

        // Adapted from Operator: the prompt line at the centre of its tunnel -- a row of
        // ticks already printed rather than typing itself out, each breathing on its own
        // phase so the row shimmers like a CRT instead of pulsing in lockstep, with a
        // "|>" caret waiting at the end of the line.
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
                function tickTexture(size) {
                    const canvas = document.createElement('canvas');
                    canvas.width = size;
                    canvas.height = size;
                    const ctx2d = canvas.getContext('2d');
                    ctx2d.fillStyle = '#ffffff';
                    ctx2d.fillRect(size * 0.42, size * 0.2, size * 0.16, size * 0.6);
                    const texture = new THREE.CanvasTexture(canvas);
                    texture.needsUpdate = true;
                    return texture;
                }

                const group = new THREE.Group();
                const count = 20;
                const spacing = options.size * 0.115;
                const startX = -((count - 1) * spacing) / 2;
                const positions = new Float32Array(count * 3);
                const colors = new Float32Array(count * 3);
                for (let i = 0; i < count; i++) {
                    positions[i * 3] = startX + i * spacing;
                    positions[i * 3 + 1] = -options.size * 0.1;
                    positions[i * 3 + 2] = 0;
                }
                const geom = new THREE.BufferGeometry();
                geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                geom.setAttribute('color', new THREE.BufferAttribute(colors, 3));
                const lineMat = new THREE.PointsMaterial({
                    color: api.palette.hex3, map: tickTexture(32), size: options.size * 0.27,
                    vertexColors: true, transparent: true, opacity: 0.95, depthWrite: false,
                    blending: THREE.AdditiveBlending, sizeAttenuation: true,
                });
                const line = new THREE.Points(geom, lineMat);
                group.add(line);

                const caretX = startX + (count - 1) * spacing + spacing;
                const caretMat = new THREE.SpriteMaterial({
                    map: caretTexture(72), color: api.palette.hex,
                    transparent: true, opacity: 1, depthWrite: false, blending: THREE.AdditiveBlending,
                });
                const caret = new THREE.Sprite(caretMat);
                const caretSize = options.size * 0.6;
                caret.scale.set(caretSize, caretSize, 1);
                caret.position.set(caretX, -options.size * 0.1, 1);
                group.add(caret);

                const haloMat = new THREE.SpriteMaterial({
                    map: api.helpers.glowTexture(48), color: api.palette.hex,
                    transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const halo = new THREE.Sprite(haloMat);
                const haloSize = options.size * 0.8;
                halo.scale.set(haloSize, haloSize, 1);
                halo.position.set(caretX, -options.size * 0.1, 0);
                group.add(halo);

                return {
                    object: group,
                    applyPalette(p) {
                        lineMat.color.setHex(p.hex3);
                        caretMat.color.setHex(p.hex);
                        haloMat.color.setHex(p.hex);
                    },
                    animate(ctx) {
                        const speed = ctx.state === 'THINKING' ? 3.2 : 1.4;
                        for (let i = 0; i < count; i++) {
                            const brightness = 0.32 + (Math.sin(ctx.time * speed + i * 0.6) * 0.5 + 0.5) * 0.22;
                            colors[i * 3] = colors[i * 3 + 1] = colors[i * 3 + 2] = brightness;
                        }
                        geom.attributes.color.needsUpdate = true;
                        const caretOpacity = ctx.state === 'SPEAKING'
                            ? 0.75 + ctx.audio * 0.4
                            : 0.72 + (Math.sin(ctx.time * speed) * 0.5 + 0.5) * 0.28;
                        caretMat.opacity = caretOpacity;
                        haloMat.opacity = 0.35 + caretOpacity * 0.35;
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

        // Adapted from White Rabbit: the pair of long ears hinged at their base, which
        // mostly sit still and flick every few seconds rather than animating
        // continuously -- each ear keeps its own next-twitch time so the two never move
        // in lockstep, and an alert state pulls them upright and flicks them more often.
        rabbitEars: {
            label: 'Rabbit ears',
            build(api, options) {
                const group = new THREE.Group();
                const s = options.radius / 30;
                const mat = new THREE.PointsMaterial({
                    color: api.palette.hex3, map: api.helpers.glowTexture(20), size: 2.7,
                    transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending, depthWrite: false,
                });
                const ears = [-1, 1].map((side) => {
                    const positions = sampleEllipseCluster(
                        [{ cx: 0, cy: 19 * s, rx: 4.3 * s, ry: 19 * s }], 520, 6 * s
                    );
                    const geom = new THREE.BufferGeometry();
                    geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));
                    const pivot = new THREE.Group();
                    pivot.position.set(side * 6.5 * s, options.radius * 0.55, 0);
                    pivot.rotation.z = -side * 0.22;
                    pivot.add(new THREE.Points(geom, mat));
                    group.add(pivot);
                    return {
                        pivot, side, restAngle: -side * 0.22,
                        nextTwitch: 1.5 + Math.random() * 3, twitchStart: -10,
                    };
                });
                return {
                    object: group,
                    applyPalette(p) { mat.color.setHex(p.hex3); },
                    animate(ctx) {
                        const thinking = ctx.state === 'THINKING';
                        const lift = ctx.state === 'IDLE' ? 0 : 0.12;
                        const minGap = thinking ? 1.2 : 2.8;
                        const maxGap = thinking ? 3 : 7;
                        ears.forEach((ear) => {
                            if (ctx.time > ear.nextTwitch) {
                                ear.twitchStart = ctx.time;
                                ear.nextTwitch = ctx.time + THREE.MathUtils.lerp(minGap, maxGap, Math.random());
                            }
                            const dt = ctx.time - ear.twitchStart;
                            const twitch = dt >= 0 && dt < 0.5 ? Math.sin(dt * 24) * 0.4 * (1 - dt / 0.5) : 0;
                            const sway = Math.sin(ctx.time * 1.05 + ear.side * 1.7) * 0.02;
                            ear.pivot.rotation.z = ear.restAngle + ear.side * lift + twitch + sway;
                        });
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
    };

    // ---- Outer rings: a boundary further out ---------------------------------

    const OUTER_RINGS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
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
    };

    // ---- Effects: an ambient layer or background ------------------------------

    const EFFECTS = {
        none: {
            label: 'Nothing',
            build() {
                return { object: new THREE.Group(), applyPalette() {}, animate() {} };
            },
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
