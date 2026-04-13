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

## Output file rules

Write the result into a file under `task/`, usually the exact path the user requested.

Examples:

- `task/price.md`
- `task/retry.md`
- `task/apikey.md`

If the user does not specify a file but clearly wants a task document, choose a sensible file name under `task/` and state it.

Use Markdown. Keep the structure readable and stable across modules.

## Document structure

Use this structure unless the user requests otherwise:

```md
# <Subsystem> 重设计建议

生成时间：YYYY-MM-DD

适用前提：

- ...

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

It should not be:

- generic architecture fluff
- a changelog of files
- a compatibility plan unless the user asked for one
- vague brainstorming without decisions

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
   - concrete scope
   - acceptance criteria
5. Number tasks in strict ascending order using `1.`, `2.`, `3.` style section titles or headings.
6. Make dependencies implicit via order; do not rely on nested dependency graphs unless necessary.

Use this task template:

```md
### 1. <任务名>

- 优先级：P0 / P1 / P2
- 难度：低 / 中 / 高
- 具体内容说明：
  - ...
- 验收标准：
  - ...
```

Use Chinese for the task body if the surrounding document is Chinese.

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

## What to avoid

Do not:

- produce a task list without analyzing code first
- mirror the existing implementation blindly
- preserve poor names just because they already exist
- write a migration or compatibility strategy unless requested
- produce tasks that are too large to verify
- produce tasks that are too tiny to matter

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
