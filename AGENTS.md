# AGENTS.md

Working rules for anyone — human or agent — changing this repository. They exist
because the history and the diff are read far more often than they are written.
The plan itself is [`PLAN.md`](PLAN.md).

## Code quality

- Prefer clear names and small, ownership-focused types.
- Comments explain *why*: an invariant, a non-obvious constraint, a lifecycle
  rule, the reference behaviour being ported. Nothing else.
- **Comment budget.** A module doc is one short paragraph. An item doc is one to
  three lines. An inline comment is one or two, and only where the code cannot
  say it itself. No design essays, no narration of the next line, no arguing
  with alternatives the code does not contain. Delete a comment when the code it
  described moves or goes away.
- No speculative abstraction "for later use". Build the contracts the current
  increment can actually exercise.
- No optional fields where the domain guarantees presence. Prefer
  compiler-enforced invariants over defensive runtime branching.
- Never swallow an error merely to keep a superseded path alive.
- Logging stays useful: no per-frame, per-page or per-scroll logs in ordinary
  operation.
- No error context that only restates the underlying error.

## Change discipline

- Keep the change coherent inside the current phase and increment.
- No pile of dead scaffolding in anticipation of later phases.
- When a new implementation becomes authoritative, delete the old one or reduce
  it to a clearly time-bounded compatibility adapter. Never leave an unused copy
  of the production implementation in the tree.

## Definition of done

An increment is done only when:

- the new path is exercised by production code;
- the old path is no longer the authority;
- the requested behaviour still works, and is visually checkable on the day;
- affected tests pass and every CI lane is green;
- stale call sites were searched for and removed;
- lifecycle and ownership are documented where they are not obvious;
- no unexplained fallback remains.

If a criterion cannot be met, report the concrete blocker. Do not silently
revert to the legacy implementation.

## Commit messages

Conventional Commits, one coherent change per commit.

### Subject

```text
<type>[optional scope]: <description>
```

- Exactly one line, with `: ` after the type or scope; imperative, present
  tense; lowercase description unless a proper noun requires otherwise; no
  trailing period.
- **Hard limit: 72 characters**, counting the whole line — type, scope, spaces,
  punctuation. **Prefer ≤ 50.** Do not stretch a subject toward 72 for detail,
  and never cut one mid-word to fit.
- Describe *what changed*. Why it changed, how it works and what was tested
  belong in the body.

| type       | use                                          |
| ---------- | -------------------------------------------- |
| `feat`     | new user-visible functionality               |
| `fix`      | bug correction                               |
| `perf`     | performance or memory improvement            |
| `refactor` | structural change, no intended behaviour change |
| `docs`     | documentation only                           |
| `test`     | tests or test infrastructure                 |
| `ci`       | CI workflow or check changes                 |
| `build`    | build, dependency or toolchain changes       |
| `chore`    | maintenance that fits nothing else           |

A scope is optional and uses repository terms — `pdf`, `reader`, `library`,
`virtualizer`, `lifecycle`, `engine`, `appearance`, `settings`, `ci`, `docs` —
kept short: `fix(pdf):`, not `fix(pdf-renderer-lifecycle-management):`.

Breaking changes use the Conventional Commits syntax:

```text
feat(engine)!: replace the global pdf session

BREAKING CHANGE: callers must create an explicit session.
```

A subject is never a progress report (`phase 2 complete`, `final fixes`) and
never an implementation essay. A phase number may appear in the scope when it
helps a search — `docs(phase-2): record the baseline` — never as the
description.

```text
fix(pdf): cancel stale page renders
perf(virtualizer): cap retained scroll items
refactor(reader): isolate virtualizer ownership
docs: record the zoom pipeline's three scales
test(lifecycle): cover rapid reopen cycles
ci: run the lifecycle regression gate
```

### Body

Optional, and exactly one blank line after the subject. Wrap lines near 72
columns. Say why the change was made, the constraint that shaped it, and what
was checked.

### Verify, every commit

Count the complete subject — spaces and punctuation included — before
committing:

```bash
node -e 'const s=process.argv[1]; const n=[...s].length; console.log(`${n}/72`); if (n>72) process.exit(1)' "fix(pdf): cancel stale page renders"
```

Then confirm what actually landed:

```bash
git log -1 --pretty=%s
```

A commit is not correctly formatted because the command that created it looked
correct.
