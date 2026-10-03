#!/usr/bin/env bash
# Every argument passed to a Tauri command must be named the way Tauri will look for it.
#
# A command declared `fn graft_select_project_rust(project_path: String)` is called from
# JavaScript as `{ projectPath }`, because Tauri converts snake_case parameters to camelCase
# across the boundary. Pass `{ project_path }` and the call is rejected at runtime with
# "missing required key projectPath" -- the panel shows an error, and nothing in a build, a
# clippy run or a unit test sees it, because both halves are individually correct. That is
# exactly what shipped in the Graft panel: two calls, both wrong, found by an operator
# clicking the button.
#
# Only the argument object's own keys are checked. A nested object is a payload being
# serialised rather than an argument list -- `save_settings_rust` takes a map of settings
# whose names are snake_case on purpose -- and a value is not a key, so
# `{ deviceCode: code.device_code }` is correct and must stay quiet. That distinction is why
# this is a brace-counting walk and not a grep.
set -euo pipefail

cd "$(dirname "$0")/.."

node - "$@" <<'JS'
const fs = require('fs');
const path = require('path');

function jsFiles(dir) {
    return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) return entry.name === 'vendor' ? [] : jsFiles(full);
        return entry.name.endsWith('.js') ? [full] : [];
    });
}

// The keys of one object literal, starting at its opening brace, at depth 1 only.
function topLevelKeys(source, open) {
    const keys = [];
    let depth = 0;
    for (let i = open; i < source.length; i += 1) {
        const char = source[i];
        if (char === '{' || char === '[' || char === '(') depth += 1;
        else if (char === '}' || char === ']' || char === ')') {
            depth -= 1;
            if (depth === 0) break;
        } else if (depth === 1) {
            const ahead = source.slice(i);
            const key = ahead.match(/^['"]?([A-Za-z_$][\w$]*)['"]?\s*[:,}]/);
            if (key && (i === open + 1 || /[{,\s]/.test(source[i - 1]))) {
                keys.push(key[1]);
                i += key[1].length - 1;
            }
        }
    }
    return keys;
}

const offenders = [];
for (const file of jsFiles('frontend')) {
    const source = fs.readFileSync(file, 'utf8');
    const call = /\b(?:tauriInvoke|invoke)\(\s*['"]([^'"]+)['"]\s*,\s*\{/g;
    let match;
    while ((match = call.exec(source)) !== null) {
        const open = match.index + match[0].length - 1;
        for (const key of topLevelKeys(source, open)) {
            if (key.includes('_')) {
                const line = source.slice(0, match.index).split('\n').length;
                offenders.push(`${file}:${line}: ${match[1]} is passed { ${key}: ... }`);
            }
        }
    }
}

if (offenders.length > 0) {
    console.error('These Tauri calls name an argument in snake_case; Tauri expects camelCase:');
    for (const offender of offenders) console.error(`  ${offender}`);
    console.error('');
    console.error('Rename the key in the JavaScript -- { project_path: p } becomes { projectPath: p }.');
    console.error('The Rust parameter stays snake_case; Tauri converts it.');
    process.exit(1);
}

console.log('OK: every Tauri call names its arguments in camelCase.');
JS
