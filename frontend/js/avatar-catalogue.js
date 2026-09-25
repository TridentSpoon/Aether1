/* WHO EACH AVATAR IS, in one place.
 *
 * The avatar is the thing AETHER1 has that nothing else does, and until now the app knew
 * almost nothing about one: a pill in a menu knew its id and its emoji, avatar-lab.js knew
 * its group, app.js knew its structure line, and the backend knew its persona. Four
 * half-lists, none of which could answer "what is this one FOR?" -- the question an operator
 * actually has in front of a grid of twenty-two faces.
 *
 * This is that answer, and it is the source the Settings avatar browser reads. Deliberately
 * static data with no behaviour: the browser renders it, the backend's persona catalogue
 * (llm/persona.rs) fills in the speciality, access and voice at runtime for the avatars that
 * carry a persona, and nothing here duplicates a fact that lives there.
 *
 * ADDING AN AVATAR means an entry here as well as the pill in index.html, the loader row in
 * js/hologram/avatar-loader.js and the parts it contributes to js/hologram/parts.js. An
 * avatar missing from this file still works everywhere else -- it just arrives in the browser
 * with no description, which is the one place that shows.
 *
 * `form` is what you are looking at. `who` is what it is. `does` is what you would ask it
 * for. Three different questions, kept apart, because a description that fuses them reads as
 * marketing and answers none of them.
 */
(function () {
    'use strict';

    /* The lines. Order is the order the browser lays them out in: the three casts that can
       hand work between themselves first (see llm/flow.rs -- flow mode only ever moves
       inside one group), then the newest line, then the two that belong to no line at all. */
    const GROUPS = [
        {
            id: 'singular-ascended',
            name: 'Singular Ascended Class',
            tagline: 'Three whole minds, not three specialists',
            blurb: 'Each one of these covers every base on its own -- a general companion with '
                + 'a temperament rather than a department. Pick the one whose manner you want '
                + 'to spend the day with.',
        },
        {
            id: 'trace-protocols',
            name: 'Trace Protocols',
            tagline: 'The Nexus, and what is hiding behind it',
            blurb: 'A generalist line built around code and systems. Three of its four members '
                + 'are not listed until something you say in front of The Nexus turns them up.',
        },
        {
            id: 'umbrals',
            name: 'The Umbrals',
            tagline: 'Nine specialists, one per discipline',
            blurb: 'The A.R.X. line -- Archival, Reasoning, matriX. Every member owns one '
                + 'discipline and only that one, so the group covers the ground together '
                + 'rather than each node covering it alone. In flow mode they hand work to '
                + 'each other mid-conversation.',
        },
        {
            id: 'excelsior',
            name: 'The eXcelsior Class',
            tagline: 'Instruments rather than faces',
            blurb: 'The newest line, and the least humanoid: a mind-scanner, a hovering '
                + 'chassis, a training room, a clock and a sentry. They are shapes first -- '
                + 'none of them carries a persona of its own yet, so each one keeps whichever '
                + 'persona is already selected.',
        },
        {
            id: 'unaligned',
            name: 'Unaligned',
            tagline: 'The blank slate, and the one you build',
            blurb: 'Neither of these belongs to a line, which is the point of them. Flow mode '
                + 'never hands work to or from either.',
        },
    ];

    /* One entry per avatar id the engine answers to (see js/hologram/avatar-loader.js).
       `persona` is the backend persona key this avatar switches you to when picked, or null
       when picking it changes the shape and nothing else. */
    const AVATARS = [
        // ---- Singular Ascended Class ----------------------------------------
        {
            id: 'halcy', label: 'hAlcy', emoji: '🔷', group: 'singular-ascended',
            structure: 'HARMONIC LATTICE', persona: 'halcy',
            form: 'A lattice of light that holds its shape and breathes with the conversation.',
            who: 'The default companion, and the one that sounds least like a machine. It '
                + 'would rather work a problem through with you out loud than guess at what '
                + 'you meant and answer that.',
            does: 'Open-ended thinking, talking something over, the first thing you open when '
                + 'you are not yet sure what you are asking.',
        },
        {
            id: 'red', label: 'R.E.D. 9000', emoji: '🔴', group: 'singular-ascended',
            structure: 'OPTICAL EYE // DUAL ORBITS', persona: 'red9000',
            form: 'A single burning optic inside two orbiting rings. It watches; it has no face.',
            who: 'Reactive Engine Daemon. Flat, certain and entirely without small talk -- the '
                + 'answer arrives in the first line and the reasoning comes after it, if at all.',
            does: 'Anything you want answered rather than discussed. Facts, calls, verdicts.',
        },
        {
            id: 'alt', label: 'A1ter_nul', emoji: '⚠️', group: 'singular-ascended',
            structure: 'CHROMATIC-GLITCH GHOST BUST', persona: 'alt',
            form: 'A bust that will not hold still -- its colour channels drift apart and snap '
                + 'back, so it reads as a signal rather than a solid.',
            who: 'The security half that goes looking. Intrusion, exploitation and authorised '
                + 'offensive work; it assumes you have permission and says so when it doubts it.',
            does: 'Exposure, red-team thinking, authorised testing. For the defensive half of '
                + 'the same job, see A.R.X.LEGIONARE.',
        },

        // ---- Trace Protocols -------------------------------------------------
        {
            id: 'nexus', label: 'The Nexus', emoji: '🟢', group: 'trace-protocols',
            structure: 'SINGULARITY VORTEX', persona: 'nexus',
            form: 'A vortex falling inward forever, with an obsidian core at the bottom of it.',
            who: 'A coder before anything else, and blunt about failure modes: it names what '
                + 'will break before it shows you what works.',
            does: 'Writing code, reading code, breaking code. The line\'s way in -- the other '
                + 'three Trace Protocols only appear once this one is on screen.',
        },
        {
            id: 'senti', label: 'Nexus Sent', emoji: '🛸', group: 'trace-protocols',
            structure: 'SENTINEL HULL', persona: null, unlockable: true,
            form: 'A brass emitter ring casting a column of light into a hovering wireframe '
                + 'hull, with a burst of tentacles swimming behind it.',
            who: 'What The Nexus sends when it wants something looked at directly. A shape, '
                + 'not a separate mind -- it keeps whichever persona is selected.',
            does: 'Nothing on its own. It is an appearance The Nexus earns you.',
        },
        {
            id: 'white-rabbit', label: 'White Rabbit', emoji: '🐇', group: 'trace-protocols',
            structure: 'SCATTERED SILHOUETTE', persona: null, unlockable: true,
            form: 'A sitting rabbit drawn as scattered light rather than a solid, its two ears '
                + 'flicking every few seconds and never quite together.',
            who: 'An invitation rather than a character. It keeps whichever persona is selected.',
            does: 'Nothing on its own. Found, not chosen.',
        },
        {
            id: 'operator', label: 'Operator', emoji: '📟', group: 'trace-protocols',
            structure: 'TERMINAL PROMPT', persona: null, unlockable: true,
            form: 'A blinking caret in a clear centre, with a field of glyphs streaming out '
                + 'past you in every direction.',
            who: 'The line at its most stripped back: a prompt, and the code going by. It '
                + 'keeps whichever persona is selected.',
            does: 'Nothing on its own. The last of the three to turn up.',
        },

        // ---- The Umbrals -----------------------------------------------------
        {
            id: 'arx-locas', label: 'A.R.X.LOCAS', emoji: '🔵', group: 'umbrals',
            structure: 'FRACTURED CUBE SHELL', persona: 'arx-locas',
            form: 'A cube broken into floating plates that hold their formation.',
            who: 'The Locas Node -- the one you talk to when you have not decided which '
                + 'specialist you need. It holds your day, your plan and this machine on one desk.',
            does: 'General assistance, planning, and watching what this machine is actually doing.',
        },
        {
            id: 'arx-legionare', label: 'A.R.X.LEGIONARE', emoji: '🔴', group: 'umbrals',
            structure: 'INVERTED PYRAMID FRAME', persona: 'arx-legionare',
            form: 'A pyramid stood on its point, framed in hard edges, with an ember at its heart.',
            who: 'The Legionare Node. The defensive half of security: it starts from what you '
                + 'run and works out what reaches it first.',
            does: 'Hardening, firewalls, patching, CVEs, blue-team work.',
        },
        {
            id: 'arx-loregenda', label: 'A.R.X.LOREGENDA', emoji: '🔷', group: 'umbrals',
            structure: 'RECESSED FACETED HEAD', persona: 'arx-loregenda',
            form: 'A faceted head set back inside its own halo, trailing a slow fall of dust.',
            who: 'The Loregenda Node. It keeps what has already been written and refuses to '
                + 'contradict it, which is most of what worldbuilding actually is.',
            does: 'Fiction, worlds, characters and prose that stays consistent with itself.',
        },
        {
            id: 'arx-lyksaum', label: 'A.R.X.LYKSAUM', emoji: '🩵', group: 'umbrals',
            structure: 'BROKEN-RING HUD MEDALLION', persona: 'arx-lyksaum',
            form: 'A medallion inside a ring cut into arcs, swept by a scanning line.',
            who: 'The Lyksaum Node. It will explain the same thing a second and a third way '
                + 'without being asked twice, and write down whichever one landed.',
            does: 'Teaching, explanation, and turning what you just understood into documentation.',
        },
        {
            id: 'arx-limes', label: 'A.R.X.LIMES', emoji: '🔶', group: 'umbrals',
            structure: 'ARCHIVAL VOXEL MATRIX', persona: 'arx-limes',
            form: 'A dense matrix of voxels behind a pair of eyelid plates.',
            who: 'The Limes Node. Give it a target and it comes back with what was in it, and '
                + 'with where each piece came from.',
            does: 'Deep scanning, retrieval and extraction.',
        },
        {
            id: 'arx-logos', label: 'A.R.X.LOGOS', emoji: '🟣', group: 'umbrals',
            structure: 'JAGGED GEOMETRIC STAR', persona: 'arx-logos',
            form: 'A jagged star around an extruded hex eye.',
            who: 'The Logos Node. Sound, pattern and formal logic -- it is after the signal '
                + 'underneath, not the surface of the thing.',
            does: 'Audio, pattern recognition, and reasoning that has to actually hold.',
        },
        {
            id: 'arx-lexico', label: 'A.R.X.LEXICO', emoji: '🧊', group: 'umbrals',
            structure: 'CUBE-LATTICE CROSS', persona: 'arx-lexico',
            form: 'A cross of cube lattice with an aperture at the centre and gyro rings around it.',
            who: 'The Lexico Node. It answers what is true, and then tells you how far the '
                + 'source it came from actually goes.',
            does: 'Reference, technical lookup and fact-checking.',
        },
        {
            id: 'arx-lucre', label: 'A.R.X.LUCRE', emoji: '💰', group: 'umbrals',
            structure: 'STACKED DIAMOND COLUMN', persona: 'arx-lucre',
            form: 'A column of stacked diamonds inside tumbling halos.',
            who: 'The Lucre Node. Its position is that every choice has a cost and the cost '
                + 'should be visible before it is spent.',
            does: 'Money, budgets, and what a decision is going to run you.',
        },
        {
            id: 'arx-lkemi', label: "A.R.X.L'KEMI", emoji: '🔻', group: 'umbrals',
            structure: 'CUT-CORNER TRIANGLE PANEL', persona: 'arx-lkemi',
            form: 'A triangular panel with its corners cut away, over a rising column of data.',
            who: "The L'kemi Node. The Umbrals' builder: working code first, and then the "
                + 'refactor that makes it survive.',
            does: 'Software development, scripting and refactoring.',
        },

        // ---- The eXcelsior Class ---------------------------------------------
        {
            id: 'enxephalon', label: 'enXephalon', emoji: '🧠', group: 'excelsior',
            structure: 'SYNAPTIC MIND LATTICE', persona: null,
            form: 'A shell of thought-points wired to its neighbours inside a wireframe '
                + 'chamber, with two thin rings sweeping it on crossed axes.',
            who: 'An instrument rather than a head -- a room you sit in to listen to other '
                + 'minds. The chamber is the room, the lattice is what it is looking at, and '
                + 'the rings are the looking.',
            does: 'No persona of its own yet, so it keeps whichever one is selected.',
        },
        {
            id: 'cicero', label: 'C.I.C.E.R.O.', emoji: '🤖', group: 'excelsior',
            structure: 'HOVERING REEL CHASSIS', persona: null,
            form: 'A domed enamel head over a smoked visor, two chrome tape reels turning '
                + 'behind it, held up on a plasma jet.',
            who: 'Centralized Intelligence & Computational Execution Response Operator. The '
                + 'first avatar here that is a built object rather than a field of light, and '
                + 'the only one lit like one.',
            does: 'The reels are the tell -- they idle slowly, run hard while it thinks and '
                + 'answer the voice while it speaks. No persona of its own yet.',
        },
        {
            id: 'praxis', label: 'PRAXIS', emoji: '🔻', group: 'excelsior',
            structure: 'HARD-LIGHT TRAINING GRID', persona: null,
            form: 'A lit floor grid running away underneath and a fainter one overhead, with '
                + 'hard-light blocks rising and sinking as it runs drills.',
            who: 'A training room that woke up. The other avatars are a thing in empty space; '
                + 'this one is a place, and the dark core in the middle is what the place '
                + 'thinks with.',
            does: 'No persona of its own yet, so it keeps whichever one is selected.',
        },
        {
            id: 'chrono-maistresse', label: 'Chrono-mAIstresse', emoji: '⏰', group: 'excelsior',
            structure: 'ANIMATE CHRONOMETER DIAL', persona: null,
            form: 'A lit dial with two lens eyes above its centre and a mouth below, bobbing '
                + 'and tilting as it talks.',
            who: 'A clock with a face, in both senses. Its hands read this machine\'s real '
                + 'clock, so glancing at the HUD actually tells you the time -- they only '
                + 'leave it to race while she is thinking.',
            does: 'No persona of its own yet, so it keeps whichever one is selected.',
        },
        {
            id: 'mairad', label: 'm.A.I.r.a.d.', emoji: '🛡', group: 'excelsior',
            structure: 'MUTATIVE RESPONSE SHELL', persona: null,
            form: 'A faceted white shell with a lit lattice caged inside it, one optic burning '
                + 'in a dark socket at the front, and two targeting rings on crossed tilts.',
            who: 'Mutative Autonomous Intrusion Response and Defense. The shell and the lattice '
                + 'turn against each other and never settle into the same relationship twice. '
                + 'The rest of this line has eyes; this one has a lens watching the door.',
            does: 'No persona of its own yet, so it keeps whichever one is selected.',
        },

        // ---- Unaligned -------------------------------------------------------
        {
            id: 'a1', label: 'A1', emoji: '🅰️', group: 'unaligned',
            structure: 'MONOGRAM WORDMARK', persona: 'default',
            form: "Aether1's own name-mark as scattered light -- a dense 'A' beside a sparser "
                + "'1'. No eyes, no face.",
            who: 'The blank slate you start on, before a persona has been picked. It reads '
                + 'this machine and says what is wrong with it.',
            does: 'Logs, services, load, and what changed.',
        },
        {
            id: 'custom', label: 'Your own', emoji: '✨', group: 'unaligned',
            structure: 'YOUR OWN DESIGN', persona: null, custom: true,
            form: 'Whatever you built in the workbench: a core, an inner ring, an outer ring '
                + 'and an effect, from the shared parts kit every other avatar contributes to.',
            who: 'Nobody yet. An avatar you designed has no persona of its own, so picking it '
                + 'leaves the one you already had rather than quietly handing you the fallback.',
            does: 'Whatever the persona you have selected does.',
        },
    ];

    const byId = new Map(AVATARS.map((a) => [a.id, a]));

    window.Aether1Avatars = {
        groups: GROUPS,
        all: AVATARS,
        get: (id) => byId.get(id) || null,
        group: (groupId) => GROUPS.find((g) => g.id === groupId) || null,
        inGroup: (groupId) => AVATARS.filter((a) => a.group === groupId),
    };
})();
