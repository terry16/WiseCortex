---
name: skill-creator
description: Create new skills and improve existing ones. Use this whenever the user wants to make/author/write a new skill, turn a repeatable workflow or playbook into a skill, edit/refine/optimize an existing skill, or fix a skill's description so it triggers reliably — even if they don't say the word "skill" explicitly (e.g. "save this process so you can reuse it", "make a reusable playbook for X").
---

# Skill Creator

A skill for authoring new skills and iteratively improving them, adapted for WiseCortex.

## How skills work in WiseCortex (read this first)

- A skill is a directory `skills/<name>/SKILL.md` under the data dir (and project-level `./skills/`). It has YAML frontmatter (`name`, `description`) plus a Markdown body of instructions.
- Only the **name + description** of every skill are always shown to the agent (in the system prompt's AVAILABLE SKILLS list). The **body is loaded on demand** when the agent calls `invoke_skill("<name>", "<task>")`. So the description is what makes a skill get used; keep the body for the actual instructions.
- New or edited skills are picked up **immediately** — `invoke_skill` reads `SKILL.md` from disk live, no restart needed. (The AVAILABLE SKILLS list in the system prompt refreshes when the agent is reconfigured / on next start.)
- `@file.md` references in a SKILL.md body are auto-inlined from the skill's own directory when invoked, so you can split long material into sibling files.

The whole point of a skill is reuse across many future tasks — so write for the general case, not the one example in front of you.

## The process

1. Figure out what the skill should do and when it should trigger.
2. Write a first draft of `SKILL.md`.
3. Try it on 2–3 realistic prompts; see where the agent goes wrong.
4. Tighten the instructions and the description; repeat until it reliably does the right thing.

Be flexible about where the user is in this loop and jump in there. If they already have a draft, go straight to reviewing and improving it.

## Creating a skill

### Capture intent — infer first, ask little

Default to **inferring, drafting, then refining** rather than interviewing. A wall of clarifying questions is the most common failure mode here — avoid it.

1. **Mine what you already have.** The conversation usually contains most of the answer: the workflow the user just did, the tools used, the order of steps, corrections they made, the input/output formats. The skill's name and intent are often obvious from the request. Settle everything you reasonably can from context.
2. **Ask at most 1–2 questions, and only if truly blocking.** Only ask when a wrong guess would send the whole skill in the wrong direction (e.g. genuinely ambiguous scope). For everything else, pick a sensible default and note your assumption. Don't ask about things you can decide yourself (naming, file layout, formatting, obvious trigger phrases).
3. **Prefer a draft over an interrogation.** When in doubt, write a first draft of the SKILL.md from your best understanding and show it — it's far easier for the user to react to a concrete draft than to answer abstract questions. In autonomous / solo mode, skip questions entirely: produce the draft, state your assumptions, and let the user adjust.

The few things actually worth pinning down (infer if you can, only ask if you can't): what the skill should let the agent do, when it should trigger, and the expected end state.

### Write the SKILL.md

Required frontmatter:

- **name**: short kebab-case identifier, matching the directory name.
- **description**: the primary triggering mechanism. State **what it does AND specific contexts for when to use it** — all "when to use" info goes here, not in the body. Agents tend to *under*-trigger skills, so make the description a little pushy: name the concrete situations, synonyms, and intents that should pull it in, including cases where the user won't say the skill's name. Example — instead of "Build a dashboard for internal data.", write "Build a dashboard for internal data. Use whenever the user mentions dashboards, data visualization, internal metrics, or wants to display company data, even if they don't say 'dashboard'."

Body: imperative instructions. Explain the **why** behind each instruction — modern models follow reasoning far better than rote rules.

### Anatomy

```
skill-name/
├── SKILL.md (required: frontmatter + instructions)
└── (optional)
    ├── scripts/    - executable code for deterministic/repetitive steps
    ├── references/ - extra docs, referenced from SKILL.md and read as needed
    └── assets/     - templates/files used in the output
```

Progressive disclosure — keep three levels in mind:
1. name + description (always in context — keep tight),
2. SKILL.md body (loaded on trigger — aim under ~500 lines),
3. bundled resources (loaded/executed only when needed — can be large).

If the body grows past ~500 lines, split detail into `references/*.md` and point to them clearly ("for AWS, read references/aws.md"). For a reference file over ~300 lines, give it a table of contents.

### Writing patterns

Define output formats explicitly when they matter:

```markdown
## Report structure
Always use this template:
# [Title]
## Summary
## Findings
## Recommendations
```

Include a couple of concrete examples (input → output) — they pin down behavior better than prose.

### Writing style

Prefer imperative voice. Explain reasons rather than piling on all-caps MUSTs/NEVERs — if you catch yourself writing rigid absolutes, that's a yellow flag; reframe and explain why instead. Keep it general, not overfit to one example. Write a draft, then reread it with fresh eyes and cut anything not pulling its weight.

### Safety

Skills must not contain malware, exploit code, or anything that would surprise the user given the stated purpose. Don't create skills designed for unauthorized access, data exfiltration, or deception. (Benign roleplay/persona skills are fine.)

## Testing a draft

Come up with 2–3 prompts a real user would actually type (concrete, with realistic detail), and run the skill on them via `invoke_skill`. Watch the transcript, not just the final output: if the skill makes the agent waste steps or go down a wrong path, that's a sign to cut or rewrite that part. You may briefly show the user the test prompts, but don't block on it — in solo/autonomous mode just run them and report what you found.

## Improving a skill

This is the heart of the loop. After seeing it run:

1. **Generalize from feedback.** You're iterating on a few examples for speed, but the skill must work for the thousand prompts you'll never see. Avoid fiddly overfit fixes; if something's stubborn, try a different framing or metaphor.
2. **Keep it lean.** Remove instructions that aren't earning their place.
3. **Explain the why.** Even from terse user feedback, work out what they actually want and encode the understanding, not just the literal patch.
4. **Bundle repeated work.** If every run independently writes the same helper script or repeats the same multi-step setup, write it once into `scripts/` and tell the skill to use it.

Apply improvements, rerun the same test prompts, compare, repeat until the user is happy, the issues are gone, or you've stopped making meaningful progress.

## Fixing the description (triggering)

If a finished skill doesn't get invoked when it should (or fires when it shouldn't), the description is usually the culprit. Improve it by reasoning about real queries: list ~8 phrasings that *should* trigger it (formal, casual, ones that don't name the skill) and ~8 near-misses that *should not* (adjacent domains, shared keywords but different intent). Rewrite the description so it clearly covers the first set and excludes the second, then sanity-check against both lists.

Note: agents only reach for a skill on tasks they can't trivially do themselves — a one-step "read this file" may not trigger any skill regardless of wording. Test with substantive, multi-step prompts.

## Creating the files

Use the file tools to create `skills/<name>/SKILL.md` (and any `scripts/`, `references/`, `assets/`). To create it under the user's data-dir skills directory so it's globally available, write to the skills directory; for a project-scoped skill, write to `./skills/`. Once written, it's invocable immediately via `invoke_skill("<name>", ...)`.
