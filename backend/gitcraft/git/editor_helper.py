"""Non-interactive editor used by GitCraft for GIT_EDITOR and GIT_SEQUENCE_EDITOR.

Usage (git appends the file to edit as the last argument):
    editor_helper.py todo [prepared-todo-file] <git-todo-file>
    editor_helper.py msg <git-message-file>

`todo` replaces the rebase todo list with a prepared one (or accepts it as-is).
`msg` accepts the proposed message, unless an interactive rebase started by
GitCraft left a message for the commit currently being reworded/squashed.

This file is executed as a standalone script and must not import gitcraft.
"""

import json
import os
import shutil
import sys

MESSAGES_FILE = "gitcraft-rebase-messages.json"


def rewrite_message(target: str) -> None:
    git_dir = os.path.dirname(os.path.abspath(target))
    messages_path = os.path.join(git_dir, MESSAGES_FILE)
    done_path = os.path.join(git_dir, "rebase-merge", "done")
    if not (os.path.exists(messages_path) and os.path.exists(done_path)):
        return
    with open(done_path, encoding="utf-8") as handle:
        steps = [line.split() for line in handle if line.strip() and not line.startswith("#")]
    if not steps or len(steps[-1]) < 2:
        return
    sha = steps[-1][1]
    with open(messages_path, encoding="utf-8") as handle:
        messages = json.load(handle)
    for key, message in messages.items():
        if key.startswith(sha) or sha.startswith(key):
            with open(target, "w", encoding="utf-8") as handle:
                handle.write(message.rstrip("\n") + "\n")
            return


def main(argv: list[str]) -> int:
    if len(argv) < 3:
        return 0
    mode, target = argv[1], argv[-1]
    if mode == "todo" and len(argv) == 4:
        shutil.copyfile(argv[2], target)
    elif mode == "msg":
        rewrite_message(target)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
