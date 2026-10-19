# Task handoff

Keep one current handoff in the task directory. Link dated evidence instead of embedding old logs or checkpoints.

```text
Branch and commit: <branch; exact tip; pushed/local state; PR if present>
Completed work: <result against the task's stopping condition>
Changed files or units: <owned paths and any coordinated shared change>
Validation and evidence: <commands, exit statuses, outcomes, and log paths>
Blockers and restrictions: <remaining failures, unresolved decisions, user stops>
Next action: <one concrete next step, or done>
Worktree and evidence retention: <retain/release; evidence destination>
```

Remove only this task's worktree after its commits are confirmed on the remote and its evidence is preserved. Leave other phase and step worktrees alone.

Before `/compact` or `/clear`, save the handoff. After compacting, check the branch and commit before resuming. Recorded permissions and old next actions do not supersede the current user request.
