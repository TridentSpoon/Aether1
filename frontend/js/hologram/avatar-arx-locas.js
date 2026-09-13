/* Avatar: A.R.X.LOCAS -- the default A.R.X. face (general assistant / system admin /
 * daily operations), built as a flat instrument panel rather than a floating creature:
 * an ornate filigreed shell -- gold corner plates, a pair of glowing lens-shaped
 * fragments top and bottom, two diamond facets left and right -- wrapped around a
 * central lens-eye (a stacked bezel of rings around a dark aperture with a glowing
 * iris and a bright pupil), with two large broken-circle rings slowly counter-
 * rotating around the whole thing like an old camera's aperture gauge.
 *
 * Loosely inspired by ornate sci-fi "cephalon"-style companion-eye panels in general --
 * not a copy of any one specific design -- and redrawn from scratch in this engine's own
 * flat-shape/dashed-line language, the same way every other avatar here is hand built.
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

        // Fixed, not theme-tinted: the panel's own dark shell and the eye's obsidian
        // aperture housing, per this engine's "the core stays obsidian" convention.
        const SHELL_DARK = 0x0f0c1b;
        const APERTURE_DARK = 0x07050f;
        // The eye's iris/pupil stay a fixed bright cyan regardless of theme -- this
        // avatar's one "hot accent that doesn't retint", the same convention R.E.D.
        // 9000's lens and the Nexus's eyes follow.
        const IRIS_HOT = 0x00d8ff;

        // SVG-style layout mapped into this engine's units: (100,100) is the panel's own
        // centre, y flips (SVG grows downward, this engine grows upward), and everything
        // is scaled up from a 200x200 sketch to a size that reads at this camera's distance.
        const SCALE = 0.72;
        const mx = (x) => (x - 100) * SCALE;
        const my = (y) => -(y - 100) * SCALE;

        function flatShape(points2D, material) {
            const shape = new THREE.Shape();
            points2D.forEach(([x, y], i) => {
                const px = mx(x), py = my(y);
                if (i === 0) shape.moveTo(px, py); else shape.lineTo(px, py);
            });
            shape.closePath();
            return new THREE.Mesh(new THREE.ShapeGeometry(shape), material);
        }

        function outlineFor(points2D, material) {
            const verts = points2D.map(([x, y]) => new THREE.Vector3(mx(x), my(y), 0.2));
            return new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints(verts), material);
        }

        function dashedCircle(radius, dashSize, gapSize, material, segments = 96) {
            const pts = [];
            for (let i = 0; i <= segments; i++) {
                const a = (i / segments) * Math.PI * 2;
                pts.push(new THREE.Vector3(Math.cos(a) * radius, Math.sin(a) * radius, 0));
            }
            const geom = new THREE.BufferGeometry().setFromPoints(pts);
            const dashMat = material.clone();
            dashMat.dashSize = dashSize;
            dashMat.gapSize = gapSize;
            const line = new THREE.LineLoop(geom, dashMat);
            line.computeLineDistances();
            return line;
        }

        // --- Shared materials -----------------------------------------------------
        const shellFillMat = new THREE.MeshBasicMaterial({ color: SHELL_DARK, side: THREE.DoubleSide, transparent: true, opacity: 0.85 });
        const plateFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending });
        const fragmentFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex3, side: THREE.DoubleSide, transparent: true, opacity: 0.8, blending: THREE.AdditiveBlending });
        const panelOutlineMat = new THREE.LineBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.85 });
        const facetOutlineHexMat = new THREE.LineBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.9 });
        const facetOutlineHex3Mat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.9 });
        const ringHexMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending });
        const dashedHex3LineMat = new THREE.LineDashedMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.85 });
        const apertureFillMat = new THREE.MeshBasicMaterial({ color: APERTURE_DARK, transparent: true, opacity: 0.95 });
        const apertureOutlineMat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.9 });
        const irisMat = new THREE.MeshBasicMaterial({ color: IRIS_HOT, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending });
        const pupilMat = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 1 });
        const crosshairHexMat = new THREE.LineBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.9 });
        const crosshairHex3Mat = new THREE.LineBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.9 });
        const nodeHexMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, transparent: true, opacity: 0.9 });
        const nodeHex3Mat = new THREE.MeshBasicMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.9 });
        const outerHaloMat = new THREE.LineDashedMaterial({ color: api.palette.hex, transparent: true, opacity: 0.55 });
        const innerHaloMat = new THREE.LineDashedMaterial({ color: api.palette.hex3, transparent: true, opacity: 0.5 });
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.22, blending: THREE.AdditiveBlending, depthWrite: false });
        const irisGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#c8feff', '#00a8cc'), transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending, depthWrite: false });

        const coreGroup = new THREE.Group();
        group.add(coreGroup);

        // Ambient bloom behind everything.
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(150, 150, 1);
        glow.position.z = -20;
        coreGroup.add(glow);

        // --- Base panel: a squared shell the rest of the filigree mounts to. ---
        const panelPts = [[35, 35], [165, 35], [165, 165], [35, 165]];
        const panel = flatShape(panelPts, shellFillMat);
        coreGroup.add(panel);
        coreGroup.add(outlineFor(panelPts, panelOutlineMat));

        // --- Corner filigree plates. ---
        [
            [[35, 55], [55, 35], [75, 35], [35, 75]],
            [[165, 55], [145, 35], [125, 35], [165, 75]],
            [[35, 145], [55, 165], [75, 165], [35, 125]],
            [[165, 145], [145, 165], [125, 165], [165, 125]],
        ].forEach((pts) => coreGroup.add(flatShape(pts, plateFillMat)));

        // --- Glowing fragment segments, top and bottom. ---
        [
            [[65, 45], [135, 45], [120, 60], [80, 60]],
            [[65, 155], [135, 155], [120, 140], [80, 140]],
        ].forEach((pts) => coreGroup.add(flatShape(pts, fragmentFillMat)));

        // --- Diamond facets, left and right. ---
        const leftFacetPts = [[45, 65], [60, 80], [60, 120], [45, 135]];
        const rightFacetPts = [[155, 65], [140, 80], [140, 120], [155, 135]];
        coreGroup.add(flatShape(leftFacetPts, shellFillMat));
        coreGroup.add(outlineFor(leftFacetPts, facetOutlineHex3Mat));
        coreGroup.add(flatShape(rightFacetPts, shellFillMat));
        coreGroup.add(outlineFor(rightFacetPts, facetOutlineHexMat));

        // --- Central lens-eye: a stacked bezel of rings around a dark aperture. ---
        const eyeGroup = new THREE.Group();
        eyeGroup.position.z = 0.4;
        coreGroup.add(eyeGroup);

        const outerRingGeom = new THREE.RingGeometry(33 * SCALE, 35 * SCALE, 48);
        const outerRing = new THREE.Mesh(outerRingGeom, ringHexMat);
        eyeGroup.add(outerRing);

        const dashedRing = dashedCircle(28 * SCALE, 3.5, 2, dashedHex3LineMat);
        eyeGroup.add(dashedRing);

        const apertureGeom = new THREE.CircleGeometry(20 * SCALE, 32);
        const aperture = new THREE.Mesh(apertureGeom, apertureFillMat);
        aperture.position.z = 0.1;
        eyeGroup.add(aperture);
        const apertureOutline = new THREE.LineLoop(new THREE.EdgesGeometry(apertureGeom), apertureOutlineMat);
        apertureOutline.position.z = 0.12;
        eyeGroup.add(apertureOutline);

        const irisGlow = new THREE.Sprite(irisGlowMat);
        irisGlow.scale.set(34, 34, 1);
        irisGlow.position.z = 0.15;
        eyeGroup.add(irisGlow);

        const iris = new THREE.Mesh(new THREE.CircleGeometry(12 * SCALE, 32), irisMat);
        iris.position.z = 0.2;
        eyeGroup.add(iris);

        const pupil = new THREE.Mesh(new THREE.CircleGeometry(5 * SCALE, 24), pupilMat);
        pupil.position.z = 0.25;
        eyeGroup.add(pupil);

        // Crosshair ticks: vertical (top/bottom) follow the main colour, horizontal
        // (left/right) follow the highlight -- the same two-tone split the filigree uses.
        function tick(x1, y1, x2, y2, material) {
            const geom = new THREE.BufferGeometry().setFromPoints([
                new THREE.Vector3(mx(x1), my(y1), 0.3), new THREE.Vector3(mx(x2), my(y2), 0.3),
            ]);
            return new THREE.Line(geom, material);
        }
        eyeGroup.add(tick(100, 60, 100, 72, crosshairHexMat));
        eyeGroup.add(tick(100, 128, 100, 140, crosshairHexMat));
        eyeGroup.add(tick(60, 100, 72, 100, crosshairHex3Mat));
        eyeGroup.add(tick(128, 100, 140, 100, crosshairHex3Mat));

        // --- Micro nodes at the compass points, just outside the panel's edge. ---
        [
            [100, 20, nodeHexMat], [100, 180, nodeHexMat],
            [20, 100, nodeHex3Mat], [180, 100, nodeHex3Mat],
        ].forEach(([x, y, mat]) => {
            const node = new THREE.Mesh(new THREE.CircleGeometry(2.5 * SCALE, 12), mat);
            node.position.set(mx(x), my(y), 0.2);
            coreGroup.add(node);
        });

        // --- Two large broken-circle rings, counter-rotating slowly around the whole
        // panel -- an aperture-gauge flourish, kept as their own group so animate() can
        // spin them independently of the panel's gentle hover. ---
        const haloGroup = new THREE.Group();
        group.add(haloGroup);
        const outerHalo = dashedCircle(64, 10, 5, outerHaloMat, 64);
        const innerHalo = dashedCircle(56, 6, 4, innerHaloMat, 64);
        haloGroup.add(outerHalo, innerHalo);

        // --- A soft scanline sweeping the panel top to bottom, looping forever. ---
        const scanlineMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(48), color: api.palette.hex3, transparent: true, opacity: 0.18, blending: THREE.AdditiveBlending, depthWrite: false });
        const scanline = new THREE.Sprite(scanlineMat);
        scanline.scale.set(95, 10, 1);
        scanline.position.z = 0.35;
        coreGroup.add(scanline);
        const panelHalfHeight = 65 * SCALE;

        return {
            group, coreGroup, haloGroup, outerHalo, innerHalo, scanline, panelHalfHeight,
            iris, pupil, irisGlow, glowMat, irisMat, irisGlowMat, pupilMat, apertureOutlineMat, dashedHex3LineMat,
            plateFillMat, fragmentFillMat, panelOutlineMat, facetOutlineHexMat, facetOutlineHex3Mat,
            ringHexMat, crosshairHexMat, crosshairHex3Mat, nodeHexMat, nodeHex3Mat,
            outerHaloMat, innerHaloMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        // A gentle hover -- this avatar reads as a mounted instrument panel, not a
        // creature, so it bobs and tilts slightly rather than orbiting the camera.
        model.group.position.y = Math.sin(ctx.time * 0.7) * 3;
        model.coreGroup.rotation.x = Math.sin(ctx.time * 0.3) * 0.05;
        model.coreGroup.rotation.y = Math.cos(ctx.time * 0.24) * 0.06;

        // A brief glitch offset/flicker on click, echoing the reference's own occasional
        // glitch-flicker keyframe.
        const glitch = ctx.click;
        model.coreGroup.position.x = (Math.random() - 0.5) * glitch * 3;
        model.coreGroup.position.y = (Math.random() - 0.5) * glitch * 3;

        model.outerHalo.rotation.z = ctx.time * 0.3;
        model.innerHalo.rotation.z = -ctx.time * 0.55;

        const scanT = (ctx.time * 12) % (model.panelHalfHeight * 2);
        model.scanline.position.y = scanT - model.panelHalfHeight;

        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 5)) : 0;
        model.dashedHex3LineMat.opacity = 0.55 + thinkPulse * 0.35;

        const irisPulse = isSpeaking ? ctx.audio : thinkPulse * 0.6;
        model.irisMat.opacity = 0.8 + irisPulse * 0.2;
        model.irisGlowMat.opacity = Math.min(1, 0.45 + irisPulse * 0.5);
        const irisScale = 1 + irisPulse * 0.25 + ctx.click * 0.2;
        model.iris.scale.setScalar(irisScale);
        model.pupil.scale.setScalar(1 + ctx.click * 0.15);
        model.irisGlow.scale.set(34 * irisScale, 34 * irisScale, 1);
        model.glowMat.opacity = 0.18 + (isSpeaking ? ctx.audio * 0.25 : Math.sin(ctx.time * 1.4) * 0.06 + 0.06);
    },

    applyPalette(model, palette) {
        model.plateFillMat.color.setHex(palette.hex);
        model.fragmentFillMat.color.setHex(palette.hex3);
        model.panelOutlineMat.color.setHex(palette.hex);
        model.facetOutlineHexMat.color.setHex(palette.hex);
        model.facetOutlineHex3Mat.color.setHex(palette.hex3);
        model.ringHexMat.color.setHex(palette.hex);
        model.dashedHex3LineMat.color.setHex(palette.hex3);
        model.apertureOutlineMat.color.setHex(palette.hex3);
        model.crosshairHexMat.color.setHex(palette.hex);
        model.crosshairHex3Mat.color.setHex(palette.hex3);
        model.nodeHexMat.color.setHex(palette.hex);
        model.nodeHex3Mat.color.setHex(palette.hex3);
        model.outerHaloMat.color.setHex(palette.hex);
        model.innerHaloMat.color.setHex(palette.hex3);
        model.glowMat.color.setHex(palette.hex);
        // Iris/pupil/aperture stay fixed -- this avatar's one hot accent that never retints.
    },
});
