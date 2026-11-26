# Repeatable AETHER CODE evaluation

Run `scripts/code_eval.py` from this checkout with a model served on localhost or a private IP. Each invocation creates a new Git repository and HOME under `/tmp`; it leaves `result.json`, `agent-output.txt`, and the fixture there for inspection. The Aether mode sets `AETHER1_CODE_EVAL_DB` to a database in that temporary HOME and forces local-only mode. The direct mode runs OpenCode with the same model and prompt in a Bubblewrap filesystem and network sandbox. A narrow Unix-socket relay connects it only to the configured local HTTP model endpoint; no account credentials are passed to the process.

Build the current binary first with `cargo build --manifest-path src-tauri/Cargo.toml`. For each case, run both modes with the same model and endpoint:

```sh
python3 scripts/code_eval.py --case small --mode aether --model MODEL --model-version DIGEST
python3 scripts/code_eval.py --case small --mode direct --model MODEL --model-version DIGEST
```

Repeat for `medium` and `long`. The cases are fixed in `scripts/code_eval.py`; change them only in a reviewed fixture revision, since changing a prompt or test between runs makes scores incomparable. `result.json` records the prompt, case, mode, model and version, engine version, machine, elapsed time, agent exit, test result, intervention count, and whether recovery was used. It labels a setup refusal as `blocked` instead of counting it as a model failure. Inspect `agent-output.txt` for the full reason.

The runner never points an agent at this checkout. It requires a working Bubblewrap installation, permission to create a local Unix socket, and a local model service. This managed host currently refuses Unix socket creation and cannot reach Ollama, so live scores from here would be setup failures, not evidence about coding quality.
