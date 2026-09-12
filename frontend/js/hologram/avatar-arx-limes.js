// Avatar 2: A.R.X.LIMES -- Faceted Floating Hub with a Fractured Convex Dome of Plates.

HologramAvatar.prototype.buildArxLimesAvatar = function() {
    this.arxLimesGroup = new THREE.Group();

    // Shared materials -- one bright outline tone threads through every shard.
    this.arxLimesHubFillMat = new THREE.MeshBasicMaterial({
        color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.32, blending: THREE.AdditiveBlending
    });
    this.arxLimesOutlineMat = new THREE.LineBasicMaterial({ color: 0xff3300, transparent: true, opacity: 0.95 });
    this.arxLimesMainFillMat = new THREE.MeshBasicMaterial({
        color: 0xffaa00, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
    });
    this.arxLimesWingFillMat = new THREE.MeshBasicMaterial({
        color: 0xff5500, side: THREE.DoubleSide, transparent: true, opacity: 0.4, blending: THREE.AdditiveBlending
    });

    // --- Central Hub: small, semi-transparent faceted anchor. Touches nothing. ---
    // Geodesic (icosahedron, subdivided once) reads as a rounder, more eye-lens-like
    // gem than a plain 8-face octahedron, while still staying faceted rather than smooth.
    const hubGeom = new THREE.IcosahedronGeometry(9, 1);
    this.arxLimesHubMesh = new THREE.Mesh(hubGeom, this.arxLimesHubFillMat);
    const hubEdges = new THREE.EdgesGeometry(hubGeom, 12);
    this.arxLimesHubOutline = new THREE.LineSegments(hubEdges, this.arxLimesOutlineMat);
    this.arxLimesGroup.add(this.arxLimesHubMesh);
    this.arxLimesGroup.add(this.arxLimesHubOutline);

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
        const plateGroup = this.buildPolygonShard(shape, 10, fillMat, this.arxLimesOutlineMat);
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
