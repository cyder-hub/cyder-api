---
name: task-generate
description: Analyze the current repository and generate a structured task document under `task/*.md` that includes a codebase-grounded analysis report and a sequential implementation task list. Use this skill whenever the user asks to analyze the code, study an existing module or requirement, discuss best practices for a subsystem, or produce a planning/analysis/task document such as pricing, retry, API key governance, routing, runtime operations, or similar architecture/work planning topics. Use it even if the user does not explicitly say "write a task doc" but clearly wants a repository-specific analysis report plus actionable tasks.
---

# Task Generate

Use this skill when the user wants a repository-specific analysis report and an implementation task list written into `task/*.md`.

The output is not casual brainstorming. It is a grounded design document derived from:

- the user request
- the repository's real code
- repository guidance files
- existing task documents when relevant
- the current product positioning and constraints

The result should be directly usable as an execution plan.

## Core workflow

Follow this sequence:

1. Read the user's request and identify the target subsystem.
2. Read repository guidance files first if present, including:
   - `AGENTS.md`
   - referenced task docs such as `task/next.md`
   - any subsystem-specific documents the user mentions
3. Inspect the relevant code paths instead of assuming architecture from memory.
4. Summarize the current implementation, strengths, gaps, and structural limitations.
5. Design the target solution using best practices.
6. Write the result into the requested `task/*.md` file.
7. If the user asks for a task list, append a sequential implementation checklist with acceptance criteria.
8. If the user says the document should be self-explanatory or the sole implementation guide, rewrite the final document as a closed design: absorb necessary context from older docs, remove dependency on them, and resolve open options into fixed rules.

Do not skip the code reading step. The document must be anchored in the actual repository.

## Code analysis requirements

Before writing, inspect the relevant code paths. At minimum, look for the pieces that matter to the subsystem:

- database schema and persistence models
- controller or API entrypoints
- service layer and business logic
- runtime/cache/state structures
- logging and observability hooks
- frontend management pages if the topic affects the admin UI
- related task docs already present in `task/`

For fragile runtime or architecture work, also inspect the exact implementation entry points that currently own behavior. Examples:

- selector functions that currently decide the main path
- logging builders that currently assemble persisted records
- request execution paths that currently send downstream traffic
- DTO builders and frontend viewers that currently shape the operator-facing contract

When analyzing, explicitly separate:

- what already exists
- what is partially implemented
- what is missing
- what should be discarded instead of extended

Do not treat the old implementation shape as a hard constraint if the user says the current module can be replaced.

## Design stance

Default to best practices, not incremental patching.

Unless the user explicitly asks for compatibility, assume:

- existing schema can be replaced
- existing APIs can be redesigned
- existing module boundaries can be redrawn
- migration compatibility is not required
- intermediate states are not required

Optimize for the final system, not the easiest partial patch.

If the user says the document will be the only development guide, do not leave unresolved branches. Pick one final design after code inspection and write it as the required implementation shape.

## Output file rules

Write the result into a file under `task/`, usually the exact path the user requested.

Examples:

- `task/price.md`
- `task/retry.md`
- `task/apikey.md`

If the user does not specify a file but clearly wants a task document, choose a sensible file name under `task/` and state it.

Use Markdown. Keep the structure readable and stable across modules.

Default to a self-contained document. The final task doc should be understandable without opening another task doc for core meaning.

If earlier task docs contain useful context, absorb the needed conclusions into the new file instead of making the new file depend on them.

For execution-oriented task docs, add a short self-explanatory maintenance contract near the top of the file. Put it after the “适用前提” block unless the user requests another location. This contract should make clear that:

- the document is a living implementation guide, not a one-time analysis memo
- when a future user asks to complete one or more tasks from the document, the executor should actually complete the specified tasks rather than only restating the plan
- after implementation, the executor must update the document with real progress and completion information
- partial completion must be marked explicitly rather than silently treated as complete
- if implementation diverges from the original plan, the document must be updated to match the landed code

For this maintenance contract, require the document to tell future executors to update at least:

- current status and overall progress near the top of the document when relevant
- the status of the affected task
- what was completed
- what verification was actually run
- notes, caveats, blockers, or plan deviations

## Document structure

Use this structure unless the user requests otherwise:

```md
# <Subsystem> 重设计建议

生成时间：YYYY-MM-DD

适用前提：

- ...

## 文档执行与维护约定

...

---

## 1. 结论先行

...

## 2. 基于当前代码的现状分析

### 2.1 当前已经具备的基础
### 2.2 当前真正缺失的部分

## 3. 为什么简单扩展现实现不是最佳实践

...

## 4. 最佳实践设计

...

## 5. 与当前项目代码的具体改造建议

...

## 6. 推荐默认策略 / 推荐默认模型

...

## 7. 最终结论

...

## 8. 可落地任务清单

...
```

Adjust section titles when the subsystem needs it, but preserve the same high-level pattern:

- execution/maintenance contract near the top for living task docs
- conclusion
- current-state analysis
- target design
- repository-specific advice
- actionable task list

## Writing requirements

The document should be:

- concrete
- repository-specific
- opinionated
- implementation-oriented
- self-contained when feasible

It should not be:

- generic architecture fluff
- a changelog of files
- a compatibility plan unless the user asked for one
- vague brainstorming without decisions

If the document is meant to guide implementation directly, prefer fixed rules over suggestion language. Replace open-ended wording such as “可以考虑”, “建议”, “可能”, “可选”, “推荐”, “首版” with explicit decisions, constraints, or banned alternatives.

When discussing current code, reference concrete module paths in prose where helpful.

Examples:

- `server/src/utils/billing.rs`
- `server/src/proxy/core.rs`
- `server/src/database/system_api_key.rs`

Explain why the current design is insufficient when recommending replacement.

## Task list rules

When the user asks for a task list, append it at the end of the document.

The task list must follow these rules:

1. Tasks are sequential and assume the user will execute them in order.
2. Tasks should target the final desired design, not transitional compromise states.
3. Each task must be reasonably sized and independently completable.
4. Each task must include:
   - numeric identifier
   - priority
   - difficulty
   - goal
   - modification entry points
   - replacement result
   - banned approaches when relevant
   - execution requirements
   - acceptance criteria
5. Number tasks in strict ascending order using `1.`, `2.`, `3.` style section titles or headings.
6. Make dependencies implicit via order; do not rely on nested dependency graphs unless necessary.
7. For tasks that touch fragile runtime behavior, specify the concrete modules or functions that should be modified first.
8. If the document is intended as the sole implementation guide, each task must make clear:
   - where to start
   - what new structure takes ownership afterward
   - which old implementation must stop being extended
   - what is explicitly forbidden

Use this task template:

```md
### 1. <任务名>

- 优先级：P0 / P1 / P2
- 难度：低 / 中 / 高
- 目标：
- 修改入口：
- 替换结果：
- 禁止项：
  1. ...
- 执行要求：
  1. ...
- 验收标准：
  1. ...
```

Use Chinese for the task body if the surrounding document is Chinese.

Do not turn the task list into a patch tutorial. The right granularity is architecture and module ownership:

- specific enough that an engineer knows where to change code and what must replace what
- not so specific that the task doc becomes a line-by-line editing script

For living implementation guides, also make the task list maintainable after execution begins. Each task should be easy to update later with real implementation progress. Prefer tasks that have a clear durable owner and a clear acceptance boundary so future executors can mark status, verification, and notes without ambiguity.

## Task splitting guidance

Split work by durable architecture boundaries, not by superficial file edits.

Good task boundaries:

- domain model definition
- schema design
- repository implementation
- core engine implementation
- main request path integration
- observability and events
- admin API redesign
- frontend admin page redesign
- testing and regression baseline
- old module removal
- replacing a weak runtime entrypoint with a new unique owner
- converting a legacy read/write contract into a new fixed contract

Bad task boundaries:

- "edit file A"
- "rename field B"
- "change frontend text"

Unless those are part of a larger meaningful unit.

## What to include in the analysis

For subsystem analysis, cover as many of these as applicable:

- current code shape
- current runtime behavior
- structural limitations
- data model limitations
- coupling problems
- missing observability
- admin UI gaps
- safety or correctness risks
- performance implications
- where the current design can be reused
- where it should be discarded

If the subsystem interacts with other major modules such as pricing, retry, routing, API keys, request logs, or provider governance, call out the boundaries explicitly.

When old modules or fields should no longer be used, say that directly. Do not merely describe the new structure; also state which old structures stop being extended.

## What to avoid

Do not:

- produce a task list without analyzing code first
- mirror the existing implementation blindly
- preserve poor names just because they already exist
- write a migration or compatibility strategy unless requested
- produce tasks that are too large to verify
- produce tasks that are too tiny to matter
- leave key architecture choices unresolved when the user asked for a final design
- hide important implementation constraints in prose outside the task list
- rely on another task doc for definitions that the current file could state directly

## Tone

Write like a senior engineer preparing a serious internal design note:

- direct
- structured
- explicit about tradeoffs
- not verbose for its own sake

The default language should match the user's language. If the user writes in Chinese, write the document in Chinese unless they request otherwise.

## Example trigger requests

Use this skill for prompts like:

- “分析一下当前价格模块，并输出一份新的 task 文档”
- “结合代码，给我一份 retry 策略分析和任务清单”
- “请基于当前仓库，设计 API Key 限速、配额、预算方案并写进 task/apikey.md”
- “参考 task/next.md 和现有代码，给出某模块最佳实践设计”

## Completion checklist

Before finishing, verify:

- the target `task/*.md` file was actually written
- the design is based on inspected code, not memory alone
- the document has both analysis and target design
- the task list is appended if requested
- task ordering is executable in sequence
- tasks optimize for the final architecture, not intermediate compatibility
- the document is self-contained for core concepts and does not require another task doc to be readable
- the task list states modification entry points, replacement boundaries, and banned alternatives for fragile tasks
- the final wording does not leave “maybe/optional/recommended” branches in sections that are meant to be implementation rules
