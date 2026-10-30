//! Lane layout for the commit graph.
//!
//! Commits must be pushed in topological order (children before parents). Each
//! row gets the lane of its commit dot plus the line segments to draw in it:
//!
//! - [`FULL`]: the lane passes straight through the row
//! - [`TOP`]: a line from lane `from` at the top edge into the commit dot
//! - [`BOTTOM`]: a line from the commit dot to lane `to` at the bottom edge
//!
//! Layout is incremental, so a further page of history can continue from the
//! state the previous page left behind.

use serde::{Deserialize, Serialize};

pub const FULL: u8 = 0;
pub const TOP: u8 = 1;
pub const BOTTOM: u8 = 2;
pub const PALETTE_SIZE: u32 = 8;

/// `[from, to, kind, color]`, in the order they are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment(pub u32, pub u32, pub u8, pub u8);

impl Segment {
    pub fn from(&self) -> u32 {
        self.0
    }
    pub fn to(&self) -> u32 {
        self.1
    }
    pub fn kind(&self) -> u8 {
        self.2
    }
    pub fn color(&self) -> u8 {
        self.3
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRow {
    pub col: u32,
    pub color: u8,
    pub segments: Vec<Segment>,
    pub width: u32,
}

/// Lane state between commits. `Id` is whatever identifies a commit: a sha, or an interned number.
#[derive(Debug, Clone)]
pub struct Layout<Id> {
    lanes: Vec<Option<Id>>,
    colors: Vec<u8>,
    next_color: u32,
}

impl<Id> Default for Layout<Id> {
    fn default() -> Self {
        Self {
            lanes: Vec::new(),
            colors: Vec::new(),
            next_color: 0,
        }
    }
}

impl<Id: Eq + Clone> Layout<Id> {
    pub fn new() -> Self {
        Self::default()
    }

    fn new_color(&mut self) -> u8 {
        let color = self.next_color % PALETTE_SIZE;
        self.next_color += 1;
        color as u8
    }

    fn free_slot(&mut self, exclude: Option<usize>) -> usize {
        let free = self
            .lanes
            .iter()
            .enumerate()
            .find(|(index, value)| value.is_none() && Some(*index) != exclude);
        match free {
            Some((index, _)) => index,
            None => {
                self.lanes.push(None);
                self.colors.push(0);
                self.lanes.len() - 1
            }
        }
    }

    pub fn push(&mut self, sha: &Id, parents: &[Id]) -> GraphRow {
        let mut segments = Vec::new();
        let hits: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, value)| value.as_ref() == Some(sha))
            .map(|(index, _)| index)
            .collect();
        let col = match hits.first() {
            Some(&first) => first,
            None => {
                let slot = self.free_slot(None);
                self.lanes[slot] = Some(sha.clone());
                self.colors[slot] = self.new_color();
                slot
            }
        };
        let color = self.colors[col];

        for (index, value) in self.lanes.iter().enumerate() {
            let Some(value) = value else { continue };
            if value == sha {
                if !hits.is_empty() {
                    segments.push(Segment(index as u32, col as u32, TOP, self.colors[index]));
                }
            } else {
                segments.push(Segment(
                    index as u32,
                    index as u32,
                    FULL,
                    self.colors[index],
                ));
            }
        }

        for &index in hits.iter().skip(1) {
            self.lanes[index] = None;
        }

        if let Some((first, rest)) = parents.split_first() {
            self.lanes[col] = Some(first.clone());
            segments.push(Segment(col as u32, col as u32, BOTTOM, color));
            for parent in rest {
                let existing = self.lanes.iter().position(|v| v.as_ref() == Some(parent));
                let target = match existing {
                    Some(target) => target,
                    None => {
                        let target = self.free_slot(Some(col));
                        self.lanes[target] = Some(parent.clone());
                        self.colors[target] = self.new_color();
                        target
                    }
                };
                segments.push(Segment(
                    col as u32,
                    target as u32,
                    BOTTOM,
                    self.colors[target],
                ));
            }
        } else {
            self.lanes[col] = None;
        }

        while matches!(self.lanes.last(), Some(None)) {
            self.lanes.pop();
            self.colors.pop();
        }

        let width = segments
            .iter()
            .map(|s| s.0.max(s.1))
            .fold(col as u32, u32::max)
            + 1;
        GraphRow {
            col: col as u32,
            color,
            segments,
            width,
        }
    }
}

/// Lays out a whole list at once.
pub fn layout<Id: Eq + Clone>(commits: &[(Id, Vec<Id>)]) -> Vec<GraphRow> {
    let mut state = Layout::new();
    commits
        .iter()
        .map(|(sha, parents)| state.push(sha, parents))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(sha: &'static str, parents: &[&'static str]) -> (&'static str, Vec<&'static str>) {
        (sha, parents.to_vec())
    }

    #[test]
    fn linear() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(rows.iter().all(|r| r.col == 0));
        assert_eq!(rows[0].segments, [Segment(0, 0, BOTTOM, 0)]);
        assert_eq!(
            rows[1].segments,
            [Segment(0, 0, TOP, 0), Segment(0, 0, BOTTOM, 0)]
        );
        assert_eq!(rows[2].segments, [Segment(0, 0, TOP, 0)]);
    }

    #[test]
    fn merge() {
        // m merges b2 (feature) into a2 (main); both descend from a1.
        let rows = layout(&[
            c("m", &["a2", "b2"]),
            c("b2", &["a1"]),
            c("a2", &["a1"]),
            c("a1", &[]),
        ]);
        assert_eq!(rows[0].col, 0);
        let merge_color = rows[0].segments[1].color();
        assert!(
            rows[0]
                .segments
                .contains(&Segment(0, 1, BOTTOM, merge_color))
        );
        assert_eq!(rows[1].col, 1);
        assert_eq!(rows[2].col, 0);
        // a1 is expected by both lanes, the second lane joins into lane 0.
        assert_eq!(rows[3].col, 0);
        assert!(
            rows[3]
                .segments
                .iter()
                .any(|s| s.from() == 1 && s.to() == 0 && s.kind() == TOP)
        );
        assert_eq!(rows.iter().map(|r| r.width).max(), Some(2));
    }

    /// Every /log golden: the same parents must produce the same rows as when they were recorded.
    #[test]
    fn matches_goldens() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens");
        let mut checked = 0;
        for entry in std::fs::read_dir(dir).expect("goldens exist") {
            let path = entry.unwrap().path();
            let golden: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            for exchange in golden["exchanges"].as_array().unwrap() {
                if exchange["request"]["path"] != "/log" {
                    continue;
                }
                let Some(commits) = exchange["response"]["body"]["commits"].as_array() else {
                    continue;
                };
                let list: Vec<(String, Vec<String>)> = commits
                    .iter()
                    .map(|c| {
                        let parents = c["parents"].as_array().unwrap();
                        (
                            c["sha"].as_str().unwrap().to_owned(),
                            parents
                                .iter()
                                .map(|p| p.as_str().unwrap().to_owned())
                                .collect(),
                        )
                    })
                    .collect();
                for (row, commit) in layout(&list).iter().zip(commits) {
                    let expected: GraphRow =
                        serde_json::from_value(commit["graph"].clone()).unwrap();
                    assert_eq!(row, &expected, "{} {}", path.display(), commit["sha"]);
                    checked += 1;
                }
            }
        }
        assert!(checked > 250, "only {checked} rows checked");
    }
}
