---
name: prune-comments
description: Reduce comment verbosity in target files — remove restate-the-code comments and "thinking in comments", keep or sharpen non-obvious WHY, and make public doc comments a relaxed, customer-facing guide.
allowed-tools: Bash, Read, Edit, Grep, Glob
---

Prune comments in the target files. Default scope: $ARGUMENTS. If no argument is given, prune the files changed on the current branch (vs. `main`) plus unstaged/untracked files — i.e. `git diff --name-only main...HEAD` ∪ `git status --porcelain`.

Two kinds of comment live in this crate and they pull in opposite directions:

- **Internal comments** (`//` inside function bodies, private items) — the governing default is **no comment**. The burden of proof is on *retention*: a comment survives only by saying something the code cannot. When you can't decide, delete. "It's not wrong" is not a reason to keep one.
- **Public doc comments** (`///`, `//!` on public items) — these render on docs.rs and *are* the crate's documentation. The default flips: a public item *should* be documented, and the bar is "does this help a reader use the crate?" not "does it say something the code can't?" Don't strip these to fragments. See **Doc comments** below.

**Zero is a valid result.** If a file's comments all earn their place, leave it untouched and report "no changes." Do not lower the bar to find work. Do not delete a load-bearing WHY to feel productive. A run that touches nothing is a correct run when the comments are already disciplined.

## Internal comments

### Remove

- **Restate-the-code.** A comment that paraphrases the next line. `// increment counter` over `i += 1`. `// loop over batches` over `for batch in batches`. `// return early if none` over `let Some(x) = x else { return };`. The identifier and structure already say it.
- **Thinking in comments.** Running narration of the author's decision process: "we tried X but switched to Y", "TODO: maybe refactor this later", "consider whether this should be...", "first we do A, then we do B, finally C". Section headers that recap the next 5 lines. Planning prose that belonged in a PR description.
- **Change-history annotations.** "was `bool`, now an enum", "added for the imaging path", "previously took `&str`", "renamed from foo". That's what git is for; the diff and commit message carry it, the source must not.
- **Phase / task / caller bookkeeping.** `(Phase 4.C)`, `// Phase 12 "Bitmaps"`, "used by `view.rs`", "fixes #1234", "part of the markup-bridge work". The codebase was built in phases; restating the phase on individual items is decoration that rots when the phase ends, the caller is renamed, or the issue closes. Belongs in commit messages and PR descriptions. The one exception is in Keep below — a phase marker that encodes a real removal trigger.
- **SDK-mirror / port annotations.** "mirrors `IntegrationGLUT`", "matches the C# binding", "matching `AoR`'s authoring style", "ports the Noesis sample order". The shim is authored *against* the SDK and its reference bindings by design; restating that on each identifier is noise, and the citation rots when the SDK moves. Drop it — unless the reference is the *anchor* for a non-obvious correctness note (see Keep).
- **Banner / separator comments.** `// ===== HELPERS =====`, `// --- private methods ---`, ASCII-art dividers. The codebase's `// ── Section ──` headers are fine *only* when they label a genuine region of a long file and say something the next identifier doesn't — a bare divider over one function is noise. If a file is so long it needs banners to navigate, the fix is to split the file.
- **Commented-out code.** Delete it. Git remembers. The only exception: a single line documenting a known-broken alternative *with* a one-line reason it's disabled (e.g. `// view.set_size(0, 0)  // disabled — zero size trips a Noesis assert`).
- **Stale / contradicted comments.** Any comment whose claim disagrees with the current code. Either fix it or remove it; never leave the lie. (The single-thread ownership and refcount-alive-at-drop invariants are load-bearing — a comment that contradicts them is actively dangerous.)
- **Trivial field/parameter comments.** `size: UVec2, // the size`. `handler: H, // the handler`. If the name is bad, fix the name; don't paper over it.
- **Closing-brace labels.** `} // end of if`, `} // impl ClassBuilder`. The editor folds; the indentation shows scope.

### Keep (and sharpen if vague)

Fitting one of these categories makes a comment *eligible* to survive — it does not make it survive. The comment must also carry a WHY a competent reader of *this code* would actually miss. A category match over an obvious-anyway fact is still a delete.

- **Non-obvious WHY.** A choice that would surprise a reader: why this algorithm over the textbook one, why this loop bound, why this order of operations matters.
- **Hidden constraints / invariants.** "Runtime must still be alive at drop — `Release()` touches freed state otherwise." "Single rendering thread: view, renderer, and input are not thread-safe." "Drop the guard before `shutdown`." "Hands ownership across the C ABI; the C side now frees it." Anything a future editor could violate by mistake. The architectural invariants (hand-written C ABI with opaque pointers, single-thread ownership, RAII registration guards, refcount-on-drop) are exactly this kind of anchor.
- **FFI boundary contracts.** `#[repr(C)]` POD whose layout must match a header; a size/offset the C++ side promises; why a `mem::forget` is correct (ownership crossed the ABI); why an `unsafe` block upholds an invariant the type system can't see. These are the load-bearing comments in an FFI crate.
- **Units / encoding.** `secs_f64`, `premultiplied alpha`, `row-major Matrix4`, `codepoint, not byte index`. A bare number or buffer whose unit/encoding isn't obvious from its type earns a comment.
- **Workaround citations.** A comment paired with a concrete external reference: a Noesis SDK quirk, an SDK header path, a `LIMITATIONS.md` entry. The citation is the value — bare "// workaround" is not.
- **Subtle correctness anchors.** Things tests can't easily catch but reviewers reliably get wrong: a NUL-in-string panic boundary, an interior-pointer lifetime, a refcount that must not double-release.
- **Scope-limited phase markers** *only* when they encode a real removal trigger, e.g. `// REMOVE once the v2 FFI lands type-tagged defaults`. A bare "(Phase 4.C)" gets cut.

## Doc comments

`///` and `//!` on public items are the crate's published documentation — what a customer reads on docs.rs, not just an inline note. This crate is heading for a crates.io release, so the goal here is a **useful, readable guide to the crate**, not the terse minimalism that governs internal comments. Keep them; make them good.

What "good" means here:

- **Relaxed and customer-facing.** Write the way the README does: plain, direct, second person where it helps ("You implement this against your asset pipeline", "Drop the guard to clear the registration"). Full sentences are welcome — this is prose a stranger reads to learn the crate, not a margin note.
- **Orient first, then qualify.** Lead with what the type or function is *for* and when you'd reach for it. On docs.rs the reader may not have the signature in front of them, so a one-line orientation earns its place even when it restates the WHAT a little. Then add the gotcha that bites: threading, drop order, a NUL-panics precondition, a returned `None` meaning.
- **Link related items.** `[`View::create`]`, `[`crate::shutdown`]`. Intra-doc links are what turn a flat list of items into a navigable guide; prefer them over bare backtick names.

Strip the AI tone from doc prose — it reads as filler and dates badly:

- **Throat-clearing.** "Note that", "It's important to note", "Keep in mind", "As you can see", "Here we". Just state the fact.
- **Filler intensifiers and marketing.** "simply", "just", "easily", "powerful", "robust", "seamless", "leverage", "utilize", "blazingly". Cut them; say the plain verb.
- **Hedging on known facts.** "should generally", "might typically". If it's a fact, state it. If it's genuinely conditional, say the condition.
- **Narrated process.** "First we do A, then B, finally C" walkthroughs of an obvious body. Document the contract, not the steps.
- **Restate-only docstrings.** A `///` that only re-says the signature ("Takes a `&str` and returns a `View`") with nothing added. Either add the orientation/gotcha that makes it useful, or drop it and let the signature speak — an empty-calorie docstring is worse than none.

Doc comments still obey the internal-comment removal rules for *noise*: no phase markers (`(Phase 4.C)`), no change-history, no SDK-mirror annotations, no "thinking out loud" inside a `///`. Strip those even when the surrounding doc stays.

Worked sharpenings — de-AI'd and oriented, not amputated:

- `//! Safe wrappers around the Noesis FrameworkElement, IView, and IRenderer opaque pointers (Phase 4.C).` → `//! Safe wrappers around the Noesis FrameworkElement, View, and Renderer.` (drop the phase marker; the rest already orients)
- `/// This function simply leverages the underlying Noesis API to easily create a powerful view from the given element.` → `/// Creates a View that hosts and renders the given element.`
- `/// Builder for a single class registration. Note that you must call register() to finish.` → `/// Builds and registers one custom class. Call [`Self::register`] to finish; the returned guard unregisters on drop.`

## Process

1. Resolve the file set from `$ARGUMENTS` (or the default git-derived set above). Exclude generated/vendored/external trees: anything under `target/`, the SDK (`$NOESIS_SDK_DIR`), the `.claude/worktrees/` copies, and any file marked generated.
2. Read each file fully before editing — context matters; a comment near the top may be load-bearing for code at the bottom.
3. For each comment, first decide which kind it is. Internal `//`: classify against Remove/Keep; to keep one, state its WHY/invariant to yourself in a single clause, and if that clause just paraphrases the code or you reach for "context"/"clarity"/"it helps", delete. Public `///`/`//!`: keep it, but make it earn its place as a guide — de-AI the tone, add the missing orientation or gotcha, strip embedded noise. Uncertainty on an internal comment resolves to deletion; uncertainty on a public doc resolves to a clearer, relaxed rewrite.
4. Apply edits with `Edit`. Do not reflow unrelated code, do not rename, do not reorder. Comments only.
5. After editing, report a per-file tally: `src/view.rs — removed 7, rewrote 2, kept 4` (count a de-AI'd doc rewrite under "rewrote"). No diff dumps; the user can read the diff themselves.
6. Do **not** run formatters, build, or tests. Comment-only edits don't change behavior; running CI here just adds latency and noise. If the user wants verification, they'll ask. (A `cargo doc` preview is the one thing worth offering after a doc-heavy run.)

## Tone for internal rewrites

A kept internal comment is worth keeping *and* worth shortening. Sharpening is part of the prune, not a separate nicety — a verbose survivor is a half-done removal. (Doc comments are the exception: relaxed prose is the point there, not a fragment.)

- A fragment beats a sentence. Cut to the surprising token and stop. No subject, no verb, no closing punctuation if they earn nothing.
- One line. A citation may take a second line; nothing else may. If the WHY won't fit on one line, the comment is carrying explanation that belongs in the commit/PR.
- Lead with the constraint or reason, not "This function...". Drop the WHAT the signature already states; keep only the WHY/invariant.
- No hedging ("maybe", "probably", "I think"). If it's uncertain, it isn't worth a comment.
- No first person, no audience address. Just state the fact.

Worked sharpenings — each cuts the restated WHAT and keeps only the surprising part:

- `// We forget the guard here because ownership of the registration has been handed across the C ABI to Noesis, which now frees it` → `// ownership crossed the C ABI; Noesis frees it now`
- `// This must run on the same thread as the view because Noesis is not thread-safe and the renderer shares its state` → `// single-thread only — Noesis is not thread-safe`
- `// Release() will touch freed state if the runtime has already shut down, so the wrapper has to drop first` → `// drop before shutdown — Release() touches freed state otherwise`

## What this skill is not

Not a refactor pass. Not a rename pass. Not a "improve the code" pass. If pruning a comment reveals that the code itself is unclear, **leave the code alone** and either keep a sharpened comment or flag it in the final report ("`classes::add_property_ex` reads obscurely without its old comment — consider renaming in a separate change"). The user decides whether to act on that.
