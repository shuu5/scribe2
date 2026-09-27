//! 事前審査の先撃ち（設計 docs/design/dispatcher.md §27 形 aa 1〜4・契約表の行 aa）: 事前審査が clean の待ち行に、予想の base を
//! 一時の worktree に実体化して Reviewed の段と同じ組み手（[`review::stage`]）で材料を組み、lens を裏で起こす。起こした周は
//! 終わりを待たず、次の起こす側の周が Reviewed の段と同じ読み手（[`review::outcome_of`]）で判定を読み、FAIL / INCONCLUSIVE で
//! 理由の型を持つ周だけ確定の finding（在り処 [`AT`]）を事前審査の結果に載せる。
//!
//! 置き場は事前審査の dir の下の `lens/<bead>/`（材料の鍵 `key`・起こした時の鍵 `fired`・lens の cmd の字 `lens`・撃ち中の印
//! `pid`・`rc`・`out`・理由 `unbuilt`・材料の dir・一時の worktree `tree`）。1 周に起こす本数は rules 行 [`ROW`] で、撃ち中の行を
//! 含めて数える（材料の組み直しは上限の外）。予想は通行証にしない（結果は `WaitReason`・起こす判定・受付のどれも読まない）。
//!
//! 歯は e2e（`crates/scribe2-boundary/tests/e2e/pipe/review.rs` の `pipe_prelens_`）が外形で測る——新設の module に in-file の歯を
//! 置くと、base に `mod` 宣言ごと無く flip-check が断る。

use super::super::contract::Contract;
use super::super::declaration::{table_facts, Ceiling, CEILING_ROW, DENIED_ROW};
use super::super::gate::Verdict;
use super::super::refuse::{normalize, Certainty, DELETE_FILE, NEW_FILE};
use super::super::review::{self, FindingKind, REVIEW_DIR};
use super::super::{git_ok, CONTRACT_FILE};
use super::precheck::{dir_of, write, Finding, Kept, Layer, Population};
use super::Input;
use crate::fleet::store::{lock_owner, started_ms, Owner};
use crate::hook::vessel::digest::fnv1a_64;
use crate::invocation::Invocation;
use crate::rules::manifest::Manifest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// 先撃ちの確定の finding の在り処（行 x の 4 値の外の語・同じ理由の型の先撃ちは行をまたいで 1 束になる・形 aa 4）。
pub(super) const AT: &str = "prelens";

/// 1 周に起こす本数の上限の rules 行（形 aa 1・任意の行）。
const ROW: &str = "pipe.precheck_lens_per_round";

/// 置き場の dir の名（事前審査の dir の下・bead ごとの dir）。
const LENS_DIR: &str = "lens";

/// 一時の worktree の名（置き場の下）。
const TREE: &str = "tree";

/// 材料の鍵の file（1 行目は事前審査の鍵・2 行目は材料の dir の鍵）。
const KEY: &str = "key";

/// lens を起こした時の材料の鍵の file。
const FIRED: &str = "fired";

/// 起こした lens の cmd の字の file。
const LENS: &str = "lens";

/// 撃ち中の印の file（`<pid> <起動時刻>`）。
const PID: &str = "pid";

/// lens の rc の file。
const RC: &str = "rc";

/// lens の stdout の file（在れば終わった）。
const OUT: &str = "out";

/// 書きかけの stdout（終わった後に [`OUT`] へ rename する）。
const PARTIAL: &str = "out.partial";

/// 契約の写しの元の file（予想の base で組んだ契約の本文）。
const SOURCE: &str = "contract.source";

/// 組めない・起こせない周の理由の file。
const UNBUILT: &str = "unbuilt";

/// 予想の印の 1 行（宣言だけの祖先を持つ行の設計の材料の末尾・形 aa 1）。
const NOTE: &str = "予想の base: 次の file は未着地の祖先の宣言で、本文を空で置いた";

/// 席の pane の変数（先撃ちの lens は起こす側の環境を継承し、これだけを外す・形 aa 1）。
const PANE_ENV: &str = "TMUX_PANE";

/// lens を包む shell の 1 行（`$0` = lens の行・`$1` = 書きかけ・`$2` = rc・`$3` = out）: rc を置いてから out を rename する。
const WRAP: &str = r#"sh -c "$0" >"$1" 2>/dev/null </dev/null; echo $? >"$2"; mv "$1" "$3""#;

/// 先撃ちの予想（precheck の口が返す・層は依存の順・契約は予想の base で組んだ 1 本とその本文）。
pub(super) struct Forecast {
    /// 祖先の層（依存の順）。
    pub(super) layers: Vec<Layer>,
    /// 予想の base で組んだ契約。
    pub(super) contract: Contract,
    /// その契約の本文（材料の dir に写す）。
    pub(super) body: String,
}

/// 起こす側の周の先撃ちの材料（事前審査の同じ周の読みを借りる）。
pub(super) struct Sweep<'a, 'b> {
    /// 列の材料。
    pub(super) input: &'a Input<'b>,
    /// 事前審査の dir。
    pub(super) dir: &'a Path,
    /// 母集団（置き場の寿命）。
    pub(super) population: &'a Population,
    /// 依存待ちの行（列の順）。
    pub(super) waiting: &'a [&'a str],
    /// 同じ周に事前審査が書き直した行の、書き直す前の結果（形 aa 4 の前の結果）。
    pub(super) before: BTreeMap<&'a str, Option<Kept>>,
}

/// 先撃ちの 1 周（形 aa 1〜4）: 母集団を出た bead の置き場と落ちた周の worktree の残りを外し、clean の行ごとに材料を組み直し・
/// 判定を結果に写し、上限の空きの分だけ lens を裏で起こす。組み直しと写し直しは撃つ条件に依らず毎周撃ち、行が無い・読めない・
/// 値 0・`--lens` の無い周は起こさない（上限の空きを 0 とする）。
pub(super) fn round(sweep: &Sweep<'_, '_>, forecast: &mut dyn FnMut(&str) -> Option<Forecast>) {
    let root = sweep.dir.join(LENS_DIR);
    prune(sweep.input.repo, &root, sweep.population);
    let flying = sweep.population.rows.keys().filter(|bead| in_flight(&root.join(bead))).count();
    let cmd = sweep.input.lens.unwrap_or_default();
    let limit = limit_of(sweep.input.manifest).filter(|_| sweep.input.lens.is_some()).unwrap_or_default();
    let mut room = usize::try_from(limit).unwrap_or(usize::MAX).saturating_sub(flying);
    for &bead in sweep.waiting {
        let (place, Some(kept)) = (root.join(bead), super::precheck::read(&sweep.dir.join(bead))) else {
            continue;
        };
        if !kept.clean || in_flight(&place) {
            continue;
        }
        if line_of(&place, KEY, 0).as_deref() != Some(kept.key.as_str()) {
            rebuild(sweep.input, &place, &kept.key, forecast(bead));
        }
        carry(sweep, bead, &place, &kept);
        if room > 0 && ready(&place, &kept.key) && fire(sweep.input, &place, cmd) {
            room = room.saturating_sub(1);
        }
    }
}

/// 上限の値（行が無い・不発効・整数でない周は `None`＝撃たない・0 も撃たない）。
fn limit_of(manifest: &Manifest) -> Option<u64> {
    crate::rules::int_row(manifest, ROW).ok().filter(|found| *found > 0)
}

/// `dispatch ls` の事前審査の行の末尾の語（行が無い・読めない周は ` prelens=unset`・組めない行は ` prelens=unbuilt`・他は空）。
pub(super) fn word(input: &Input<'_>, bead: &str) -> &'static str {
    if crate::rules::int_row(input.manifest, ROW).is_err() {
        return " prelens=unset";
    }
    let unbuilt = dir_of(input.state_dir).join(LENS_DIR).join(bead).join(UNBUILT).exists();
    if unbuilt {
        " prelens=unbuilt"
    } else {
        ""
    }
}

/// 周の頭の片付け: 落ちた周の worktree の残り（`tree` に在る worktree は `git worktree remove --force`・dir が無く登録だけが
/// 残る worktree は `git worktree prune`）を外し、母集団を出た bead の置き場を外す。worktree でない file は消さない（形 aa 1）。
fn prune(repo: &Path, root: &Path, population: &Population) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        drop_tree(repo, &entry.path().join(TREE));
        let name = entry.file_name().to_string_lossy().into_owned();
        if !population.rows.contains_key(&name) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    let _ = git_ok(repo, &["worktree", "prune"]);
}

/// 撃ち中か（`out` が無く、印を `lock_owner` が生きていると判じるか読めない・fail-closed）。印の無い行は撃ち中でない。
fn in_flight(place: &Path) -> bool {
    if place.join(OUT).exists() {
        return false;
    }
    match std::fs::read_to_string(place.join(PID)) {
        Ok(body) => lock_owner(&body, started_ms) != Owner::Dead,
        Err(err) => err.kind() != std::io::ErrorKind::NotFound,
    }
}

/// 置き場の file の `n` 行目（無い・読めない周は `None`）。
fn line_of(place: &Path, name: &str, n: usize) -> Option<String> {
    std::fs::read_to_string(place.join(name)).ok()?.lines().nth(n).map(str::to_owned)
}

/// 材料を組み直して `key` を付け替える（形 aa 3）。組めない周は理由の `unbuilt` を置く。組んだ材料の鍵が `fired` と違う行と、
/// 前の判定が測れない（`unparsed`）行は `rc`・`out`・`fired` を外す（別の材料の判定を写さない・鍵が動いた周に撃ち直す）。
fn rebuild(input: &Input<'_>, place: &Path, key: &str, forecast: Option<Forecast>) {
    let built = forecast
        .ok_or_else(|| "予想の base を決められない（祖先の層か契約の生成）".to_owned())
        .and_then(|found| build(input, place, &found));
    let digest = match built {
        Ok(found) => found,
        Err(reason) => return unbuilt(place, &reason),
    };
    let _ = std::fs::remove_file(place.join(UNBUILT));
    let stale = line_of(place, FIRED, 0).is_some_and(|fired| fired != digest) || verdict(place).is_some_and(|found| found.is_err());
    if stale {
        for name in [RC, OUT, FIRED] {
            let _ = std::fs::remove_file(place.join(name));
        }
    }
    let _ = std::fs::write(place.join(KEY), format!("{key}\n{digest}\n"));
}

/// 組めない・起こせない理由の 1 行を置く（黙って撃たないに畳まない・C10）。
fn unbuilt(place: &Path, reason: &str) {
    let _ = std::fs::create_dir_all(place);
    let _ = std::fs::write(place.join(UNBUILT), format!("{}\n", reason.replace('\n', " ")));
}

/// 置き場の `tree` に main の HEAD の detached な worktree を作り、層を当てて材料を組み、worktree を外す。材料の鍵を返す。
fn build(input: &Input<'_>, place: &Path, forecast: &Forecast) -> Result<String, String> {
    let tree = place.join(TREE);
    std::fs::create_dir_all(place).map_err(|err| format!("{} を作れない: {err}", place.display()))?;
    drop_tree(input.repo, &tree);
    let path = tree.display().to_string();
    if !git_ok(input.repo, &["worktree", "add", "--detach", &path, "HEAD"]) {
        return Err(format!("一時の worktree {path} を作れない"));
    }
    let staged = materialize(&tree, &forecast.layers).and_then(|declared| stage(input, place, &tree, forecast, declared.as_deref()));
    drop_tree(input.repo, &tree);
    staged
}

/// 一時の worktree を外す（worktree の印 `.git` の file を持つ dir だけ・worktree でない file は触らない＝組めない周の file は残す）。
fn drop_tree(repo: &Path, tree: &Path) {
    if tree.join(".git").is_file() {
        let _ = git_ok(repo, &["worktree", "remove", "--force", &tree.display().to_string()]);
    }
}

/// 層を依存の順に当てて index に載せ（`git add -A`）、宣言だけの祖先が空で置いた file の列を返す（宣言だけの祖先が無ければ
/// `None`＝予想の印を足さない）。Gated PASS の祖先は `add` を祖先の木の HEAD から拡張子で絞らずに写し `remove` を消す。
fn materialize(tree: &Path, layers: &[Layer]) -> Result<Option<Vec<String>>, String> {
    let mut declared: Option<Vec<String>> = None;
    for layer in layers {
        match *layer {
            Layer::Tree { ref add, ref remove, ref head, .. } => {
                let mut args = vec!["checkout", head.as_str(), "--"];
                args.extend(add.iter().map(String::as_str));
                if !add.is_empty() && !git_ok(tree, &args) {
                    return Err(format!("祖先の木 {head} の file を写せない"));
                }
                for path in remove {
                    let _ = std::fs::remove_file(tree.join(path));
                }
            }
            Layer::Declared(ref write_set) => declare(tree, write_set, declared.get_or_insert_with(Vec::new))?,
        }
    }
    if !git_ok(tree, &["add", "-A"]) {
        return Err("予想の base を index に載せられない".to_owned());
    }
    Ok(declared)
}

/// 宣言だけの祖先の層: `+` の file を空で作り（`placed` に足す）、`~` の file を消す。
fn declare(tree: &Path, write_set: &[String], placed: &mut Vec<String>) -> Result<(), String> {
    for item in write_set {
        let path = normalize(item);
        if item.starts_with(DELETE_FILE) {
            let target = tree.join(&path);
            let _ = if path.ends_with('/') { std::fs::remove_dir_all(target) } else { std::fs::remove_file(target) };
        }
        if !item.starts_with(NEW_FILE) || path.ends_with('/') {
            continue;
        }
        let target = tree.join(&path);
        let parent = target.parent().map(Path::to_path_buf).unwrap_or_else(|| tree.to_path_buf());
        std::fs::create_dir_all(parent).and_then(|()| std::fs::write(&target, "")).map_err(|err| format!("{path} を置けない: {err}"))?;
        placed.push(path);
    }
    Ok(())
}

/// 実体化した木から材料を組み（契約の本文を置き場に写してから [`review::stage`]）、材料の dir の鍵を返す。
fn stage(input: &Input<'_>, place: &Path, tree: &Path, forecast: &Forecast, declared: Option<&[String]>) -> Result<String, String> {
    let source = place.join(SOURCE);
    std::fs::write(&source, &forecast.body).map_err(|err| format!("{} を書けない: {err}", source.display()))?;
    let requirements = requirements_of(tree, input.manifest)?;
    let dir = place.join(REVIEW_DIR);
    let _ = std::fs::remove_dir_all(&dir);
    let note = declared.map_or_else(String::new, |paths| {
        std::iter::once(NOTE.to_owned()).chain(paths.iter().map(|path| format!("- {path}"))).collect::<Vec<String>>().join("\n")
    });
    review::stage(tree, (&forecast.contract, &source), &requirements, &dir, &note)?;
    digest(&dir)
}

/// 要件面の repo 相対 path（HEAD の宣言 `requirements`・`pipe review` の入口と同じ [`table_facts`] の読み口・表の検査を撃たない
/// 組み立て＝クラスの語列表は空）。
fn requirements_of(repo: &Path, manifest: &Manifest) -> Result<String, String> {
    let commands = crate::rules::list_row(manifest, CEILING_ROW)?;
    let denied = crate::rules::list_row(manifest, DENIED_ROW)?;
    let ceiling = Ceiling { row: CEILING_ROW, commands, denied, classes: &[] };
    table_facts(repo, &ceiling)
        .map(|facts| facts.requirements)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / "))
}

/// 材料の鍵（dir の全 file の名と本文を名の順に並べた digest の 1 つの字・材料の種類を列挙しない・形 aa 3）。
fn digest(dir: &Path) -> Result<String, String> {
    let entries = std::fs::read_dir(dir).map_err(|err| format!("{} を読めない: {err}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default().into_bytes());
        bytes.push(0);
        bytes.extend(std::fs::read(&path).map_err(|err| format!("{} を読めない: {err}", path.display()))?);
        bytes.push(0);
    }
    Ok(fnv1a_64(&bytes))
}

/// 終わった lens の rc と stdout（`out` が無い周は `None`）。
fn ended(place: &Path) -> Option<(Option<i32>, String)> {
    let text = std::fs::read_to_string(place.join(OUT)).ok()?;
    Some((line_of(place, RC, 0).and_then(|found| found.trim().parse().ok()), text))
}

/// Reviewed の段の使い回しの読み口（形 ac 1）: 実物の base で組んだ材料の dir `dir` の鍵が置き場の `fired` と同じで、判定が
/// 測れて（`unparsed` でない）、置き場の `lens` の字が便の lens の `cmd` と同じ周だけ、先撃ちの lens の rc と stdout を返す。
pub(in crate::pipe) fn reusable(state_dir: &Path, bead: &str, dir: &Path, cmd: &str) -> Option<(Option<i32>, String)> {
    let place = dir_of(state_dir).join(LENS_DIR).join(bead);
    let same = digest(dir).ok().is_some_and(|found| line_of(&place, FIRED, 0) == Some(found))
        && std::fs::read_to_string(place.join(LENS)).is_ok_and(|found| found == cmd);
    (same && verdict(&place).is_some_and(|found| found.is_ok())).then(|| ended(&place)).flatten()
}

/// 終わった lens の判定（`out` が無い周は `None`）: `Ok(Some)` は FAIL / INCONCLUSIVE で理由の型を持つ確定・`Ok(None)` は PASS・
/// `Err(())` は測れない（rc が 0 でない・JSON を読めない・理由の型が無い＝`unparsed`）。
fn verdict(place: &Path) -> Option<Result<Option<(FindingKind, String)>, ()>> {
    let (rc, text) = ended(place)?;
    Some(match review::outcome_of(rc, &text) {
        (Verdict::Pass, ..) => Ok(None),
        (_, Some(kind), evidence) if kind != FindingKind::Unparsed => Ok(Some((kind, evidence))),
        _ => Err(()),
    })
}

/// 判定を事前審査の結果に写す（形 aa 4）: 材料の鍵が `fired` と同じで判定が確定の周だけ在り処 [`AT`] の確定を載せ、結果の file が
/// 持つ先撃ちの確定と違う周だけ書き直す。前の結果は、同じ周に事前審査が書き直した行は書き直す前の結果（new の印を持ち越す）。
fn carry(sweep: &Sweep<'_, '_>, bead: &str, place: &Path, kept: &Kept) {
    let same = line_of(place, KEY, 0).as_deref() == Some(kept.key.as_str()) && line_of(place, KEY, 1) == line_of(place, FIRED, 0);
    let found = verdict(place).and_then(Result::ok).flatten().filter(|_| same).map(|(kind, evidence)| Finding {
        certainty: Certainty::Firm,
        name: kind.as_str().to_owned(),
        at: AT.to_owned(),
        reason: evidence.replace('\n', " "),
    });
    let now: Vec<&str> = kept.firm.keys().filter(|(_, at)| at == AT).map(|(name, _)| name.as_str()).collect();
    if now == found.iter().map(|finding| finding.name.as_str()).collect::<Vec<&str>>() {
        return;
    }
    let previous = sweep.before.get(bead).map_or(Some(kept), Option::as_ref);
    write(sweep.dir, bead, &kept.key, Some(found.as_slice()), previous);
}

/// 起こしてよい行か（材料が今の事前審査の鍵で組めていて、組めない理由が無く、終わった判定が無い＝未撃ちか印の死んだ行）。
fn ready(place: &Path, key: &str) -> bool {
    line_of(place, KEY, 0).as_deref() == Some(key) && !place.join(UNBUILT).exists() && !place.join(OUT).exists()
}

/// lens を裏で起こす（`spawn_self` と同じ起こし方＝process group を分け stdin を閉じる・箱で包まない・終わりを待たない・形 aa 2）。
/// `fired` と `lens` を起こす時に写し、起こせたら印 `<pid> <起動時刻>` を置く。起こせない周は `unbuilt` を置いて `false`。
fn fire(input: &Input<'_>, place: &Path, cmd: &str) -> bool {
    let Some(digest) = line_of(place, KEY, 1) else {
        return false;
    };
    let _ = std::fs::remove_file(place.join(PID));
    let marked = std::fs::write(place.join(FIRED), format!("{digest}\n")).and_then(|()| std::fs::write(place.join(LENS), cmd));
    let contract = place.join(REVIEW_DIR).join(CONTRACT_FILE).display().to_string();
    let line = crate::headless::fill(cmd, &[("{contract}", &contract), ("{worktree}", &input.repo.display().to_string())]);
    let spawned = marked.and_then(|()| {
        Invocation::new("sh")
            .args(["-c", WRAP, line.as_str()])
            .args([place.join(PARTIAL), place.join(RC), place.join(OUT)])
            .current_dir(input.repo)
            .env_remove(PANE_ENV)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|child| child.id())
    });
    match spawned {
        Ok(pid) => {
            let mark = started_ms(pid).started().map_or_else(|| format!("{pid}\n"), |at| format!("{pid} {at}\n"));
            let _ = std::fs::write(place.join(PID), mark);
            true
        }
        Err(err) => {
            let _ = std::fs::remove_file(place.join(FIRED));
            unbuilt(place, &format!("lens を起こせない: {err}"));
            false
        }
    }
}
