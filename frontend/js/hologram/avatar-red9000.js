// Avatar 4: R.E.D. 9000 (HAL 9000) -- Obsidian Eye with Two Static Eyelid Arcs.

HologramAvatar.prototype.buildRed9000Avatar = function() {
    this.redGroup = new THREE.Group();

    // 1. Obsidian inner core (HAL / Reactive Daemon Eye) -- fixed dark glossy material,
    // a touch smaller than the old glowing-red sphere, not theme-tinted.
    const coreGeom = new THREE.SphereGeometry(22, 32, 32);
    const coreMat = new THREE.MeshPhongMaterial({
        color: 0x0a0505,
        specular: 0xff6a55,
        shininess: 90,
        transparent: true,
        opacity: 0.97
    });
    this.redCoreSphere = new THREE.Mesh(coreGeom, coreMat);
    this.redGroup.add(this.redCoreSphere);

    // Outer Tactical Wireframe Lens Shell -- a faint accent, kept sparse and dim so it
    // doesn't mask the obsidian core underneath.
    const lensGeom = new THREE.SphereGeometry(34, 10, 10);
    const lensMat = new THREE.MeshBasicMaterial({
        color: 0xff3355,
        wireframe: true,
        transparent: true,
        opacity: 0.12,
        blending: THREE.AdditiveBlending
    });
    this.redLensOuter = new THREE.Mesh(lensGeom, lensMat);
    this.redGroup.add(this.redLensOuter);

    // 2. Eyelid arcs -- flat partial rings cupping the core from above and below,
    // replacing the old two full tilted orbit rings.
    const blueGeom = new THREE.RingGeometry(58, 62, 48, 1, 0.35, 2.44);
    const blueMat = new THREE.MeshBasicMaterial({
        color: 0x0066ff,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.85,
        blending: THREE.AdditiveBlending
    });
    this.redBlueCircle = new THREE.Mesh(blueGeom, blueMat);
    this.redGroup.add(this.redBlueCircle);

    const cyanGeom = new THREE.RingGeometry(58, 62, 48, 1, Math.PI + 0.35, 2.44);
    const cyanMat = new THREE.MeshBasicMaterial({
        color: 0x00f0ff,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.85,
        blending: THREE.AdditiveBlending
    });
    this.redCyanCircle = new THREE.Mesh(cyanGeom, cyanMat);
    this.redGroup.add(this.redCyanCircle);

    this.scene.add(this.redGroup);
};
