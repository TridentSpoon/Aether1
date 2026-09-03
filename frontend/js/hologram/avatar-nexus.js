// Avatar 3: The Nexus Crew (v3) -- an elongated, bulbous cephalopod hull tapering into a
// forward sensor-node cluster, rendered as a semi-transparent wireframe/point-cloud "live
// sensor feed" (not a solid textured mesh), with 10 long chain-link tentacles trailing
// behind. Idle, the tentacles drift in slow jellyfish-like waves; alert (thinking, speaking,
// or actively tracking the cursor) they spread outward with sharp angular bends, pointing
// forward toward whatever they're locked onto. Set against a falling NEXUS letter rain and
// a faint radar-grid backdrop, with CRT scanline/flicker dressing applied in CSS (see
// core.js's setAvatar and A1theme.css/sprite.css's #hologram-viewport.crt-active::after).

HologramAvatar.prototype.buildNexusAvatar = function() {
    this.nexusGroup = new THREE.Group();

    // --- Radar-grid backdrop: a single large, dim, additive sprite well behind the rain,
    // reinforcing the "dark, translucent grid" CRT-radar look the rain alone doesn't give. ---
    this.nexusGridMat = new THREE.SpriteMaterial({
        map: this.createGridSpriteTexture(256, 10),
        color: 0x00ff66, transparent: true, opacity: 0.1,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    const gridSprite = new THREE.Sprite(this.nexusGridMat);
    gridSprite.scale.set(340, 340, 1);
    gridSprite.position.set(0, 0, -150);
    this.nexusGroup.add(gridSprite);

    // --- NEXUS letter rain: real glyphs (not abstract points), falling straight down
    // and wrapping top-to-bottom. ---
    const letters = ['N', 'E', 'X', 'U', 'S'];
    this.nexusRainMaterials = letters.map(ch => new THREE.SpriteMaterial({
        map: this.createLetterSpriteTexture(ch),
        color: 0x00ff66,
        transparent: true,
        opacity: 0.72,
        blending: THREE.AdditiveBlending,
        depthWrite: false
    }));

    const rainCount = 220;
    const rainHalfWidth = 130;
    const rainTopY = 110;
    const rainBottomY = -110;
    const rainMinZ = -80;
    const rainMaxZ = -30;
    for (let i = 0; i < rainCount; i++) {
        const mat = this.nexusRainMaterials[Math.floor(Math.random() * letters.length)];
        const sprite = new THREE.Sprite(mat);
        const scale = (6 + Math.random() * 5) * 1.1;
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

    // --- The Crew's hull + tentacles, held in its own sub-group so it can rotate
    // independently without spinning the rain/grid backdrop along with it. Faces the
    // camera (+Z) at rest; tentacles trail toward -Z. ---
    this.nexusCreatureGroup = new THREE.Group();

    // A fixed local tilt, nested one level inside nexusCreatureGroup and never touched by
    // animate.js's per-frame look-around (which sets nexusCreatureGroup's own rotation
    // directly). Without this, the hull's whole elongation axis lines up exactly with the
    // camera's own view axis (both Z) and the "elongated, bulbous, narrowing at the nose"
    // shape is invisible -- it reads as plain concentric rings instead of a torpedo profile.
    // Tilting the body a few tens of degrees off-axis puts that profile back in view while
    // the creature still reads as "facing" the camera/cursor.
    this.nexusRestTiltGroup = new THREE.Group();
    this.nexusRestTiltGroup.rotation.set(0.18, 0.55, 0.06);
    this.nexusCreatureGroup.add(this.nexusRestTiltGroup);

    // Elongated, bulbous chassis: a lathe-revolved profile (radius, height-along-axis)
    // gives a smooth torpedo/mantle hull that a warped sphere can't -- wide through the
    // mid-body, narrowing at both ends, tapering to a near-point at the nose. Built with
    // height along the profile's own Y, then rotated so that axis becomes Z (forward),
    // matching every other avatar's front-facing (+Z) convention.
    const hullProfile = [
        [0.8, -24], [6, -19], [10, -11], [12.5, -2], [13.5, 5],
        [12, 11], [8.5, 16], [4.5, 19.5], [1.2, 22]
    ].map(([r, z]) => new THREE.Vector2(r, z));
    const hullGeom = new THREE.LatheGeometry(hullProfile, 24);
    hullGeom.rotateX(Math.PI / 2); // profile height -> Z (front/back), matches dirX/dirY/Z convention below

    // Vertical gradient (dim underside fading to a bright crown) via vertex colors --
    // multiplies with material.color, so theme tinting (see applyColorPalette in core.js)
    // still tints a gradient instead of a flat fill.
    const hullPos = hullGeom.attributes.position;
    const hullBounds = new THREE.Box3().setFromBufferAttribute(hullPos);
    const hullMinY = hullBounds.min.y, hullMaxY = hullBounds.max.y;
    const hullColors = new Float32Array(hullPos.count * 3);
    for (let i = 0; i < hullPos.count; i++) {
        const t = (hullPos.getY(i) - hullMinY) / (hullMaxY - hullMinY || 1);
        const lum = 0.32 + t * 0.68;
        hullColors[i * 3] = lum;
        hullColors[i * 3 + 1] = lum;
        hullColors[i * 3 + 2] = lum;
    }
    hullGeom.setAttribute('color', new THREE.BufferAttribute(hullColors, 3));

    // Wireframe / point-cloud rendering, not a solid textured mesh:
    //  - a near-invisible fill, just enough to give the point cloud something to sit "on"
    //  - a dense inner wireframe (every triangle edge) at low opacity -- the "inner
    //    structural lines"
    //  - a sparser outer edge set (WireframeGeometry limited to edges past a wide angle
    //    threshold reads as the hull's structural ribs/silhouette) at high, additive
    //    opacity -- brighter than the inner lines, exactly the depth cue asked for
    //  - a sparse point cloud sampled at every vertex, additive and brighter still, so
    //    nodes/joints read as the brightest thing on the hull
    this.nexusHullFillMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, vertexColors: true, transparent: true, opacity: 0.05, depthWrite: false
    });
    this.nexusHeadMesh = new THREE.Mesh(hullGeom, this.nexusHullFillMat);
    this.nexusRestTiltGroup.add(this.nexusHeadMesh);

    this.nexusHullInnerWireMat = new THREE.LineBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.22
    });
    this.nexusHeadInnerWire = new THREE.LineSegments(new THREE.WireframeGeometry(hullGeom), this.nexusHullInnerWireMat);
    this.nexusRestTiltGroup.add(this.nexusHeadInnerWire);

    this.nexusHeadOutlineMat = new THREE.LineBasicMaterial({
        color: 0x00ffaa, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending
    });
    this.nexusHeadOutline = new THREE.LineSegments(new THREE.EdgesGeometry(hullGeom, 30), this.nexusHeadOutlineMat);
    this.nexusRestTiltGroup.add(this.nexusHeadOutline);

    this.nexusHullPointsMat = new THREE.PointsMaterial({
        color: 0x00ffaa, map: this.createGlowSpriteTexture(24), size: 2.6,
        transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending,
        depthWrite: false, sizeAttenuation: true
    });
    this.nexusHeadPoints = new THREE.Points(hullGeom, this.nexusHullPointsMat);
    this.nexusRestTiltGroup.add(this.nexusHeadPoints);

    // Ribbed carapace bands -- three thin wireframe rings encircling the hull like a
    // segmented armored shell, radii approximating the hull's own profile at each Z so
    // they sit flush on the surface instead of floating inside/outside it.
    this.nexusRibMat = new THREE.LineBasicMaterial({
        color: 0x00ffaa, transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending
    });
    [{ z: -10, r: 11.7 }, { z: 0, r: 13.2 }, { z: 9, r: 12.2 }].forEach(({ z, r }) => {
        const ribGeom = new THREE.TorusGeometry(r, 0.3, 6, 32);
        const rib = new THREE.LineSegments(new THREE.EdgesGeometry(ribGeom, 20), this.nexusRibMat);
        rib.position.z = z;
        this.nexusHeadMesh.add(rib); // child of the hull so it pulses/scales along with it
    });

    // Forward sensor-node array -- the hull's nose narrows into this tight cluster of
    // small glowing nodes instead of a single "face". Fixed phosphor red/orange
    // regardless of color theme, like every avatar's always-red accent (hAlcy's core,
    // R.E.D.'s eye, etc.) -- reused eye-ring/highlight sprite pattern from the previous
    // build, just relocated to the new hull's nose.
    const sensorMat = new THREE.MeshBasicMaterial({
        color: 0xff5522, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    const sensorGlowMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xff5522, transparent: true, opacity: 0.55,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.nexusEyeRingMat = new THREE.SpriteMaterial({
        map: this.createRingSpriteTexture(64, 0.16), color: 0xffcda0, transparent: true, opacity: 0.8,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.nexusEyeHighlightMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xffffff, transparent: true, opacity: 0.95,
        depthWrite: false
    });
    const sensorCount = 6;
    const sensorRingZ = 17.5;
    const sensorRingR = 5;
    for (let i = 0; i < sensorCount; i++) {
        const angle = (i / sensorCount) * Math.PI * 2;
        const ex = Math.cos(angle) * sensorRingR;
        const ey = Math.sin(angle) * sensorRingR * 0.75;
        const ez = sensorRingZ + Math.sin(angle * 2) * 0.6;
        const er = 1.3;

        const node = new THREE.Mesh(new THREE.SphereGeometry(er, 8, 8), sensorMat);
        node.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(node); // child of the hull so the cluster pulses/scales with it

        const glow = new THREE.Sprite(sensorGlowMat);
        glow.scale.setScalar(er * 3.4);
        glow.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(glow);

        const ring = new THREE.Sprite(this.nexusEyeRingMat);
        ring.scale.setScalar(er * 2.8);
        ring.position.set(ex, ey, ez + 0.3);
        this.nexusHeadMesh.add(ring);

        const highlight = new THREE.Sprite(this.nexusEyeHighlightMat);
        highlight.scale.setScalar(er * 0.85);
        highlight.position.set(ex - er * 0.35, ey + er * 0.35, ez + 0.6);
        this.nexusHeadMesh.add(highlight);

        this.nexusEyes.push({ mesh: node, glow, ring, highlight });
    }
    // A single forward-facing sensor right at the tip, anchoring the cluster.
    const noseNode = new THREE.Mesh(new THREE.SphereGeometry(1.6, 8, 8), sensorMat);
    noseNode.position.set(0, 0, 21.5);
    this.nexusHeadMesh.add(noseNode);
    const noseGlow = new THREE.Sprite(sensorGlowMat);
    noseGlow.scale.setScalar(6);
    noseGlow.position.set(0, 0, 21.5);
    this.nexusHeadMesh.add(noseGlow);
    this.nexusEyes.push({ mesh: noseNode, glow: noseGlow, ring: null, highlight: null });

    // --- Tentacles: 10 long, segmented, chain-like tendrils trailing from the hull's
    // tail. Each is a shared BufferGeometry (its positions rewritten every frame by
    // animateNexus) driving both a dashed Line (the dash pattern itself reads as chain
    // links) and a Points cloud of glowing joint markers -- brighter than the connecting
    // line, the same "nodes brighter than structure lines" depth cue as the hull. ---
    const tentacleCount = 10;
    const segmentsPerTentacle = 14;
    this.nexusTentacleLineMat = new THREE.LineDashedMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.6, dashSize: 2.4, gapSize: 1.3
    });
    this.nexusTentaclePointMat = new THREE.PointsMaterial({
        color: 0x00ffaa, map: this.createGlowSpriteTexture(24), size: 3.2,
        transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        depthWrite: false, sizeAttenuation: true
    });
    // Pincer tips -- three wireframe prongs per tentacle, fixed phosphor accent like the
    // sensor cluster, so every "always lit" part of the Crew reads as one accent color.
    const clawMat = new THREE.LineBasicMaterial({
        color: 0xff5522, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    const clawGlowMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(24), color: 0xff5522, transparent: true, opacity: 0.4,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    for (let t = 0; t < tentacleCount; t++) {
        const baseAngle = (t / tentacleCount) * Math.PI * 2;
        const geom = new THREE.BufferGeometry();
        const positions = new Float32Array(segmentsPerTentacle * 3);
        geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));

        const line = new THREE.Line(geom, this.nexusTentacleLineMat);
        this.nexusRestTiltGroup.add(line);
        const points = new THREE.Points(geom, this.nexusTentaclePointMat);
        this.nexusRestTiltGroup.add(points);

        const claws = [];
        for (let c = 0; c < 3; c++) {
            const clawGeom = new THREE.ConeGeometry(0.75, 3.4, 6);
            const claw = new THREE.LineSegments(new THREE.EdgesGeometry(clawGeom, 1), clawMat);
            const clawGlow = new THREE.Sprite(clawGlowMat);
            clawGlow.scale.setScalar(2.4);
            claw.add(clawGlow);
            this.nexusRestTiltGroup.add(claw);
            claws.push(claw);
        }

        this.nexusTentacles.push({
            geom,
            positions,
            line,
            points,
            claws,
            baseAngle,
            // Per-tentacle speed/phase variance so they never move in lockstep, whether
            // drifting calmly or lunging aggressively.
            speedMult: 0.85 + Math.random() * 0.4,
            phaseSeed: Math.random() * Math.PI * 2,
            aggroT: 0 // eased 0 (idle/jellyfish) .. 1 (aggressive/lunging), see animateNexus
        });
    }

    this.nexusGroup.add(this.nexusCreatureGroup);
    this.scene.add(this.nexusGroup);
};
