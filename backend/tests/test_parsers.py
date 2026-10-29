from gitcraft.git import graph, parsers


def test_parse_status_v2():
    out = "\0".join(
        [
            "# branch.oid abc",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
            "1 M. N... 100644 100644 100644 aaa bbb staged.txt",
            "1 .M N... 100644 100644 100644 aaa bbb dirty file.txt",
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.txt",
            "old.txt",
            "u UU N... 100644 100644 100644 100644 a b c conflict.txt",
            "? untracked.txt",
            "",
        ]
    )
    status = parsers.parse_status(out)
    assert status["branch"] == {
        "head": "main",
        "oid": "abc",
        "upstream": "origin/main",
        "ahead": 2,
        "behind": 1,
        "detached": False,
    }
    assert [e["path"] for e in status["staged"]] == ["staged.txt", "new.txt"]
    assert status["staged"][1]["origPath"] == "old.txt"
    assert [e["path"] for e in status["unstaged"]] == ["dirty file.txt", "untracked.txt"]
    assert status["conflicts"] == [{"path": "conflict.txt", "status": "UU"}]


DIFF = """diff --git a/f.txt b/f.txt
index 1111111..2222222 100644
--- a/f.txt
+++ b/f.txt
@@ -1,3 +1,3 @@ ctx
 one
-two
+TWO
 three
"""


def test_parse_diff_and_line_numbers():
    [f] = parsers.parse_diff(DIFF)
    assert f["oldPath"] == f["newPath"] == "f.txt"
    [hunk] = f["hunks"]
    assert hunk["context"] == "ctx"
    assert [(l["t"], l["old"], l["new"]) for l in hunk["lines"]] == [
        (" ", 1, 1),
        ("-", 2, None),
        ("+", None, 2),
        (" ", 3, 3),
    ]


def test_partial_patch_forward_and_reverse():
    [f] = parsers.parse_diff(DIFF)
    # Stage only the addition: the removal becomes context.
    forward = parsers.build_partial_patch(f, {0: {2}}, reverse=False)
    assert "@@ -1,3 +1,4 @@\n one\n two\n+TWO\n three\n" in forward
    # Reverse with only the removal selected: the addition becomes context.
    reverse = parsers.build_partial_patch(f, {0: {1}}, reverse=True)
    assert "@@ -1,4 +1,3 @@\n one\n-two\n TWO\n three\n" in reverse


def test_parse_decorations():
    refs = parsers.parse_decorations("HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1")
    assert refs == [
        {"name": "main", "kind": "local", "current": True},
        {"name": "origin/main", "kind": "remote", "current": False},
        {"name": "v1", "kind": "tag", "current": False},
    ]


def test_conflict_markers():
    text = "a\n<<<<<<< HEAD\nours\n||||||| base\norig\n=======\ntheirs\n>>>>>>> branch\nz\n"
    chunks = parsers.parse_conflict_markers(text)
    assert chunks[0] == {"type": "common", "lines": ["a"]}
    assert chunks[1] == {"type": "conflict", "ours": ["ours"], "base": ["orig"], "theirs": ["theirs"]}
    assert chunks[2] == {"type": "common", "lines": ["z", ""]}


def test_untracked_diff():
    d = parsers.untracked_diff("n.txt", b"x\ny")
    assert [l["t"] for l in d["hunks"][0]["lines"]] == ["+", "+", "\\"]
    assert parsers.untracked_diff("b.bin", b"\0\1")["binary"]


def c(sha, *parents):
    return {"sha": sha, "parents": list(parents)}


def test_graph_linear():
    rows = graph.layout([c("c", "b"), c("b", "a"), c("a")])
    assert [r["col"] for r in rows] == [0, 0, 0]
    assert rows[0]["segments"] == [[0, 0, graph.BOTTOM, 0]]
    assert rows[1]["segments"] == [[0, 0, graph.TOP, 0], [0, 0, graph.BOTTOM, 0]]
    assert rows[2]["segments"] == [[0, 0, graph.TOP, 0]]


def test_graph_merge():
    # m merges b2 (feature) into a2 (main); both descend from a1.
    rows = graph.layout([c("m", "a2", "b2"), c("b2", "a1"), c("a2", "a1"), c("a1")])
    assert rows[0]["col"] == 0
    assert [0, 1, graph.BOTTOM, rows[0]["segments"][1][3]] in rows[0]["segments"]
    assert rows[1]["col"] == 1
    assert rows[2]["col"] == 0
    # a1 is expected by both lanes, the second lane joins into lane 0.
    assert rows[3]["col"] == 0
    assert any(s[0] == 1 and s[1] == 0 and s[2] == graph.TOP for s in rows[3]["segments"])
    assert max(r["width"] for r in rows) == 2
