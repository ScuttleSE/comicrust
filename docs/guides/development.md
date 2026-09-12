# Guide: development

## When you set up a clone

The project has two forges. Gitea (`origin`) runs CI. GitHub
(`ScuttleSE/comicrust`) is the public mirror and holds the releases.
One `git push` must reach both. Configure the clone with both commands:

```sh
git remote set-url --add --push origin \
    git@github.com:ScuttleSE/comicrust.git
git remote set-url --add --push origin \
    ssh://git@git.hemmalab.se:2222/scuttle/comicrust.git
```

Add both URLs. The first `--add --push` replaces the implicit default
push URL. If you add only the GitHub URL, all pushes go to GitHub and
none go to Gitea.

Confirm the result with `git config --get-all remote.origin.pushurl`.
It must print two lines, GitHub first. GitHub must be first, because the
push to Gitea starts the release workflow.

This configuration is local to the clone. Git does not keep it in the
repository. If you push from a clone without it, the mirror falls
behind and the release workflow fails. See
`.gitea/publish_github_release.sh`.

## Before you write code

1. Find and read the C# source for the behavior. The reference is the
   specification. Never guess from a name, a screenshot, or memory.
2. Read the ADRs that the active phase file links to.
3. Confirm the task is in the active phase file. If it is not, ask before
   you start.

The reference is decompiled. Expect dead code, odd names, and swallowed
exceptions. Port the behavior, not the style.

## While you write code

- Make the smallest correct change.
- Do not add a compatibility path without a concrete, measured need.
- Preserve unrelated changes in the working tree. Check `git status` before
  you edit.
- Reflection by property name is load-bearing in the C#. The port uses an
  explicit property registry in `cr-core`. Add to the registry instead of
  adding a parallel lookup.
- Be lenient when you load user data. Windows paths are baked into it.

## Before you commit

1. Run the three required commands in `docs/guides/verification.md`.
2. Check `git status` and stage only the intended files.
3. Confirm no user library data is staged. See
   `docs/guides/data-safety.md`.
4. Write an imperative, concise subject line.
5. Commit, then push. A push starts CI. The push must also reach the
   GitHub mirror. See "When you set up a clone".

## When a change fixes a defect

Prove the fix against the measured problem, not against the build.

The strongest form is a verify-both-ways measurement:

1. Show the gate FAILS on a build with the fix neutralized.
2. Show the gate PASSES with the fix in place.
3. Record both results.

"It builds" is not evidence.

## When you finish a session

Update `docs/current-status.md`. Replace the stale content. The next agent
must be able to read that one file and know the exact state.
