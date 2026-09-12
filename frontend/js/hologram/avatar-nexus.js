// Avatar 3: The Nexus Crew (v3) -- an elongated, bulbous cephalopod hull tapering into a
// forward eye-lens cluster (carried over from the previous build), rendered as a
// semi-transparent wireframe/point-cloud "live sensor feed" (not a solid textured mesh),
// with 10 long chain-link tentacles trailing
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

    // Squash to an oval in head-on view: same width (X), 60% of the height (Y). Applied as
    // an Object3D-level scale (not baked into hullGeom's vertices) so it's one shared factor
    // across all four hull representations above, and so the ribs/eye-lens cluster below --
    // children of nexusHeadMesh -- automatically flatten along with it instead of needing
    // their own hand-adjusted Y coordinates. animate.js's per-frame "breathing" pulse
    // multiplies on top of this base scale rather than overwriting it, so the oval
    // proportions hold through that too.
    this.nexusHeadHeightScale = 0.6; // read by animateNexus's breathing-pulse code
    [this.nexusHeadMesh, this.nexusHeadInnerWire, this.nexusHeadOutline, this.nexusHeadPoints].forEach(obj => {
        obj.scale.set(1, this.nexusHeadHeightScale, 1);
    });

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

    // Forward eye-lens cluster -- the same hand-placed, irregular scatter of 8 lenses
    // (varied size, asymmetric spread) carried over from the previous build, just rescaled
    // onto this hull's narrower nose so they still sit close to its surface. Fixed phosphor
    // red regardless of color theme, like every avatar's always-lit accent (hAlcy's core,
    // R.E.D.'s eye, etc.).
    const eyeMat = new THREE.MeshBasicMaterial({
        color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    const eyeGlowMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xff2418, transparent: true, opacity: 0.55,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.nexusEyeRingMat = new THREE.SpriteMaterial({
        map: this.createRingSpriteTexture(64, 0.16), color: 0xffdca0, transparent: true, opacity: 0.8,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.nexusEyeHighlightMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(32), color: 0xffffff, transparent: true, opacity: 0.95,
        depthWrite: false
    });
    // [rawEx, rawEy, rawEz, er] -- the original layout, verbatim; only the xy scale (0.75,
    // fitted to this hull's ~4-8 unit nose radius instead of the old icosahedron's ~14-18)
    // and a +1 nose-ward nudge on z are new.
    const eyeLayout = [
        [-7.5, -4, 15.5, 2.2], [-4.5, -6.5, 17, 1.8], [-1, -3.5, 18.5, 2.6],
        [2.5, -6, 17.5, 1.9], [6, -3.5, 16, 2.3], [8, -6.5, 14, 1.5],
        [0, -8, 15.5, 1.6], [-2.5, -1.5, 18.8, 1.4]
    ];
    const eyeXyScale = 0.75;
    eyeLayout.forEach(([rawEx, rawEy, rawEz, er]) => {
        const ex = rawEx * eyeXyScale;
        const ey = rawEy * eyeXyScale;
        const ez = rawEz + 1;

        const eye = new THREE.Mesh(new THREE.SphereGeometry(er, 8, 8), eyeMat);
        eye.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(eye); // child of the hull so the cluster pulses/scales with it

        const glow = new THREE.Sprite(eyeGlowMat);
        glow.scale.setScalar(er * 3.2);
        glow.position.set(ex, ey, ez);
        this.nexusHeadMesh.add(glow);

        const ring = new THREE.Sprite(this.nexusEyeRingMat);
        ring.scale.setScalar(er * 2.6);
        ring.position.set(ex, ey, ez + 0.3);
        this.nexusHeadMesh.add(ring);

        const highlight = new THREE.Sprite(this.nexusEyeHighlightMat);
        highlight.scale.setScalar(er * 0.85);
        highlight.position.set(ex - er * 0.35, ey + er * 0.35, ez + 0.6);
        this.nexusHeadMesh.add(highlight);

        this.nexusEyes.push({ mesh: eye, glow, ring, highlight });
    });

    // --- Tentacles: 10 long, segmented, chain-like tendrils trailing from the hull's
    // tail. Each tentacle's joint positions live in one shared Float32Array (rewritten
    // every frame by animateNexus); a chain of tapered cylinder links spans every
    // consecutive pair for real, controllable thickness -- a plain Line's width can't be
    // reliably controlled across browsers/GPUs in WebGL, so this is deliberately actual
    // geometry, not a wide line. A Points cloud at the same positions gives the glowing
    // joint markers, brighter than the links between them -- the same "nodes brighter than
    // structure lines" depth cue as the hull, and read together as banded chain links. ---
    const tentacleCount = 10;
    const segmentsPerTentacle = 21; // 1.5x the original 14
    const linkUnitGeom = new THREE.CylinderGeometry(1, 1, 1, 6); // unit cylinder along Y, one shared geometry rescaled per link per frame
    this.nexusTentacleLinkMat = new THREE.MeshBasicMaterial({
        color: 0x00ff66, transparent: true, opacity: 0.65
    });
    this.nexusTentaclePointMat = new THREE.PointsMaterial({
        color: 0x00ffaa, map: this.createGlowSpriteTexture(24), size: 3.2,
        transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending,
        depthWrite: false, sizeAttenuation: true
    });
    // Pincer tips -- three wireframe prongs per tentacle, the same fixed red as the eye
    // cluster, so every "always lit" part of the Crew reads as one accent color.
    const clawMat = new THREE.LineBasicMaterial({
        color: 0xff2418, transparent: true, opacity: 0.95, blending: THREE.AdditiveBlending
    });
    const clawGlowMat = new THREE.SpriteMaterial({
        map: this.createGlowSpriteTexture(24), color: 0xff2418, transparent: true, opacity: 0.4,
        blending: THREE.AdditiveBlending, depthWrite: false
    });
    for (let t = 0; t < tentacleCount; t++) {
        const baseAngle = (t / tentacleCount) * Math.PI * 2;
        const geom = new THREE.BufferGeometry();
        const positions = new Float32Array(segmentsPerTentacle * 3);
        geom.setAttribute('position', new THREE.BufferAttribute(positions, 3));

        const points = new THREE.Points(geom, this.nexusTentaclePointMat);
        this.nexusRestTiltGroup.add(points);

        // One link per gap between consecutive joints; animateNexus rescales/reorients each
        // one every frame to span its two endpoints, tapering thinner toward the tip.
        const links = [];
        for (let l = 0; l < segmentsPerTentacle - 1; l++) {
            const link = new THREE.Mesh(linkUnitGeom, this.nexusTentacleLinkMat);
            this.nexusRestTiltGroup.add(link);
            links.push(link);
        }

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
            points,
            links,
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
    this.avatarZoomGroup.add(this.nexusGroup);
};
