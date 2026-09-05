/* A working avatar you can copy.
 *
 * This file is NOT loaded by Aether1. It is here to be copied: save it as
 * avatar-<your-name>.js, change the id and the shapes, and load it (see the bottom of
 * this file, and js/hologram/README.md).
 *
 * What it draws: a wire cube that turns, brightens with the voice, and jumps on a
 * click. Deliberately plain -- the point is that every line is something you can see
 * on screen and change.
 *
 * The three rules, and there are only three:
 *
 *   1. build(api) runs once. Put everything you make into a group and return it.
 *   2. animate(model, ctx) runs about sixty times a second. Change things; do not
 *      make things -- building geometry every frame is what makes an avatar stutter.
 *   3. applyPalette(model, palette) runs when the colour theme changes. Retint here,
 *      not in build(), so your avatar follows the theme.
 */

HologramAvatar.registerAvatar({
    /* The name Aether1 knows this avatar by. Lowercase, no spaces; it ends up in a
       saved setting, so changing it later means anyone using it gets the default back. */
    id: 'template-cube',

    /* What a person sees in a picker. */
    label: 'Template cube',

    /* ---- build ------------------------------------------------------------
     * Called once, when the engine starts.
     *
     * `api` gives you:
     *   api.THREE     the 3D library (also available as the global THREE)
     *   api.palette   the colours in use right now
     *   api.helpers   texture makers the built-in avatars use:
     *                 glowTexture(size), radialGlowTexture(size, centre, edge),
     *                 ringTexture(size, thickness), hexVertices(radius, rotation)
     *
     * Return an object. It MUST have a `group` -- a THREE.Object3D that Aether1 adds
     * to the scene and shows or hides as the avatar is selected. Put anything else you
     * want to reach later on the same object; it comes back to you as `model`.
     *
     * A sense of scale: the camera sits 240 units back, and the built-in avatars are
     * roughly 60-100 units across. Something 5 units wide will look like a speck.
     */
    build(api) {
        const group = new THREE.Group();

        /* Materials are shared between the shapes that use them, which is also why
           retinting one material below recolours everything wearing it.
           MeshBasicMaterial ignores lights -- right for something that reads as drawn
           light rather than a lit object. */
        const edgeMaterial = new THREE.LineBasicMaterial({
            color: api.palette.hex,
            transparent: true,
            opacity: 0.9,
        });
        const faceMaterial = new THREE.MeshBasicMaterial({
            color: api.palette.hex,
            transparent: true,
            opacity: 0.12,
            side: THREE.DoubleSide,
            blending: THREE.AdditiveBlending, // overlapping faces add up and glow
        });

        const boxGeometry = new THREE.BoxGeometry(70, 70, 70);
        const faces = new THREE.Mesh(boxGeometry, faceMaterial);
        const edges = new THREE.LineSegments(new THREE.EdgesGeometry(boxGeometry), edgeMaterial);
        group.add(faces, edges);

        /* Everything on this object comes back as `model` in the other two functions. */
        return { group, edgeMaterial, faceMaterial };
    },

    /* ---- animate ----------------------------------------------------------
     * Called once a frame while your avatar is the one on screen.
     *
     * `ctx` tells you what is going on:
     *   ctx.time       seconds since the engine started, always climbing
     *   ctx.audio      0 to 1, how loud the voice is right now
     *   ctx.audioData  64 frequency bins, 0-255, quiet to loud, low to high
     *   ctx.click      0 to 1, briefly, when someone clicks the avatar
     *   ctx.state      'IDLE' | 'LISTENING' | 'THINKING' | 'SPEAKING'
     *   ctx.palette    the colours in use
     *
     * Use ctx.time rather than counting frames: frames are not evenly spaced, and an
     * avatar that counts them runs at a different speed on a different machine.
     */
    animate(model, ctx) {
        model.group.rotation.y = ctx.time * 0.4;
        model.group.rotation.x = Math.sin(ctx.time * 0.3) * 0.4;

        // Grow with the voice, and jump on a click.
        model.group.scale.setScalar(1 + ctx.audio * 0.35 + ctx.click * 0.25);

        // Brighten while thinking, so the avatar says something the text does not.
        const thinking = ctx.state === 'THINKING' ? Math.abs(Math.sin(ctx.time * 4)) * 0.4 : 0;
        model.edgeMaterial.opacity = 0.55 + ctx.audio * 0.45 + thinking;
    },

    /* ---- applyPalette -----------------------------------------------------
     * Called when someone picks a different colour theme, and once at startup.
     *
     * A palette has: hex (the main colour), hex2 and hex3 (two companions), and r/g/b
     * as 0-1 numbers for anything that needs them separately.
     *
     * Not every part should follow the theme. Every built-in avatar keeps its core a
     * fixed obsidian black and its "hot" accents a fixed colour, so the avatar still
     * looks like itself in six different themes. Leave those out of this function.
     */
    applyPalette(model, palette) {
        model.edgeMaterial.color.setHex(palette.hex);
        model.faceMaterial.color.setHex(palette.hex3);
    },
});

/* ---- Loading your avatar -----------------------------------------------------
 *
 * While you are working on it: open frontend/avatar-lab.html and use "Load an avatar
 * file". Nothing needs editing, and you can reload the file as often as you like.
 *
 * To install it in Aether1 for good: put the file in frontend/js/hologram/ and add one
 * line to frontend/index.html, next to the other avatar scripts near the bottom:
 *
 *     <script src="js/hologram/avatar-your-name.js?v=1"></script>
 *
 * It must come after core.js and before app.js. Then add it to the avatar picker in
 * the same file, using the same id you chose above:
 *
 *     <button class="avatar-pill ..." data-avatar-val="template-cube">Template cube</button>
 */
