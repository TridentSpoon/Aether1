/* Avatar: A.R.X.LEGIONARE -- an inverted four-sided pyramid frame hanging apex-down,
 * split into three tiered shells -- a pointed tip, a middle band, and a wide top band
 * -- each its own translucent frustum, separated from its neighbours by a clear
 * horizontal air gap. Each tier is further split into its four flat wall panels, each
 * pulled back from its neighbours by a vertical air gap, so the four gaps run the
 * shape's full height like a cross of open slits -- the frame reads as loose plates
 * hovering around a shared taper rather than one solid mass, and the crystal core
 * shows through the slits instead of being sealed inside. A small crystalline core
 * (an icosahedron) tumbles on its own fast independent spin at the centre, lit from
 * within by a pulsing hot-red glow standing in for the reference's point light.
 *
 * Loosely inspired by faceted low-poly sci-fi companion-drone motifs in general --
 * an inverted pyramid shell around a spinning crystalline core -- not a copy of any
 * one specific design, and redrawn from scratch in this engine's own flat-shape/
 * unlit-material language (MeshBasicMaterial, no scene lighting) the same way every
 * other avatar here is hand built; see js/hologram/core.js's note that only hAlcy's
 * obsidian core is actually lit.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'arx-legionare',
    label: 'A.R.X.LEGIONARE',
    // The Umbrals: the A.R.X. line -- see registerAvatar's optional def.group in
    // js/hologram/README.md and index.html's avatar-menu / js/avatar-lab.js's BUILT_IN,
    // which group the same way.
    group: 'The Umbrals',

    build(api) {
        const group = new THREE.Group();

        // Fixed, not theme-tinted: the crystal core's hot glow, this avatar's one
        // identity colour that never retints, the same convention R.E.D. 9000's lens
        // and A.R.X.LOCAS's core accent follow.
        const CORE_HOT = 0xff3300;

        const PYRAMID_RADIUS = 40;
        const PYRAMID_HEIGHT = 78;
        const TIER_COUNT = 3;
        const TIER_FILL = 0.72; // fraction of each tier's vertical slot actually built -- the rest is air gap
        const CRYSTAL_RADIUS = 14;
        const CRYSTAL_OFFSET_Y = 5; // slightly toward the apex, before the whole thing inverts

        // --- Ambient bloom behind everything. ---
        const glowMat = new THREE.SpriteMaterial({ map: api.helpers.glowTexture(64), color: api.palette.hex, transparent: true, opacity: 0.16, blending: THREE.AdditiveBlending, depthWrite: false });
        const glow = new THREE.Sprite(glowMat);
        glow.scale.set(150, 150, 1);
        glow.position.z = -10;
        group.add(glow);

        // A static 180-degree flip so the pyramid hangs apex-down, everything below
        // built as if apex-up first (matches the reference's own build-then-invert order).
        const invertGroup = new THREE.Group();
        invertGroup.rotation.z = Math.PI;
        group.add(invertGroup);

        // The slow idle tumble; the crystal core spins independently faster, as its
        // own child transform on top of this one.
        const spinGroup = new THREE.Group();
        invertGroup.add(spinGroup);

        // Three tiered shells sliced from one ideal cone's silhouette (apex at
        // +PYRAMID_HEIGHT/2, base radius PYRAMID_RADIUS at -PYRAMID_HEIGHT/2), each
        // shrunk toward its own slot's centre so a clear horizontal air gap separates
        // it from its neighbours -- their radii still line up with that shared taper,
        // so the three pieces read as one pyramid's silhouette even with the gaps
        // carved out of it. Each tier is in turn built as four flat wall panels (one
        // per side of the four-sided pyramid) instead of one solid shell, each panel
        // shrunk toward its own face's centre the same way, so a vertical air gap
        // opens at every corner and the four gaps line up tier to tier into a
        // continuous cross of open slits down the whole shape.
        const pyramidFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.32 });
        const wireframeMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });

        const slotHeight = PYRAMID_HEIGHT / TIER_COUNT;
        const tierHeight = slotHeight * TIER_FILL;
        const radiusAt = (y) => PYRAMID_RADIUS * (PYRAMID_HEIGHT / 2 - y) / PYRAMID_HEIGHT;

        const FACE_COUNT = 4;
        const FACE_FILL = 0.74; // fraction of each face's angular slot actually built -- the rest is the vertical air gap
        const angleStep = (Math.PI * 2) / FACE_COUNT;

        // Builds one flat wall panel as a trapezoid (or, when radiusTop is 0, a
        // triangle) between two angles -- the same flat quad a low-poly cylinder's
        // side face already is, just carved out on its own so it can be shrunk and
        // gapped independently of its neighbours.
        function buildPanelGeometry(angleStart, angleEnd, radiusTop, radiusBottom, topY, bottomY) {
            const x0t = Math.cos(angleStart) * radiusTop, z0t = Math.sin(angleStart) * radiusTop;
            const x1t = Math.cos(angleEnd) * radiusTop, z1t = Math.sin(angleEnd) * radiusTop;
            const x0b = Math.cos(angleStart) * radiusBottom, z0b = Math.sin(angleStart) * radiusBottom;
            const x1b = Math.cos(angleEnd) * radiusBottom, z1b = Math.sin(angleEnd) * radiusBottom;
            const geo = new THREE.BufferGeometry();
            geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array([
                x0b, bottomY, z0b, x1b, bottomY, z1b, x1t, topY, z1t,
                x0b, bottomY, z0b, x1t, topY, z1t, x0t, topY, z0t,
            ]), 3));
            geo.computeVertexNormals();
            return geo;
        }

        for (let i = 0; i < TIER_COUNT; i++) {
            const center = -PYRAMID_HEIGHT / 2 + (i + 0.5) * slotHeight;
            const topY = center + tierHeight / 2;
            const bottomY = center - tierHeight / 2;
            const isTip = i === TIER_COUNT - 1; // the slot nearest the apex -- built as a point
            const rTop = isTip ? 0 : radiusAt(topY);
            const rBottom = radiusAt(bottomY);

            for (let f = 0; f < FACE_COUNT; f++) {
                const faceMid = f * angleStep + angleStep / 2;
                const halfSpan = (angleStep / 2) * FACE_FILL;
                const a0 = faceMid - halfSpan;
                const a1 = faceMid + halfSpan;

                const panelGeo = buildPanelGeometry(a0, a1, rTop, rBottom, topY, bottomY);
                const panelMesh = new THREE.Mesh(panelGeo, pyramidFillMat);
                spinGroup.add(panelMesh);

                const panelWire = new THREE.LineSegments(new THREE.EdgesGeometry(panelGeo), wireframeMat);
                panelWire.scale.set(1.01, 1.01, 1.01);
                spinGroup.add(panelWire);
            }
        }

        const crystalMat = new THREE.MeshBasicMaterial({ color: CORE_HOT, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending });
        const crystal = new THREE.Mesh(new THREE.IcosahedronGeometry(CRYSTAL_RADIUS, 0), crystalMat);
        crystal.position.set(0, CRYSTAL_OFFSET_Y, 0);
        spinGroup.add(crystal);

        const coreGlowMat = new THREE.SpriteMaterial({ map: api.helpers.radialGlowTexture(64, '#ffcf9e', '#ff3300'), transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false });
        const coreGlowScale = CRYSTAL_RADIUS * 3;
        const coreGlow = new THREE.Sprite(coreGlowMat);
        coreGlow.scale.set(coreGlowScale, coreGlowScale, 1);
        coreGlow.position.set(0, CRYSTAL_OFFSET_Y, 0);
        spinGroup.add(coreGlow);

        return {
            group, spinGroup, crystal, coreGlow, coreGlowScale,
            glowMat, pyramidFillMat, wireframeMat, crystalMat, coreGlowMat,
        };
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';

        model.group.position.y = Math.sin(ctx.time * 1.5) * 3;
        model.spinGroup.rotation.y = ctx.time * 0.2;

        model.crystal.rotation.x = ctx.time * 0.4;
        model.crystal.rotation.y = ctx.time * 0.6;

        // A brief glitch offset/flicker on click, plus a wireframe flash.
        const glitch = ctx.click;
        model.spinGroup.position.x = (Math.random() - 0.5) * glitch * 4;
        model.spinGroup.position.z = (Math.random() - 0.5) * glitch * 4;
        model.wireframeMat.opacity = 0.7 + glitch * 0.3;

        // A steady energy hum, boosted by speaking (audio) or a faster thinking pulse.
        const idlePulse = (Math.sin(ctx.time * 4) + 1) / 2;
        const speakPulse = isSpeaking ? ctx.audio : 0;
        const thinkPulse = isThinking ? Math.abs(Math.sin(ctx.time * 6)) : 0;
        const pulse = idlePulse * 0.4 + speakPulse * 0.5 + thinkPulse * 0.4;

        model.coreGlowMat.opacity = Math.min(1, 0.35 + pulse * 0.55);
        model.crystalMat.opacity = 0.75 + pulse * 0.25;
        const scale = 1 + pulse * 0.25 + ctx.click * 0.3;
        model.crystal.scale.setScalar(scale);
        model.coreGlow.scale.set(model.coreGlowScale * scale, model.coreGlowScale * scale, 1);

        model.glowMat.opacity = 0.14 + (isSpeaking ? ctx.audio * 0.2 : Math.sin(ctx.time * 1.3) * 0.05 + 0.05);
    },

    applyPalette(model, palette) {
        model.pyramidFillMat.color.setHex(palette.hex);
        model.wireframeMat.color.setHex(palette.hex2);
        model.glowMat.color.setHex(palette.hex);
        // The crystal core and its glow stay fixed -- this avatar's one hot accent
        // that never retints.
    },
});
