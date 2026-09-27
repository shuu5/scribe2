//! 直しの束（設計 docs/design/dispatcher.md §27 形 1〜4・契約表の行 y）: 事前審査の確定の finding を根（断りの名と在り処）で
//! 束ね、束ごとの 1 file を事前審査の dir の下の `bundle/` に置く。束の id は根の字の digest で決まる（同じ根は周をまたいで
//! 同じ束・初めて見た時刻を持ち越す）。確定の消えた束の file は同じ周に外す。
//!
//! 読む側は 3 つ: 終端の周の知らせ（[`changed`]・束の集合が前に送った集合と違う周だけ）・`dispatch ls` の行（[`lines`]）・
//! idle の知らせと heartbeat の末尾と段の上げ（[`tally`]・台帳を読まず置き場の file だけ）。結果の file の読み手は事前審査の
//! [`read`] の 1 本（2 本目を書かない・C2）。event kind は足さない。
//!
//! 歯は e2e（`crates/scribe2-boundary/tests/e2e/notify.rs` の `pipe_notify_precheck_` と `seat/tick.rs` の
//! `seat_tick_precheck_`）が外形で測る——新設の module に in-file の歯を置くと、base に `mod` 宣言ごと無く flip-check が断る。

use super::super::review::design_material;
use super::super::table::{self, find_row, Pointer, END};
use super::facts::Precheck;
use super::precheck::{dir_of, read, Population};
use super::Input;
use crate::hook::vessel::digest::fnv1a_64;
use crate::name::NAME;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// 束の dir の名（事前審査の dir の下）。
const BUNDLE_DIR: &str = "bundle";

/// 前に送った束の集合の印（束の dir の下・`.` を含む名は束でない）。
const SENT: &str = ".sent";

/// `dispatch ls` の束の行の書き出し（形 2）。
const LINE: &str = "[DISPATCH-BUNDLE]";

/// 束の file の本文の区切り（見出しの行はこの前だけ読む）。
const PART: &str = "== ";

/// 書きかけの結果の file の接尾辞（事前審査の書きの一時 file・数えない）。
const TEMPORARY: &str = ".tmp";

/// 束に当たった行 1 つ。
struct Hit<'a> {
    /// bead id。
    bead: &'a str,
    /// 行の設計 pointer。
    pointer: &'a Pointer,
    /// 確定の finding の理由の 1 行。
    reason: String,
}

/// 束の file の見出し（根の名・初めて見た時刻・当たった行の bead）。
struct Header {
    /// 根の断りの名。
    root: String,
    /// 初めて見た周の時刻（UTC 秒）。
    first: Option<u64>,
    /// 当たった行の bead。
    beads: Vec<String>,
}

/// 根の字から束の id を決める（断りの名と在り処の digest・同じ根は周をまたいで同じ id）。
fn id_of(name: &str, at: &str) -> String {
    fnv1a_64(format!("{name} {at}").as_bytes())
}

/// 周の終わりの束ね（形 1）: 依存待ちの行の結果の file の確定の finding を根で束ね、束ごとに 1 file を一時 file → rename で書き、
/// 確定の消えた束の file を外す。束が 1 つも無い周は dir を作らない。書けない周は黙る（次の周に書き直す）。
pub(super) fn round(input: &Input<'_>, dir: &Path, population: &Population, waiting: &[&str]) {
    let mut roots: BTreeMap<(String, String), Vec<Hit<'_>>> = BTreeMap::new();
    for &bead in waiting {
        let (Some(row), Some(kept)) = (population.rows.get(bead), read(&dir.join(bead))) else {
            continue;
        };
        for (root, reason) in kept.firm {
            roots.entry(root).or_default().push(Hit { bead, pointer: &row.pointer, reason });
        }
    }
    let place = dir.join(BUNDLE_DIR);
    let ids: BTreeSet<String> = roots.keys().map(|(name, at)| id_of(name, at)).collect();
    for (id, path) in listed(&place) {
        if !ids.contains(&id) {
            let _ = std::fs::remove_file(path);
        }
    }
    if roots.is_empty() || std::fs::create_dir_all(&place).is_err() {
        return;
    }
    let now = crate::seat::state::now_secs();
    for ((name, at), hits) in &roots {
        let id = id_of(name, at);
        let path = place.join(&id);
        let first = header(&path).and_then(|found| found.first).unwrap_or(now);
        let temporary = place.join(format!("{id}.{}{TEMPORARY}", std::process::id()));
        if std::fs::write(&temporary, body_of(input, (&id, name, at), first, hits)).is_err() {
            continue;
        }
        if std::fs::rename(&temporary, &path).is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

/// 束の file の本文: 見出し（id・根・初めて見た時刻・行ごとの pointer と finding と測り直しの argv）の後ろに、行ごとの TOML の写しと
/// 節の本文（審査の材料と同じ [`design_material`]）を `== ` の区切りで並べる。
fn body_of(input: &Input<'_>, (id, name, at): (&str, &str, &str), first: u64, hits: &[Hit<'_>]) -> String {
    let mut head = format!("id={id}\nroot={name} at={at}\nfirst_seen={first}\n");
    let mut parts = String::new();
    for hit in hits {
        let design = format!("{}#{}", hit.pointer.path, hit.pointer.id);
        let (bead, repo) = (hit.bead, input.repo.display());
        head.push_str(&format!("row={bead} pointer={design}\nfinding={bead} name={name} at={at} reason={}\n", hit.reason));
        head.push_str(&format!("remeasure={bead} argv={NAME} pipe preflight --design {design} --bead {bead} --repo {repo}\n"));
        parts.push_str(&format!("{PART}toml {bead}\n{}\n", toml_of(input.repo, hit.pointer)));
        parts.push_str(&format!("{PART}section {bead}\n{}", design_material(input.repo, &design)));
    }
    format!("{head}{parts}")
}

/// 行の TOML の写し（[`find_row`] の行番号の `[[contract]]` から次の `[[contract]]` か区間の終わりの前まで・組み直さない）。
fn toml_of(repo: &Path, pointer: &Pointer) -> String {
    let Ok(text) = table::read(repo, &pointer.path) else {
        return format!("（{} を読めない）", pointer.path);
    };
    let Ok(row) = find_row(&pointer.path, &text, &pointer.id) else {
        return format!("（契約表の行 {} を読めない）", pointer.id);
    };
    let mut lines = text.lines().skip(usize::try_from(row.line).unwrap_or(usize::MAX).saturating_sub(1));
    let mut kept: Vec<&str> = lines.next().into_iter().collect();
    kept.extend(lines.take_while(|line| !line.trim_start().starts_with("[[contract]]") && line.trim() != END));
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    kept.join("\n")
}

/// 束の file の見出しを読む（区切りの前の行だけ・読めない file は `None`）。
fn header(path: &Path) -> Option<Header> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut found = Header { root: String::new(), first: None, beads: Vec::new() };
    for line in text.lines().take_while(|line| !line.starts_with(PART)) {
        if let Some(root) = line.strip_prefix("root=") {
            found.root = root.split_once(" at=").map_or(root, |(name, _)| name).to_owned();
        } else if let Some(first) = line.strip_prefix("first_seen=") {
            found.first = first.trim().parse().ok();
        } else if let Some(row) = line.strip_prefix("row=") {
            found.beads.push(row.split_once(' ').map_or(row, |(bead, _)| bead).to_owned());
        }
    }
    Some(found)
}

/// 束の file の列（(id, path)・id の順・`.` を含む名〔送った集合の印と一時 file〕は束でない）。
fn listed(place: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(place) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            (!name.contains('.')).then(|| (name, std::path::absolute(entry.path()).unwrap_or_else(|_| entry.path())))
        })
        .collect();
    found.sort();
    found
}

/// `dispatch ls` の束の行（束ごとに `[DISPATCH-BUNDLE] id=<束> rows=<m> root=<名> file=<path>`・file を読むだけ・形 2）。
pub(super) fn lines(state_dir: &Path) -> Vec<String> {
    let bundles = listed(&dir_of(state_dir).join(BUNDLE_DIR));
    let line = |(id, path): (String, PathBuf)| {
        let found = header(&path)?;
        Some(format!("{LINE} id={id} rows={} root={} file={}", found.beads.len(), found.root, path.display()))
    };
    bundles.into_iter().filter_map(line).collect()
}

/// 束の集合が前に送った集合と違う周だけ、束の (id, path) の列と当たった行の数を返し、送った集合の印を書き換える（形 2・束が 0 本に
/// なった周も違う周に数える）。事前審査の dir の無い置き場・集合が同じ周・印を書けない周は `None`（同じ 1 行を毎周送らない）。
pub(crate) fn changed(state_dir: &Path) -> Option<(Vec<(String, PathBuf)>, usize)> {
    let dir = dir_of(state_dir);
    if !dir.is_dir() {
        return None;
    }
    let place = dir.join(BUNDLE_DIR);
    let bundles = listed(&place);
    let set: Vec<&str> = bundles.iter().map(|(id, _)| id.as_str()).collect();
    let sent = place.join(SENT);
    if std::fs::read_to_string(&sent).unwrap_or_default().trim_end() == set.join(",") {
        return None;
    }
    std::fs::create_dir_all(&place).ok()?;
    std::fs::write(&sent, format!("{}\n", set.join(","))).ok()?;
    let rows: BTreeSet<String> = bundles.iter().filter_map(|(_, path)| header(path)).flat_map(|found| found.beads).collect();
    Some((bundles, rows.len()))
}

/// 置き場の事前審査の本数（確定を持つ行・結果を持つ行・束・最も古い束の初めて見た時刻）。事前審査の dir の無い置き場は `None`
/// （測っていないを 0 に畳まない・形 3）。台帳は読まない。
pub(super) fn tally(state_dir: &Path) -> Option<Precheck> {
    let dir = dir_of(state_dir);
    let entries = std::fs::read_dir(&dir).ok()?;
    let (mut firm, mut results) = (0, 0);
    for entry in entries.flatten().filter(|entry| entry.path().is_file()) {
        if entry.file_name().to_string_lossy().ends_with(TEMPORARY) {
            continue;
        }
        if let Some(kept) = read(&entry.path()) {
            results += 1;
            firm += usize::from(!kept.firm.is_empty());
        }
    }
    let bundles = listed(&dir.join(BUNDLE_DIR));
    let oldest = bundles.iter().filter_map(|(_, path)| header(path)?.first).min();
    Some(Precheck { firm, results, bundles: bundles.len(), oldest })
}
