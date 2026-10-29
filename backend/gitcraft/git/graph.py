"""Lane layout for the commit graph.

Commits must be in topological order (children before parents). Each row gets
the lane of its commit dot plus the line segments to draw in that row:

    FULL   lane passes straight through the row (top to bottom)
    TOP    line from lane `from` at the top edge into the commit dot
    BOTTOM line from the commit dot to lane `to` at the bottom edge

Segments are emitted as [from, to, kind, color].
"""

from __future__ import annotations

from typing import Any

FULL, TOP, BOTTOM = 0, 1, 2
PALETTE_SIZE = 8


def _free_slot(lanes: list[str | None], exclude: int = -1) -> int:
    for index, value in enumerate(lanes):
        if value is None and index != exclude:
            return index
    lanes.append(None)
    return len(lanes) - 1


def layout(commits: list[dict[str, Any]]) -> list[dict[str, Any]]:
    lanes: list[str | None] = []
    colors: list[int] = []
    next_color = 0
    rows = []

    def new_color() -> int:
        nonlocal next_color
        color = next_color % PALETTE_SIZE
        next_color += 1
        return color

    def set_lane(index: int, sha: str | None, color: int | None = None) -> None:
        while len(colors) < len(lanes):
            colors.append(0)
        lanes[index] = sha
        if color is not None:
            colors[index] = color

    for commit in commits:
        sha = commit["sha"]
        segments: list[list[int]] = []
        hits = [i for i, value in enumerate(lanes) if value == sha]
        if hits:
            col = hits[0]
        else:
            col = _free_slot(lanes)
            set_lane(col, sha, new_color())
        color = colors[col]

        for index, value in enumerate(lanes):
            if value is None:
                continue
            if value == sha:
                if hits:
                    segments.append([index, col, TOP, colors[index]])
            else:
                segments.append([index, index, FULL, colors[index]])

        for index in hits[1:]:
            set_lane(index, None)

        parents = commit["parents"]
        if parents:
            set_lane(col, parents[0])
            segments.append([col, col, BOTTOM, color])
            for parent in parents[1:]:
                target = next((i for i, value in enumerate(lanes) if value == parent), None)
                if target is None:
                    target = _free_slot(lanes, exclude=col)
                    set_lane(target, parent, new_color())
                segments.append([col, target, BOTTOM, colors[target]])
        else:
            set_lane(col, None)

        while lanes and lanes[-1] is None:
            lanes.pop()
            colors.pop()

        width = max([col] + [max(s[0], s[1]) for s in segments]) + 1
        rows.append({"col": col, "color": color, "segments": segments, "width": width})
    return rows
