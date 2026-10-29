// DTOs mirroring the GitCraft backend (backend/gitcraft/git/parsers.py).

export type RefKind = 'local' | 'remote' | 'tag' | 'head';

export interface RefLabel {
  name: string;
  kind: RefKind;
  current: boolean;
}

/** [fromLane, toLane, kind (0 full, 1 top, 2 bottom), color] */
export type GraphSegment = [number, number, number, number];

export interface GraphRow {
  col: number;
  color: number;
  segments: GraphSegment[];
  width: number;
}

export interface FileChange {
  path: string;
  status: string;
  origPath: string | null;
}

export interface LogCommit {
  sha: string;
  parents: string[];
  author: string;
  email: string;
  time: number;
  refs: RefLabel[];
  subject: string;
  graph?: GraphRow;
  file?: FileChange;
}

export interface LogResult {
  commits: LogCommit[];
  hasMore: boolean;
}

export interface CommitDetail extends LogCommit {
  committer: string;
  committerEmail: string;
  commitTime: number;
  message: string;
  files: FileChange[];
}

export type LineTag = ' ' | '+' | '-' | '\\';

export interface DiffLine {
  t: LineTag;
  text: string;
  old: number | null;
  new: number | null;
}

export interface Hunk {
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  context: string;
  lines: DiffLine[];
}

export interface FileDiff {
  header: string[];
  oldPath: string | null;
  newPath: string | null;
  status: string;
  binary: boolean;
  truncated: boolean;
  hunks: Hunk[];
}

export interface BranchInfo {
  name: string;
  sha: string;
  upstream: string | null;
  ahead: number;
  behind: number;
  upstreamGone: boolean;
  current: boolean;
}

export interface RemoteBranch {
  name: string;
  remote: string;
  branch: string;
  sha: string;
}

export interface TagInfo {
  name: string;
  sha: string;
  annotated: boolean;
  message: string;
  time: number;
}

export interface Remote {
  name: string;
  fetchUrl: string;
  pushUrl: string;
}

export interface Stash {
  name: string;
  sha: string;
  time: number;
  message: string;
}

export interface Submodule {
  sha: string;
  path: string;
  state: string;
}

export interface Refs {
  local: BranchInfo[];
  remote: RemoteBranch[];
  tags: TagInfo[];
  remotes: Remote[];
  stashes: Stash[];
  submodules: Submodule[];
  head: string | null;
}

export interface Operation {
  kind: 'merge' | 'rebase' | 'cherry-pick' | 'revert' | 'am';
  branch?: string | null;
  onto?: string | null;
  step?: number | null;
  total?: number | null;
  stoppedSha?: string | null;
  head?: string;
}

export interface BranchStatus {
  head: string | null;
  oid: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  detached: boolean;
}

export interface Status {
  branch: BranchStatus;
  staged: FileChange[];
  unstaged: FileChange[];
  conflicts: { path: string; status: string }[];
  operation: Operation | null;
}

export interface OpResult {
  ok: boolean;
  conflicts: boolean;
  message: string;
  operation?: Operation;
}

export interface RecentRepo {
  path: string;
  name: string;
  lastOpened: number;
  exists?: boolean;
}

export interface DirListing {
  path: string;
  parent: string | null;
  home: string;
  roots: string[];
  isRepo: boolean;
  dirs: { name: string; path: string; isRepo: boolean }[];
}

export interface BlameLine {
  sha: string;
  line: number;
  text: string;
  author: string;
  time: number;
  summary: string;
}

export type ConflictChunk =
  | { type: 'common'; lines: string[] }
  | { type: 'conflict'; ours: string[]; base: string[]; theirs: string[] };

export interface ConflictVersions {
  path: string;
  base: string | null;
  ours: string | null;
  theirs: string | null;
  working: string | null;
  chunks: ConflictChunk[];
  oursLabel: string;
  theirsLabel: string;
}

export type RebaseAction = 'pick' | 'reword' | 'edit' | 'squash' | 'fixup' | 'drop';
