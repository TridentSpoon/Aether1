/* Avatar: A.R.X.LOCAS -- the default A.R.X. face (general assistant / system admin /
 * daily operations): a small glowing core sphere held inside a fractured cube shell --
 * twenty-four crystalline shard panels, split from the six faces of a cube and pulled
 * apart along their own face normals -- that surrounds the core with a clear gap and
 * never touches it. The shell slowly tumbles as one loose formation while the core
 * counter-rotates inside it, so the two pieces always read as separate structures
 * orbiting rather than one solid object.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-locas',
    label: 'A.R.X.LOCAS',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the core sphere's own dark shell and its inner hot
        // accent, per this engine's "the core stays obsidian" / "hot accents stay fixed"
        // convention (the same one R.E.D. 9000's lens and the Nexus's eyes follow).
        const CORE_DARK = 0x0f0c1b;
        const CORE_HOT = 0x00d8ff;

        const SPHERE_RADIUS = 20;
        const CUBE_HALF = 46; // shell sits with a ~26-unit gap outside the sphere's surface
        const QUAD_SIZE = CUBE_HALF; // each face (2*CUBE_HALF wide) split into a 2x2 grid
        const SHARD_SHRINK = 0.74; // shrink each shard toward its own centre to crack the seams

        // Six cube faces as (outward normal, in-plane basis u/v, palette channel). Using
        // DoubleSide materials means winding/handedness of u/v doesn't matter visually.
        const FACES = [
            { n: new THREE.Vector3(1, 0, 0), u: new THREE.Vector3(0, 1, 0), v: new THREE.Vector3(0, 0, 1), channel: 'hex' },
            { n: new THREE.Vector3(-1, 0, 0), u: new THREE.Vector3(0, 1, 0), v: new THREE.Vector3(0, 0, -1), channel: 'hex' },
            { n: new THREE.Vector3(0, 1, 0), u: new THREE.Vector3(1, 0, 0), v: new THREE.Vector3(0, 0, -1), channel: 'hex2' },
            { n: new THREE.Vector3(0, -1, 0), u: new THREE.Vector3(1, 0, 0), v: new THREE.Vector3(0, 0, 1), channel: 'hex2' },
            { n: new THREE.Vector3(0, 0, 1), u: new THREE.Vector3(1, 0, 0), v: new THREE.Vector3(0, 1, 0), channel: 'hex3' },
            { n: new THREE.Vector3(0, 0, -1), u: new THREE.Vector3(-1, 0, 0), v: new THREE.Vector3(0, 1, 0), channel: 'hex3' },
        ];

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(160, 160, 1);
        glow.position.z = -10;
        group.add(glow);

        const coreGroup = new THREE.Group();
        group.add(coreGroup);

        // --- The spherical core: a dark obsidian shell, a faint theme-tinted wire
        // lattice over it, and a fixed hot cyan accent glowing at its centre. ---
        const sphereGroup = new THREE.Group();
        coreGroup.add(sphereGroup);

        const sphereShellMat = new THREE.MeshBasicMaterial({ color: CORE_DARK, transparent: true, opacity: 0.92 });
        const sphereShell = new THREE.Mesh(new THREE.SphereGeometry(SPHERE_RADIUS, 24, 18), sphereShellMat);
        sphereGroup.add(sphereShell);

        const sphereWireMat = new THREE.MeshBasicMaterial({ color: api.palette.hex3, wireframe: true, transparent: true, opacity: 0.3 });
        const sphereWire = new THREE.Mesh(new THREE.SphereGeometry(SPHERE_RADIUS + 0.6, 14, 10), sphereWireMat);
        sphereGroup.add(sphereWire);

        const coreHotMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending });
        const coreHot = new THREE.Mesh(new THREE.SphereGeometry(SPHERE_RADIUS * 0.32, 16, 12), coreHotMat);
        sphereGroup.add(coreHot);

        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#c8feff', '#00a8cc'), transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = SPHERE_RADIUS * 2.4;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        sphereGroup.add(coreGlow);

        // --- The fractured cube shell: 24 shard panels (six faces x four quadrants),
        // each pulled outward off its face by its own small jitter and tilted slightly
        // around its own normal, so the shell reads as broken apart rather than a solid
        // cube -- and slowly drifts in and out on its own phase, like loose debris. ---
        const shardGroup = new THREE.Group();
        coreGroup.add(shardGroup);

        const shardFillMats = { hex: [], hex2: [], hex3: [] };
        const shardEdgeMats = { hex: [], hex2: [], hex3: [] };
        const shards = [];

        FACES.forEach((face, faceIndex) => {
            [-1, 1].forEach((su) => {
                [-1, 1].forEach((sv) => {
                    const center = face.n.clone().multiplyScalar(CUBE_HALF)
                        .add(face.u.clone().multiplyScalar(su * QUAD_SIZE / 2))
                        .add(face.v.clone().multiplyScalar(sv * QUAD_SIZE / 2));

                    const jitter = 2 + Math.random() * 8;
                    const basePos = center.add(face.n.clone().multiplyScalar(jitter));

                    const size = QUAD_SIZE * SHARD_SHRINK;
                    const geom = new THREE.PlaneGeometry(size, size);
                    const fillMat = new THREE.MeshBasicMaterial({
                        color: api.palette[face.channel], side: THREE.DoubleSide, transparent: true,
                        opacity: 0.4, blending: THREE.AdditiveBlending,
                    });
                    shardFillMats[face.channel].push(fillMat);

                    const shard = new THREE.Mesh(geom, fillMat);
                    shard.quaternion.setFromUnitVectors(new THREE.Vector3(0, 0, 1), face.n);
                    shard.rotateZ((Math.random() - 0.5) * 0.5);
                    shard.position.copy(basePos);
                    shardGroup.add(shard);

                    const edgeMat = new THREE.LineBasicMaterial({ color: api.palette[face.channel], transparent: true, opacity: 0.85 });
                    shardEdgeMats[face.channel].push(edgeMat);
                    shard.add(new THREE.LineSegments(new THREE.EdgesGeometry(geom), edgeMat));

                    shards.push({
                        mesh: shard,
                        normal: face.n.clone(),
                        basePos,
                        phase: faceIndex * 1.3 + su * 0.4 + sv * 0.9 + Math.random() * 1.5,
                        driftRate: 0.5 + Math.random() * 0.4,
                        driftAmp: 2 + Math.random() * 2,
                    });
                });
            });
        });

        // --- Two broken-circle rings, tilted like a gauge ring and slowly counter-
        // rotating around the whole formation. ---
        const haloGroup = new THREE.Group();
        haloGroup.rotation.x = Math.PI / 2.4;
        group.add(haloGroup);

        function dashedRing(radius, dashSize, gapSize, material, segments = 72) {
            const pts = [];
            for (let i = 0; i <= segments; i++) {
                const a = (i / segments) * Math.PI * 2;
                pts.push(new THREE.Vector3(Math.cos(a) * radius, Math.sin(a) * radius, 0));
            }
            const geom = new THREE.BufferGeometry().setFromPoints(pts);
            const mat = material.clone();
            mat.dashSize = dashSize;
            mat.gapSize = gapSize;
            const line = new THREE.LineLoop(geom, mat);
            line.computeLineDistances();
            return line;
        }

        const outerHaloMat = new THREE.LineDashedMaterial({ color: api.palette.hex, transparent: true, opacity: 0.5 });
        const innerHaloMat = new THREE.LineDashedMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.4 });
        const outerHalo = dashedRing(78, 9, 5, outerHaloMat);
        const innerHalo = dashedRing(68, 5, 4, innerHaloMat);
        haloGroup.add(outerHalo, innerHalo);

        return {
            group, coreGroup, sphereGroup, shardGroup, shards, haloGroup, outerHalo, innerHalo,
            glowMat, sphereWireMat, coreHotMat, coreHot, coreGlowMat, coreGlow, coreGlowScale,
            shardFillMats, shardEdgeMats, outerHaloMat, innerHaloMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 0.6) * 3;

        // The shell tumbles as one loose formation; the core counter-rotates inside
        // it on a different axis speed so the two always read as separate bodies.
        model.shardGroup.rotation.y = ctx.time * 0.12;
        model.shardGroup.rotation.x = Math.sin(ctx.time * 0.18) * 0.12;
        model.sphereGroup.rotation.y = -ctx.time * 0.2;
        model.sphereGroup.rotation.x = Math.cos(ctx.time * 0.15) * 0.08;

        // A brief outward shatter-pulse on click, on top of each shard's own steady
        // drift along its face normal.
        const glitchPush = ctx.click * 6;
        model.shards.forEach((shard) => {
            const drift = Math.sin(ctx.time * shard.driftRate + shard.phase) * shard.driftAmp;
            shard.mesh.position.copy(shard.basePos).addScaledVector(shard.normal, drift + glitchPush);
        });

        model.outerHalo.rotation.z = ctx.time * 0.25;
        model.innerHalo.rotation.z = -ctx.time * 0.4;

        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 5)) : 0;
        model.sphereWireMat.opacity = 0.22 + thinkPulse * 0.3;

        const hotPulse = isSpeaking ? ctx.audio : thinkPulse * 0.6;
        model.coreHotMat.opacity = 0.75 + hotPulse * 0.25;
        model.coreGlowMat.opacity = Math.min(1, 0.4 + hotPulse * 0.5);
        const hotScale = 1 + hotPulse * 0.3 + ctx.click * 0.25;
        model.coreHot.scale.setScalar(hotScale);
        model.coreGlow.scale.set(model.coreGlowScale * hotScale, model.coreGlowScale * hotScale, 1);

        model.glowMat.opacity = 0.14 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.3) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.sphereWireMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        model.outerHaloMat.color.setHex(palette.hex);
        model.innerHaloMat.color.setHex(palette.hex3);
        ['hex', 'hex2', 'hex3'].forEach((channel) => {
            model.shardFillMats[channel].forEach((mat) => mat.color.setHex(palette[channel]));
            model.shardEdgeMats[channel].forEach((mat) => mat.color.setHex(palette[channel]));
        });
        // Core sphere shell + hot inner accent stay fixed -- this avatar's one hot
        // accent that never retints.
    },
});
