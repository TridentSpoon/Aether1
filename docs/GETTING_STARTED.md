# Giving Aether1 a brain

Fresh out of the box, Aether1 can talk but it cannot think. It will greet you, read
your system stats out loud and answer with a few canned lines — and that is all,
because no AI model is connected yet.

This page fixes that. It takes about ten minutes, most of which is waiting for a
download. You do not need to understand any of it, and nothing here costs money.

> **The short version:** open Aether1, press **🧠 Set up the AI** in the menu, and do
> what it says. The wizard checks your machine and only ever asks you for one thing at
> a time. This page is the same journey written down, for when you would rather read
> it, or when something does not go the way the wizard expected.

---

## What we are installing, and why

The "brain" is a separate free program called **Ollama**. It runs AI models on your
own computer — your words never leave the machine. Aether1 talks to it the way a web
browser talks to a website, except the website is on your own desk.

So there are two things to get:

1. **Ollama** — the program that runs models. Installed once.
2. **A model** — the actual AI. Downloaded once, a file of a few gigabytes.

Then you tell Aether1 which model to use. That is the whole job.

---

## Step 1 — Install Ollama

Pick your machine.

### Windows

1. Go to **https://ollama.com/download** in your web browser.
2. Press the **Windows** button. A file called something like `OllamaSetup.exe`
   downloads.
3. Open that file and press **Next** / **Install** until it finishes.
4. Nothing appears to happen. That is correct — Ollama runs quietly in the
   background. You should see a small llama icon appear near the clock.

### Mac

1. Go to **https://ollama.com/download** in your web browser.
2. Press the **macOS** button. A file called `Ollama.dmg` downloads.
3. Open it and drag the **Ollama** icon into your **Applications** folder.
4. Open **Ollama** from Applications once. It will ask for permission the first time;
   say yes. After that it starts on its own whenever you log in.

### Linux

Open a terminal and paste this one line, then press Enter:

```sh
curl -fsSL https://ollama.com/install.sh | sh
```

It will ask for your password (the installer needs it to place the program on your
system). When it finishes, start it:

```sh
sudo systemctl enable --now ollama
```

If your system does not use systemd, just run `ollama serve` in a terminal and leave
that window open.

### How do I know it worked?

Go back to Aether1, open **🧠 Set up the AI**, and press **🔄 Check again**. If the
headline changes from "there is nothing here yet" to something about choosing a
model, it worked. You can close this page and let the wizard finish.

---

## Step 2 — Download a model

A model is the AI itself. Bigger models are cleverer and slower; smaller ones are
faster and need less memory. Aether1 looks at how much memory your machine has and
marks one of them **Recommended** — that is the one to take unless you have a reason
not to.

Roughly:

| Your computer's memory | What is recommended | Download size |
| --- | --- | --- |
| 2 GB | Qwen 2.5 (tiny) | about 400 MB |
| 4 GB | Llama 3.2 (small) | about 1.3 GB |
| 8 GB | Llama 3.2 (medium) | about 2 GB |
| 16 GB | Llama 3.1 (large) | about 4.9 GB |
| 32 GB | Qwen 2.5 (very large) | about 9 GB |
| 64 GB or more | Llama 3.3 (huge) | about 43 GB |

There are nineteen models on the list in all — the popular ones from Meta (Llama),
Google (Gemma), Alibaba (Qwen), Mistral, Microsoft (Phi) and DeepSeek. The ones your
machine has the memory for are listed straight away; the bigger ones are one click
away, behind a line that reads **Show N bigger models**. Nothing is hidden from you —
if you know your machine better than the guess does, pick past the recommendation.

A note on the DeepSeek R1 models: they think a problem through before answering, so
they are slower but much better at puzzles, maths and code. They are worth trying if
the answers from an ordinary model are not quite good enough.

**In the wizard:** choose one and press **📥 Download this brain**. A progress bar appears
under **DOWNLOADING** showing the real percentage and how many gigabytes have arrived
so far — not a guess, but the figure the model server itself reports. Downloading takes
a few minutes on a fast connection and rather longer on a slow one, and the wizard says
the moment it is done.

You can start more than one download at a time: pick a second model and press Download
again, and it gets its own bar. Three at once is the limit, and there is no advantage to
more — they all share one connection, so starting a fourth would not make any of them
finish sooner.

You can close Aether1 while it downloads — the download is Ollama's job, not Aether1's,
and it carries on regardless. Re-open the wizard and the bars pick up where they were.

**If you would rather type it:** open a terminal (on Windows, press the Start button
and type `cmd`) and run the model's name after `ollama pull`, for example:

```sh
ollama pull llama3.2:3b
```

Leave the window open until it says `success`.

---

## Step 3 — Tell Aether1 to use it

In the wizard, pick the model from the list and press **✔ Use this and finish**. That is it — the
Neural Dialogue Stream is now backed by a real model, and the amber "no AI is
connected" card in the chat disappears.

If you prefer to do it by hand, open **Settings → Agent & System → 🧠 The Brain**
and set:

- **WHERE THE AI RUNS**: `On this computer -- Ollama-style server`
- **ADDRESS**: `http://localhost:11434`
- **WHICH MODEL**: the name you downloaded, e.g. `llama3.2:3b`

Press **📡 Test it** to check, then **Save**. Or open **🔍 Search this computer for AI
servers and keys**, press **🔄 Scan now**, and pick the server it found — that fills
all three in for you.

---

## When something is wrong

**The wizard still says nothing is installed, but I installed it.**
Ollama has to be *running*, not just installed. On Windows and Mac it starts itself —
look for its icon near the clock, and open the Ollama app once if it is not there. On
Linux, run `sudo systemctl start ollama`.

**The wizard says it is running but has no models.**
The install and the model are two separate downloads. Do Step 2.

**The download bar has not moved in a long time.**
Very large models on a slow connection genuinely take an hour or more, and the bar only
moves when bytes actually arrive. If it stops for good, press **📥 Download this brain**
again — nothing is downloaded twice, it picks up where it left off.

**The wizard says Ollama is installed but not running.**
Press **▶ Start it for me**. That is the one gap Aether1 can close by itself: it starts
the copy of Ollama already on this machine and then re-checks. Everything else the
wizard can only describe.

**I get an answer, but it is one of the same few canned lines every time.**
That is offline mode: Aether1 is still set to `offline` rather than to your model.
Open **Settings → Agent & System → 🧠 The Brain** and check the three boxes in
Step 3 above. The status line at the top of that group says in plain words whether a
brain is connected.

**There is no "Download this brain" button in the wizard.**
Aether1 can only start a download when the `ollama` command is on this machine. If
you installed Ollama somewhere else on your network, or installed it in a way that
hid the command, use the typed version in Step 2 instead.

**My computer is too small for any of this.**
Then the honest answer is a cloud provider: an AI run by a company, over the
internet. It is fast and needs nothing from your machine, but **the words you type
leave this computer** — which is exactly what the rest of Aether1 is built to avoid.
The wizard offers it, and never chooses it for you. If you want it, you will need an
account and an API key from the provider, pasted into the **API KEY** box under
**Settings → Agent & System → 🧠 The Brain**.

---

## What about the voice?

Speech is separate from the brain and already works out of the box, using a voice
built into your operating system. Everything under **Settings → Agent & System → 🗣
Voice & Sound** is
optional. The **Offline speech files** box nested in there is for people who have
installed Piper or Whisper by hand and need to point Aether1 at the files; leaving
those boxes empty means "find them yourself", which is what you want.

## Where your things live

- Your conversations and memories: `backend/` inside the Aether1 folder.
- Your notes: whatever folder you set under **Settings → Agent & System → 📓 Memory**.
- The models: Ollama's own folder, outside Aether1. Deleting Aether1 does not delete
  them, and `ollama rm <model name>` is how you get the disk space back.
