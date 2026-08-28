// Avatar 3: The Nexus -- a Sentinel-esque ribbed head with a clustered red eye-lens array,
// a front mandible cluster, and claw-tipped trailing tentacles that hunt the cursor,
// set against falling NEXUS letter rain.

HologramAvatar.prototype.buildNexusAvatar = function() {
    this.nexusGroup = new THREE.Group();

    // --- NEXUS letter rain: real glyphs (not abstract points), falling straight down
    // and wrapping top-to-bottom. This replaces the old inward-spiral vortex entirely. ---
    const letters = ['N', 'E', 'X', 'U', 'S'];
    this.nexusRainMaterials = letters.map(ch => new THREE.SpriteMaterial({
        map: this.createLetterSpriteTexture(ch),
        color: 0x00ff66,
        transparent: true,
        opacity: 0.72, // 10% dimmer than the original 0.8
        blending: THREE.AdditiveBlending,
        depthWrite: false
    }));

    const rainCount = 90;
    const rainHalfWidth = 130;
    const rainTopY = 110;
    const rainBottomY = -110;
    // Kept well behind the creature's frontmost point (the head's front surface, ~+22) at
    // all times, so no rain drop can ever render in front of / cover the avatar. Sprites are
    // scaled up a bit to compensate for reading smaller at this greater camera distance.
    const rainMinZ = -80;
    const rainMaxZ = -30;
    for (let i = 0; i < rainCount; i++) {
        const mat = this.nexusRainMaterials[Math.floor(Math.random() * letters.length)];
        const sprite = new THREE.Sprite(mat);
        const scale = (9 + Math.random() * 7) * 1.2;
        sprite.scale.set(scale, scale, 1);
        sprite.position.set(
            (Math.random() - 0.5) * rainHalfWidth * 2,
            rainTopY + Math.random() * (rainTopY - rainBottomY),
            rainMinZ + Math.random() * (rainMaxZ - rainMinZ)
        );
        sprite.userData = {
            speed: 26 + Math.random() * 38,
            flickerPhase: Math.random() * Math.PI * 2
        };
        this.nexusRainDrops.push(sprite);
        this.nexusGroup.add(sprite);
    }

    // --- Squid/brain creature: a mantle-like head with trailing tentacles, held in its
    // own sub-group so it can rotate independently without spinning the rain field
    // along with it. Sits front-facing toward the camera (+Z) at rest. ---
    this.nexusCreatureGroup = new THREE.Group();

    // Width (X) and length (Z, the mantle's existing front-to-back elongation) grown 10%/20%
    // beyond a plain sphere; child elements below (eyes, ribs) are repositioned by the same
    // factors so they still sit on the surface instead of sinking into the bigger head. A
    // bigger head relative to the limbs reads as more "cute" (child-like proportions), not less.
    const headWidenX = 1.1;
    const headLengthenZ = 1.2;
    const headGeom = new THREE.IcosahedronGeometry(17, 1);
    headGeom.scale(headWidenX, 0.82, 1.2 * headLengthenZ);

    // Vertical gradient (dim underside fading to a bright crown) via vertex colors --
    // this multiplies with material.color, so theme tinting (see applyColorPalette in
    // core.js) still works, it just tints a gradient instead of a flat fill.
    const headPos = headGeom.attributes.position;
    const headBounds = new THREE.Box3().setFromBufferAttribute(headPos);
    const headMinY = headBounds.min.y, headMaxY = headBounds.max.y;
    const headColors = new Float32Array(headPos.count * 3);
    for (let i = 0; i < headPos.count; i++) {
        const t = (headPos.getY(i) - headMinY) / (headMaxY - headMinY); // 0 (bottom) .. 1 (top)
        const lum = 0.32 + t * 0.68;
        headColors[i * 3] = lum;
        headColors[i * 3 + 1] = lum;
        headColors[i * 3 + 2] = lum;
    }
    headGeom.setAttribute('color', new THREE.BufferAttribute(headColors, 3));

    // Shared subtle inner-glow sprite material -- reused as a child of the head, every
    // tentacle segment, and every arm joint/rod below, so all of them read as dark shapes
    // with a faint light from within rather than a bright fill.
    const glowTexture = this.createGlowSpriteTexture(32);
    this.nexusGlowMat = new THREE.SpriteMaterial({
        map: glowTexture, color: 0x00ff66, transparent: true, opacity: 0.28,
        blending: THREE.AdditiveBlending, depthWrite: false
    });

    // Solid, gradient-shaded, and normally blended (not additive). Heavily darkened (like
    // the tentacles/arms) with a single soft glow sprite at its core for that same faint
    // light-from-within look, instead of a brighter flat fill.
    const headMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, vertexColors: true, transparent: true, opacity: 0.85
    });
    this.nexusHeadMesh = new THREE.Mesh(headGeom, headMat);
    this.nexusCreatureGroup.add(this.nexusHeadMesh);

    const headGlow = new THREE.Sprite(this.nexusGlowMat);
    headGlow.scale.setScalar(16);
    headGlow.position.set(0, 2, 4);
    this.nexusHeadMesh.add(headGlow);

    // Additively blended so this facet "lattice" reads as distinctly brighter against the
    // now-darker fill, instead of just a thin line at the same brightness as the body.
    const headEdges = new THREE.EdgesGeometry(headGeom, 15);
    const headOutlineMat = new THREE.LineBasicMaterial({
        color: 0x00ffaa, transparent: true, opacity: 1.0, blending: THREE.AdditiveBlending
    });
    this.nexusHeadOutline = new THREE.LineSegments(headEdges, headOutlineMat);
    this.nexusCreatureGroup.add(this.nexusHeadOutline);

    // Ribbed carapace bands -- two thin rings encircling the head's Z axis like a
    // segmented shell, sharing the outline material so they pick up the same theme tint.
    // Children of the head mesh so they scale/pulse along with it.
    [{ z: -8, r: 12.5 }, { z: 5, r: 14.5 }].forEach(({ z, r }) => {
        const ribGeom = new THREE.TorusGeometry(r * headWidenX, 0.35, 6, 32);
        const ribEdges = new THREE.EdgesGeometry(ribGeom, 20);
        const rib = new THREE.LineSegments(ribEdges, headOutlineMat);
        rib.position.z = z * headLengthenZ;
        this.nexusHeadMesh.add(rib);
    });

    // Clustered red eye-lenses on the head's lower-front -- fixed color regardless of
    // color theme (an always-red accent, like hAlcy's obsidian core), children of the
    // head mesh so they track its gentle idle/reactive pulse automatically.
    const eyeMat = new THREE.MeshBasicMaterial({
        color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    const eyeGlowTexture = this.createGlowSpriteTexture(32);
    const eyeGlowMat = new THREE.SpriteMaterial({
        map: eyeGlowTexture, color: 0xff2418, transparent: true, opacity: 0.55,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    // Metallic bezel outline around each lens -- a sprite (always faces the camera, unlike
    // flat ring geometry) so it stays a clean circle no matter how the head is turned.
    // Shared material: animate.js spins it slowly, which reads as an animated scan rather
    // than a static cartoon outline.
    const eyeRingTexture = this.createRingSpriteTexture(64, 0.16);
    this.nexusEyeRingMat = new THREE.SpriteMaterial({
        map: eyeRingTexture, color: 0xffdca0, transparent: true, opacity: 0.8,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    // Small white anime/cartoon eye-shine, offset toward the upper-left of each lens --
    // reads as a soft, expressive highlight rather than a menacing glowing lens, making the
    // whole face less threatening. Fixed white regardless of color theme, like the lens itself.
    this.nexusEyeHighlightMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xffffff, transparent: true, opacity: 0.95,
        depthWrite: false
    });
    const eyeLayout = [
        [-7.5, -4, 15.5, 2.2], [-4.5, -6.5, 17, 1.8], [-1, -3.5, 18.5, 2.6],
        [2.5, -6, 17.5, 1.9], [6, -3.5, 16, 2.3], [8, -6.5, 14, 1.5],
        [0, -8, 15.5, 1.6], [-2.5, -1.5, 18.8, 1.4]
    ];
    eyeLayout.forEach(([rawEx, ey, rawEz, er]) => {
        const ex = rawEx * headWidenX;
        // Pushed a few units further out than a flat rawEz*headLengthenZ would land --
        // the icosahedron's facet edges (the "lattice" wireframe) bulge unevenly near this
        // cluster, and a purely proportional scale left some eyes sitting just inside that
        // bulge, so the wireframe rendered in front of / through them. This margin clears it.
        const ez = rawEz * headLengthenZ + 3.5;
        const eye = new THREE.Mesh(new THREE.SphereGeometry(er, 8, 8), eyeMat);
        eye.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(eye);

        const glow = new THREE.Sprite(eyeGlowMat);
        glow.scale.setScalar(er * 3.2);
        glow.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(glow);

        const ring = new THREE.Sprite(this.nexusEyeRingMat);
        ring.scale.setScalar(er * 2.6);
        ring.position.set(ex, ey, ez + 0.3); // nudged toward camera so it never z-fights the lens
        this.nexusHeadMesh.add(ring);

        const highlight = new THREE.Sprite(this.nexusEyeHighlightMat);
        highlight.scale.setScalar(er * 0.85);
        highlight.position.set(ex - er * 0.35, ey + er * 0.35, ez + 0.6);
        this.nexusHeadMesh.add(highlight);

        this.nexusEyes.push({ mesh: eye, glow, ring, highlight });
    });

    // Tentacles trail behind the head, away from the camera (local -Z), so the head
    // faces the viewer (+Z) and the tentacles correctly read as further back in depth.
    // Dark, solid (not wireframe/additive) spheres -- each carries the shared subtle
    // glow sprite as a child so it reads as a faint light from within rather than a bright
    // wireframe line.
    const tentacleCount = 6;
    const segmentsPerTentacle = 18; // ~2.5x the original length, same per-segment spacing
    this.nexusTentacleMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.88
    });
    // Clawed talons at each tentacle's tip -- fixed red, like the eye-lenses, so they read
    // as the same accent regardless of color theme.
    const clawMat = new THREE.MeshBasicMaterial({
        color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    for (let t = 0; t < tentacleCount; t++) {
        const baseAngle = (t / tentacleCount) * Math.PI * 2;
        const segments = [];
        for (let s = 0; s < segmentsPerTentacle; s++) {
            const size = 4.5 * (1 - s / segmentsPerTentacle) + 1;
            const geom = new THREE.SphereGeometry(size, 8, 8);
            const seg = new THREE.Mesh(geom, this.nexusTentacleMat);
            const glow = new THREE.Sprite(this.nexusGlowMat);
            glow.scale.setScalar(size * 1.5);
            seg.add(glow);
            this.nexusCreatureGroup.add(seg);
            segments.push(seg);
        }
        const claws = [];
        for (let c = 0; c < 3; c++) {
            const claw = new THREE.Mesh(new THREE.ConeGeometry(0.7, 3.2, 6), clawMat);
            this.nexusCreatureGroup.add(claw);
            claws.push(claw);
        }
        this.nexusTentacles.push({
            segments,
            claws,
            baseAngle,
            spreadRadius: 9,
            // Per-tentacle speed/phase variance so they writhe independently rather
            // than moving as one synchronized wave.
            speedMult: 0.8 + Math.random() * 0.5,
            phaseSeed: Math.random() * Math.PI * 2
        });
    }

    // Front mandible/arm cluster -- short, segmented mechanical arms framing the head from
    // below-front, like a Sentinel's grasping cluster. Solid ball joints connected by rigid
    // rod links (rescaled/oriented to span each pair every frame) rather than the tentacles'
    // sphere-chain shape, so these still read as jointed arms, not more tentacles. Same
    // heavily-darkened-fill-plus-subtle-glow treatment as the tentacles/head, on top of the
    // dark edge outline (nexusLegOutlineMat) that gives the joints/rods their silhouette.
    const legCount = 7;
    const jointsPerLeg = 3; // three rods max: anchor->j0->j1->j2
    this.nexusLegMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.95
    });
    this.nexusLegRodMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.9
    });
    this.nexusLegOutlineMat = new THREE.LineBasicMaterial({
        color: 0x0a1a10, transparent: true, opacity: 0.85
    });
    // The tip joint (the globe at the end of each mandible) gets a fixed lime core instead
    // of the shared dark fill, with its own faint glow sprite -- reads as a distinct
    // light-up tip rather than just another dark bead.
    this.nexusMandibleTipMat = new THREE.MeshBasicMaterial({
        color: 0xaaff33, transparent: true, opacity: 0.95
    });
    this.nexusMandibleTipGlowMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xaaff33, transparent: true, opacity: 0.35,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    for (let l = 0; l < legCount; l++) {
        const spread = legCount === 1 ? 0 : (l / (legCount - 1) - 0.5) * 2; // -1..1 across the fan
        const joints = [];
        for (let s = 0; s < jointsPerLeg; s++) {
            const isTip = s === jointsPerLeg - 1;
            const size = 1.7 * (1 - s / jointsPerLeg) + 0.7;
            const geom = new THREE.SphereGeometry(size, 6, 6);
            const joint = new THREE.Mesh(geom, isTip ? this.nexusMandibleTipMat : this.nexusLegMat);
            const jointOutline = new THREE.LineSegments(new THREE.EdgesGeometry(geom, 1), this.nexusLegOutlineMat);
            joint.add(jointOutline);
            const jointGlow = new THREE.Sprite(isTip ? this.nexusMandibleTipGlowMat : this.nexusGlowMat);
            jointGlow.scale.setScalar(size * (isTip ? 1.8 : 1.5));
            joint.add(jointGlow);
            this.nexusCreatureGroup.add(joint);
            joints.push(joint);
        }
        // One rod per gap: anchor->joint0, joint0->joint1, ... -- unit-height cylinders
        // rescaled and rotated to span their two endpoints every frame. The outline is a
        // child of the rod mesh, so it inherits that same per-frame transform for free. No
        // glow sprite here (unlike the joints) -- a child sprite would inherit the rod's
        // per-frame non-uniform scale and stretch into a smear instead of staying a soft dot.
        const rods = [];
        for (let r = 0; r < jointsPerLeg; r++) {
            const rodGeom = new THREE.CylinderGeometry(0.45, 0.6, 1, 6);
            const rod = new THREE.Mesh(rodGeom, this.nexusLegRodMat);
            const rodOutline = new THREE.LineSegments(new THREE.EdgesGeometry(rodGeom, 1), this.nexusLegOutlineMat);
            rod.add(rodOutline);
            this.nexusCreatureGroup.add(rod);
            rods.push(rod);
        }
        this.nexusLegs.push({
            joints,
            rods,
            spread,
            speedMult: 0.8 + Math.random() * 0.4,
            phaseSeed: Math.random() * Math.PI * 2
        });
    }

    this.nexusGroup.add(this.nexusCreatureGroup);
    this.scene.add(this.nexusGroup);
};
