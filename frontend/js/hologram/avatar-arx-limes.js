// Avatar 2: A.R.X.LIMES -- Faceted Floating Hub with a Fractured Convex Dome of Plates.

HologramAvatar.prototype.buildArxLimesAvatar = function() {
    this.arxLimesGroup = new THREE.Group();

    // Shared materials -- one bright outline tone threads through every shard.
    // Plates are now real extruded slabs (see buildPolygonShard's thickness param), so
    // depthWrite stays at its default true: each plate needs to occlude its own back
    // face and side walls correctly, and to occlude/be occluded by the hub and its
    // neighbours -- unlike a thin glow overlay, this is genuine solid geometry.
    this.arxLimesOutlineMat = new THREE.LineBasicMaterial({ color: 0xff3300, transparent: true, opacity: 0.95 });
    this.arxLimesMainFillMat = new THREE.MeshBasicMaterial({
        color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
    });
    this.arxLimesWingFillMat = new THREE.MeshBasicMaterial({
        color: 0xff5500, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
    });
    const PLATE_THICKNESS = 10;

    // --- Central Hub: a black hole. A near-opaque void (no additive blending -- an
    // additive near-black is invisible against the HUD's dark backdrop, and this needs
    // to read as a solid absence, not a faint haze) ringed by a fixed hot "event
    // horizon" rim that -- like every other avatar's one hot accent -- never retints
    // with the theme, plus a swirling accretion disc standing in for infalling matter.
    // Geodesic (icosahedron, subdivided once) reads as a rounder, more eye-lens-like
    // gem than a plain 8-face octahedron, while still staying faceted rather than smooth.
    this.arxLimesHubFillMat = new THREE.MeshBasicMaterial({
        color: 0x040209, side: THREE.DoubleSide, transparent: true, opacity: 0.94
    });
    this.arxLimesHubRimMat = new THREE.LineBasicMaterial({ color: 0xffb347, transparent: true, opacity: 0.95 });
    const hubGeom = new THREE.IcosahedronGeometry(9, 1);
    this.arxLimesHubMesh = new THREE.Mesh(hubGeom, this.arxLimesHubFillMat);
    const hubEdges = new THREE.EdgesGeometry(hubGeom, 12);
    this.arxLimesHubOutline = new THREE.LineSegments(hubEdges, this.arxLimesHubRimMat);
    this.arxLimesGroup.add(this.arxLimesHubMesh);
    this.arxLimesGroup.add(this.arxLimesHubOutline);

    // --- Accretion disc: two soft glowing bands tilted off-axis around the hub, plus a
    // handful of bright hotspots orbiting the inner one so the disc visibly swirls rather
    // than just glowing in place. Flat RingGeometry annuli textured with a soft radial-fade
    // gradient (createRadialBandTexture), not solid-shaded TorusGeometry tubes -- an unlit
    // tube reads as painted plastic, while a feathered gradient band reads as actual light.
    // Real mesh geometry, not sprites -- a Sprite always faces the camera regardless of its
    // parent's rotation, so it can't be tilted; the tilt is the whole point here. A fixed
    // hot-accent colour like the rim, not theme-tinted -- it's meant to read as glowing
    // infalling matter, not a UI element. ---
    this.arxLimesAccretionGroup = new THREE.Group();
    this.arxLimesAccretionGroup.rotation.x = 1.15;
    this.arxLimesGroup.add(this.arxLimesAccretionGroup);

    this.arxLimesAccretionBandTexture = this.createRadialBandTexture();

    this.arxLimesAccretionRing1Mat = new THREE.MeshBasicMaterial({
        map: this.arxLimesAccretionBandTexture, color: 0xffb347, side: THREE.DoubleSide,
        transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.arxLimesAccretionRing1 = new THREE.Mesh(new THREE.RingGeometry(13, 19, 64), this.arxLimesAccretionRing1Mat);
    this.arxLimesAccretionGroup.add(this.arxLimesAccretionRing1);

    this.arxLimesAccretionRing2Mat = new THREE.MeshBasicMaterial({
        map: this.arxLimesAccretionBandTexture, color: 0xff6a1a, side: THREE.DoubleSide,
        transparent: true, opacity: 0.55, blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.arxLimesAccretionRing2 = new THREE.Mesh(new THREE.RingGeometry(18.5, 26, 64), this.arxLimesAccretionRing2Mat);
    this.arxLimesAccretionGroup.add(this.arxLimesAccretionRing2);

    this.arxLimesAccretionHotspotsMat = new THREE.MeshBasicMaterial({
        color: 0xfff2c0, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false
    });
    this.arxLimesAccretionHotspots = [];
    for (let i = 0; i < 5; i++) {
        const dot = new THREE.Mesh(new THREE.SphereGeometry(0.9, 8, 8), this.arxLimesAccretionHotspotsMat);
        this.arxLimesAccretionGroup.add(dot);
        this.arxLimesAccretionHotspots.push({ mesh: dot, angle: (i / 5) * Math.PI * 2, radius: 16 + (i % 2) * 5, speed: 0.01 + i * 0.002 });
    }

    // --- Plate shapes, all defined locally extending toward +Y ("outward"), so every
    // plate can share the same angle -> world orientation formula regardless of type. ---
    const topShape = [{ x: -16, y: 0 }, { x: 16, y: 0 }, { x: 8, y: 13 }, { x: -8, y: 13 }];
    // Bottom anchor is larger than the top one, per spec. Both are wide and shallow --
    // shaped like eyelid arcs hugging the hub -- rather than tall narrow trapezoids.
    const bottomShape = [{ x: -19, y: 0 }, { x: 19, y: 0 }, { x: 10, y: 16 }, { x: -10, y: 16 } ];
    // Elongated, irregular wing blade swept toward +X; mirrored (negate x) for the left side.
    const wingShapeRight = [{ x: -2, y: 0 }, { x: 2, y: 0 }, { x: 17, y: 26 }, { x: 4, y: 31 }];
    const wingShapeLeft = wingShapeRight.map(p => ({ x: -p.x, y: p.y }));

    const addPlate = (shape, fillMat, worldAngleDeg, tier, radius) => {
        const worldAngle = worldAngleDeg * Math.PI / 180;
        // buildPolygonShard already angles the far (outward) edge backward in Z while
        // keeping the near (hub-facing) edge flush -- no extra rotation needed here.
        const plateGroup = this.buildPolygonShard(shape, 10, fillMat, this.arxLimesOutlineMat, PLATE_THICKNESS);
        plateGroup.position.set(Math.cos(worldAngle) * radius, Math.sin(worldAngle) * radius, 0);
        plateGroup.rotation.z = worldAngle - Math.PI / 2;
        this.arxLimesGroup.add(plateGroup);
        this.arxLimesPlates.push({
            group: plateGroup,
            baseAngle: worldAngle,
            baseRadius: radius,
            phase: worldAngleDeg * 0.03,
            tier
        });
    };

    // Top/bottom hug close to the hub like eyelids; the 4 wings sit further out near the
    // horizontal corners (0/180 deg) like the pointed outer corners/lashes of an eye --
    // together giving the whole cluster a wide, almond-eye silhouette.
    addPlate(topShape, this.arxLimesMainFillMat, 90, 'main', 16);
    addPlate(bottomShape, this.arxLimesMainFillMat, -90, 'main', 16);
    addPlate(wingShapeRight, this.arxLimesWingFillMat, 15, 'wing', 26);
    addPlate(wingShapeRight, this.arxLimesWingFillMat, -15, 'wing', 26);
    addPlate(wingShapeLeft, this.arxLimesWingFillMat, 165, 'wing', 26);
    addPlate(wingShapeLeft, this.arxLimesWingFillMat, -165, 'wing', 26);

    this.avatarZoomGroup.add(this.arxLimesGroup);
};
