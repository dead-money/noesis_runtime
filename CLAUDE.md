## Attribution

Do not add `Co-Authored-By: Claude` trailers to commits or "Generated with Claude Code" footers to PR bodi
es. Author lines and PR bodies stay clean. `scripts/git-hooks/commit-msg` strips the
se defensively; activate per clone with `git config core.hooksPath scripts/git-hooks`. Do not work around
the hook.
