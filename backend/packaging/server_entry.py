"""Entry point of the bundled (PyInstaller) GitCraft server used by the desktop app."""

import sys

if __name__ == "__main__":
    # Git invokes the bundled executable as its editor during interactive rebases;
    # handle that before importing the (comparatively slow) web stack.
    if len(sys.argv) > 1 and sys.argv[1] == "--gitcraft-editor":
        from gitcraft.git.editor_helper import main as editor_main

        sys.exit(editor_main([sys.argv[0], *sys.argv[2:]]))

    from gitcraft.app import main

    main()
