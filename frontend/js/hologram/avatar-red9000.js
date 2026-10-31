/* Avatar: R.E.D. 9000 -- an obsidian eye with a hot lens glow behind dark camera glass,
 * two eyelid arcs mounted at different depths like a camera's stacked bezel rings, and a
 * light inside that breathes with the voice.
 *
 * The eye never looks away from you. At rest it is still, apart from a blink every few
 * seconds and a faint nod when something is clicked; the work shows in the lens rather
 * than in the pose.
 *
 * Registered through the same HologramAvatar.registerAvatar() contract any avatar file
 * uses -- see js/hologram/README.md and avatar-template.js.
 */

HologramAvatar.registerAvatar({
    id: 'red',
    label: 'R.E.D. 9000',
    group: 'Singular Ascended Class',

    build(api) {
        const group = new THREE.Group();

        /* The obsidian core. A faint warm emissive tint rather than only a specular
           highlight, so the glass reads as having heat inside it rather than merely
           catching the light -- it matters most at the rim, where the lens glow sprite
           in front of it cannot reach. Fixed, not theme-tinted: this avatar's one
           constant identity colour, same convention every other avatar follows. */
        const coreMat = new THREE.MeshPhongMaterial({
            color: 0x0a0505,
            emissive: 0x3a0a04,
            specular: 0xff6a55,
            shininess: 90,
            transparent: true,
            opacity: 0.97,
        });
        const coreSphere = new THREE.Mesh(new THREE.SphereGeometry(22, 32, 32), coreMat);
        group.add(coreSphere);

        // The tactical wireframe shell. Sparse and dim on purpose: it is an accent, and
        // anything denser masks the obsidian core underneath it.
        const lensMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex,
            wireframe: true,
            transparent: true,
            opacity: 0.12,
            blending: THREE.AdditiveBlending,
        });
        const lensOuter = new THREE.Mesh(new THREE.SphereGeometry(34, 10, 10), lensMat);
        group.add(lensOuter);

        /* The lens glow: a fixed yellow-hot-to-crimson gradient on the core's front face.
           A sprite, so it always faces the camera and reads correctly through the group's
           thinking and speaking wobble, seated just outside the core's radius of 22 so the
           core's own near surface never occludes it. */
        const lensGlowTexture = api.helpers.radialGlowTexture(128, '#fff26b', '#ff2200');
        const lensGlowMat = new THREE.SpriteMaterial({
            map: lensGlowTexture,
            transparent: true,
            opacity: 0.55,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
        });
        const lensGlow = new THREE.Sprite(lensGlowMat);
        lensGlow.scale.set(30, 30, 1);
        lensGlow.position.z = 25;
        group.add(lensGlow);

        /* The same gradient again at the core's own centre, drawn with no depth test so
           the opaque sphere in front of it never hides it: it bleeds past the silhouette
           on every side rather than only the front, which is what makes it read as light
           radiating from inside the glass instead of a decal stuck on the front. Dimmer,
           and drawn first, so the core's surface still reads as solid glass with light
           behind it rather than as a hole. */
        const innerGlowMat = new THREE.SpriteMaterial({
            map: lensGlowTexture,
            transparent: true,
            opacity: 0.35,
            blending: THREE.AdditiveBlending,
            depthWrite: false,
            depthTest: false,
        });
        const innerGlow = new THREE.Sprite(innerGlowMat);
        innerGlow.scale.set(50, 50, 1);
        innerGlow.renderOrder = -1;
        group.add(innerGlow);

        // A warm point light at the lens, so the core's specular highlight breathes along
        // with the glow above rather than sitting still while everything else moves.
        const eyeLight = new THREE.PointLight(0xff6a33, 3, 90);
        eyeLight.position.z = 20;
        group.add(eyeLight);

        /* The eyelids: flat partial rings cupping the core from above and below, each at
           its own depth like a camera's stacked bezel rings rather than both flush on one
           plane. These two do follow the theme. */
        const blueMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex2,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending,
        });
        const blueLid = new THREE.Mesh(new THREE.RingGeometry(58, 62, 48, 1, 0.35, 2.44), blueMat);
        blueLid.position.z = 8;
        group.add(blueLid);

        const cyanMat = new THREE.MeshBasicMaterial({
            color: api.palette.hex3,
            side: THREE.DoubleSide,
            transparent: true,
            opacity: 0.85,
            blending: THREE.AdditiveBlending,
        });
        const cyanLid = new THREE.Mesh(new THREE.RingGeometry(58, 62, 48, 1, Math.PI + 0.35, 2.44), cyanMat);
        cyanLid.position.z = 18;
        group.add(cyanLid);

        return {
            group,
            coreSphere, lensMat, lensOuter, lensGlow, innerGlow, eyeLight,
            blueLid, cyanLid, blueMat, cyanMat,
            /* The blink clock. It lived on the engine before this avatar carried its own
               state; starting in the past means the first blink is scheduled rather than
               playing on frame one. */
            blinkStart: -10,
            nextBlink: 4,
            handledClick: -999,
        };
    },

    applyPalette(model, p) {
        // The core, both glows and the eye light are fixed -- see build.
        model.lensMat.color.setHex(p.hex);
        model.blueMat.color.setHex(p.hex2);
        model.cyanMat.color.setHex(p.hex3);
    },

    animate(model, ctx) {
        const isThinking = ctx.state === 'THINKING';
        const isSpeaking = ctx.state === 'SPEAKING';
        const t = ctx.time;

        if (isThinking || isSpeaking) {
            model.group.rotation.y = Math.sin(t * 0.3) * 0.15;
            model.group.rotation.x = Math.sin(t * 0.2) * 0.1;
        } else {
            // Front-facing and still at rest, apart from a faint nod on a click and the
            // blink below -- a click triggers one immediately, and otherwise it blinks on
            // its own every few seconds so the eye never reads as a fixed prop.
            model.group.rotation.y += (ctx.click * 0.12 - model.group.rotation.y) * 0.15;
            model.group.rotation.x += (ctx.click * 0.08 - model.group.rotation.x) * 0.15;

            if (ctx.clickTime !== model.handledClick) {
                model.handledClick = ctx.clickTime;
                model.blinkStart = t;
                model.nextBlink = t + 4 + Math.random() * 4;
            }
            if (t >= model.nextBlink) {
                model.blinkStart = t;
                model.nextBlink = t + 4 + Math.random() * 5;
            }
        }

        let coreScale;
        if (isSpeaking) coreScale = 1 + ctx.audio * 1.3;
        else if (isThinking) coreScale = 1 + Math.sin(t * 16) * 0.3;
        else coreScale = 1 + ctx.click * 0.35;
        model.coreSphere.scale.setScalar(coreScale);

        const lensSpin = isThinking ? 0.02 : (isSpeaking ? 0.012 : ctx.click * 0.02);
        model.lensOuter.rotation.y += lensSpin;
        model.lensOuter.rotation.x += lensSpin * 0.6;
        model.lensOuter.scale.setScalar(isSpeaking ? 1 + ctx.audio * 0.6 : 1 + ctx.click * 0.2);

        /* The blink: both lids scale toward the eye's equator and back, meeting over the
           core for an instant. A faint audio flicker continues while speaking, brightened
           for the moment the lids sweep past the lens. */
        const BLINK_DURATION = 0.28;
        const blinkAge = t - model.blinkStart;
        const closeness = (blinkAge >= 0 && blinkAge < BLINK_DURATION)
            ? Math.sin((blinkAge / BLINK_DURATION) * Math.PI)
            : 0;
        const lidScaleY = 1 - closeness * 0.96;
        const lidOpacity = (isSpeaking ? 0.85 + ctx.audio * 0.15 : 0.85) + closeness * 0.15;
        model.blueLid.scale.y = lidScaleY;
        model.blueMat.opacity = lidOpacity;
        model.cyanLid.scale.y = lidScaleY;
        model.cyanMat.opacity = lidOpacity;

        // The glow and the light pulse together off one signal, with a bright pop at the
        // peak of a blink -- a glint off the lens as the lids pass it.
        let glow;
        if (isSpeaking) glow = 0.6 + ctx.audio * 1.3 + Math.sin(t * 14) * 0.08;
        else if (isThinking) glow = 0.6 + Math.sin(t * 16) * 0.35;
        else glow = 0.5 + ctx.click * 0.4;
        glow += closeness * 0.5;

        const glowScale = 30 * (1 + glow * 0.18);
        model.lensGlow.scale.set(glowScale, glowScale, 1);
        model.lensGlow.material.opacity = Math.min(1, 0.35 + glow * 0.4);

        const haloScale = 50 * (1 + glow * 0.22);
        model.innerGlow.scale.set(haloScale, haloScale, 1);
        model.innerGlow.material.opacity = Math.min(0.7, 0.2 + glow * 0.28);

        model.eyeLight.intensity = 2 + glow * 5;
    },
});
