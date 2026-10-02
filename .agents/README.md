# Agent Skills For hd

This folder holds four reusable skills for agents that work on hd's
design. Each is a standard [Agent Skills](https://agentskills.io/home)
folder, `SKILL.md` with `name` and `description` frontmatter.

| Skill | Use it to |
| --- | --- |
| [`stress-test`](skills/stress-test/SKILL.md) | try a recorded decision set against real library code, and rank what breaks |
| [`complexity-reducer`](skills/complexity-reducer/SKILL.md) | find cuts and merges in a design or spec area, with soundness and rule accounting |
| [`brainstorm`](skills/brainstorm/SKILL.md) | survey other languages and compare three to five options for an open question |
| [`spec-update`](skills/spec-update/SKILL.md) | apply a decided owner decision end to end: spec, fixtures, prototype, known issues, docs, checks, and push |

All four follow [skills/shared-rules.md](skills/shared-rules.md): the owner
decides design, agents only ask (only `spec-update` edits the spec, and only
to apply a decision), and the git, safety, and writing rules.

## Layout

```
AGENTS.md                      repo instructions; points here
.agents/
  README.md                    this file
  skills/
    shared-rules.md            rules every skill reads first
    stress-test/SKILL.md
    complexity-reducer/SKILL.md
    brainstorm/SKILL.md
    spec-update/SKILL.md
.claude/skills -> ../.agents/skills   committed symlink, for Claude Code
```

`.agents/skills/` is the single source of truth. The only harness-specific
entry is the `.claude/skills` symlink, because Claude Code does not read
`.agents/`. The rest of `.claude/` (agent worktrees) is ignored by git.

## Using A Skill From A Subagent

A harness that reads skills lists them and loads one when its description
matches the task, or when you name it. For a subagent, or a harness with no
skill support, say in the prompt: "Read `.agents/skills/<name>/SKILL.md` and
`.agents/skills/shared-rules.md`, and follow them." The skills are plain
Markdown, so this works everywhere.

## Which Harness Reads What

Checked on 2026-09-27 against the documentation below. Claude Code
(2.1.283) and Codex CLI (0.156.1) were also checked by running them on a
scratch repository.

| Harness | Reads `AGENTS.md` | Finds these skills | What it gets |
| --- | --- | --- | --- |
| Claude Code | yes, from v2.1.277, when there is no `CLAUDE.md` or `CLAUDE.local.md` in or above the working directory | through the `.claude/skills` symlink only; it reads nothing under `.agents/` | on-demand skills, also invocable as `/stress-test` and so on |
| OpenAI Codex CLI | yes | yes, `.agents/skills` natively | on-demand skills |
| GitHub Copilot (VS Code, CLI, cloud agent) | yes | yes, `.agents/skills` or `.claude/skills` | on-demand skills |
| Cursor | yes | yes, `.agents/skills` natively | on-demand skills |
| Gemini CLI | only if `context.fileName` in its settings lists `AGENTS.md` | yes, `.agents/skills` natively | on-demand skills |

What no harness gets from this folder:

- **No selectable subagents.** These are skills, not subagent definitions.
  A harness that has subagents (Claude Code's `.claude/agents/`, Codex's
  `.codex/agents/*.toml`, Copilot's `.github/agents/`, Gemini's
  `.gemini/agents/`) gets no new agent type. Tell the subagent to read the
  skill instead.
- **Gemini CLI without the setting** reads the skills but not `AGENTS.md`.
- **Claude Code before v2.1.277**, or in a session that cannot read
  `AGENTS.md`, sees the skills but not the pointer in `AGENTS.md`.
- **A `CLAUDE.md` or `CLAUDE.local.md`** added anywhere at or above the
  repo root stops Claude Code from reading `AGENTS.md`, unless its
  Project instructions setting is `claude-md-and-agents-md`.
- **Windows checkouts** without symlink support get a plain file named
  `.claude/skills`, so Claude Code finds no skills there.
- Claude-only frontmatter (`allowed-tools`, `context: fork`) is not used,
  so every harness reads the same files.

## Sources

- Claude Code, AGENTS.md support and its conditions, and "Not read: ...
  anything under a `.agents/` directory":
  <https://code.claude.com/docs/en/memory#agents-md>
- Claude Code skill locations (`.claude/skills/`) and symlinked skill
  folders: <https://code.claude.com/docs/en/skills>
- Claude Code subagents: <https://code.claude.com/docs/en/sub-agents>
- Codex AGENTS.md discovery: <https://learn.chatgpt.com/docs/agent-configuration/agents-md>
- Codex skills ("Codex scans `.agents/skills` in every directory from your
  current working directory up to the repository root"):
  <https://learn.chatgpt.com/docs/build-skills>
- Codex subagents (TOML): <https://learn.chatgpt.com/docs/agent-configuration/subagents>
- Copilot in VS Code, custom instructions and AGENTS.md:
  <https://code.visualstudio.com/docs/copilot/customization/custom-instructions>
- Copilot in VS Code, custom agents:
  <https://code.visualstudio.com/docs/copilot/customization/custom-agents>
- Copilot repository instructions and AGENTS.md:
  <https://docs.github.com/en/copilot/how-tos/configure-custom-instructions/add-repository-instructions>
- Copilot agent skills (".github/skills, .claude/skills, or .agents/skills"):
  <https://docs.github.com/en/copilot/concepts/agents/about-agent-skills>
- Cursor rules and AGENTS.md: <https://cursor.com/docs/rules>
- Cursor skills: <https://cursor.com/docs/skills>
- Gemini CLI context files and `context.fileName`:
  <https://geminicli.com/docs/cli/gemini-md>
- Gemini CLI skills (`.gemini/skills` or the `.agents/skills` alias):
  <https://geminicli.com/docs/cli/skills>
- AGENTS.md standard: <https://agents.md>
- Agent Skills specification (`name` must match the folder name, at most
  64 characters; `description` at most 1024):
  <https://agentskills.io/specification>
