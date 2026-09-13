/* Avatar: A.R.X.LEGIONARE -- an inverted four-sided pyramid frame hanging apex-down,
 * split into three tiered shells -- a pointed tip, a middle band, and a wide top band
 * -- each its own translucent frustum with a bright wireframe overlay, separated from
 * its neighbours by a clear air gap, so the frame reads as three plates stacked around
 * a shared taper rather than one solid mass. A small crystalline core (an icosahedron)
 * tumbles on its own fast independent spin inside the middle band, lit from within by
 * a pulsing hot-red glow standing in for the reference's point light.
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
        // shrunk toward its own slot's centre so a clear air gap separates it from its
        // neighbours -- their radii still line up with that shared taper, so the three
        // pieces read as one pyramid's silhouette even with the gaps carved out of it.
        const pyramidFillMat = new THREE.MeshBasicMaterial({ color: api.palette.hex, side: THREE.DoubleSide, transparent: true, opacity: 0.32 });
        const wireframeMat = new THREE.LineBasicMaterial({ color: api.palette.hex2, transparent: true, opacity: 0.85 });

        const slotHeight = PYRAMID_HEIGHT / TIER_COUNT;
        const tierHeight = slotHeight * TIER_FILL;
        const radiusAt = (y) => PYRAMID_RADIUS * (PYRAMID_HEIGHT / 2 - y) / PYRAMID_HEIGHT;

        for (let i = 0; i < TIER_COUNT; i++) {
            const center = -PYRAMID_HEIGHT / 2 + (i + 0.5) * slotHeight;
            const topY = center + tierHeight / 2;
            const bottomY = center - tierHeight / 2;
            const isTip = i === TIER_COUNT - 1; // the slot nearest the apex -- built as a point

            const tierGeo = isTip
                ? new THREE.ConeGeometry(radiusAt(bottomY), tierHeight, 4)
                : new THREE.CylinderGeometry(radiusAt(topY), radiusAt(bottomY), tierHeight, 4);

            const tierMesh = new THREE.Mesh(tierGeo, pyramidFillMat);
            tierMesh.position.y = center;
            spinGroup.add(tierMesh);

            const tierWire = new THREE.LineSegments(new THREE.EdgesGeometry(tierGeo), wireframeMat);
            tierWire.scale.set(1.01, 1.01, 1.01);
            tierWire.position.y = center;
            spinGroup.add(tierWire);
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
