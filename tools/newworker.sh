#!/bin/zsh
# newworker.sh NAME : git worktree ~/work/wt/NAME on branch work/NAME from master (+ tmp dir)
N=$1; cd "$(git -C "$(dirname "$0")" rev-parse --show-toplevel)" || exit 1
git worktree add -q ~/work/wt/$N -b work/$N master 2>/dev/null || git worktree add -q ~/work/wt/$N work/$N
mkdir -p ~/work/wt/$N/tmp && echo ~/work/wt/$N
