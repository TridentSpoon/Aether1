// Avatar 4: R.E.D. 9000 (HAL 9000) -- Obsidian Eye with a Hot Lens Glow, Two Eyelid Arcs
// Mounted at Different Depths (like a camera's bezel rings) that Blink, and a Speech-
// Reactive Light (see animateRed9000 in animate.js for all the reactive behavior; this file
// just builds the static geometry).

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

    // Lens glow -- a fixed yellow-hot-to-crimson radial gradient sitting on the obsidian
    // core's front face, like HAL's glowing aperture behind dark camera glass. A sprite
    // (always faces the camera) so it reads correctly regardless of the group's subtle
    // thinking/speaking wobble, seated just outside the core's radius (22) so the core's
    // own near surface never occludes it. Fixed color regardless of theme, same convention
    // as the core itself; animateRed9000 pulses its scale/opacity with speech/thinking/blink.
    const lensGlowTexture = this.createRadialGlowTexture(128, '#fff26b', '#ff2200');
    const lensGlowMat = new THREE.SpriteMaterial({
        map: lensGlowTexture,
        transparent: true,
        opacity: 0.55,
        blending: THREE.AdditiveBlending,
        depthWrite: false
    });
    this.redLensGlow = new THREE.Sprite(lensGlowMat);
    this.redLensGlow.scale.set(30, 30, 1);
    this.redLensGlow.position.z = 25;
    this.redGroup.add(this.redLensGlow);

    // Eye light -- a warm point light seated at the lens so the obsidian core's specular
    // highlight breathes along with the lens glow above (animateRed9000 drives intensity).
    this.redEyeLight = new THREE.PointLight(0xff6a33, 3, 90);
    this.redEyeLight.position.z = 20;
    this.redGroup.add(this.redEyeLight);

    // 2. Eyelid arcs -- flat partial rings cupping the core from above and below, each
    // mounted a bit forward of the core at its own depth (like a camera's stacked bezel
    // rings) instead of both sitting flush on the same plane, replacing the old two full
    // tilted orbit rings.
    const blueGeom = new THREE.RingGeometry(58, 62, 48, 1, 0.35, 2.44);
    const blueMat = new THREE.MeshBasicMaterial({
        color: 0x0066ff,
        side: THREE.DoubleSide,
        transparent: true,
        opacity: 0.85,
        blending: THREE.AdditiveBlending
    });
    this.redBlueCircle = new THREE.Mesh(blueGeom, blueMat);
    this.redBlueCircle.position.z = 8;
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
    this.redCyanCircle.position.z = 18;
    this.redGroup.add(this.redCyanCircle);

    this.scene.add(this.redGroup);
};
