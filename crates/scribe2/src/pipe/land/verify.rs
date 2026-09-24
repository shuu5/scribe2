//! land の主実測（設計 docs/design/pipeline.md §5.4・§41・FR11・`s2-07l.457`）。
//!
//! main を進めた後に**別の worktree** で verify を撃ち、その 3 値を返す群（[`verify_main`] と材料・record）と、
//! 赤 / 測れなかったの終端（[`main_red`] / [`main_unmeasured`]）である。`pipe/land.rs` からの**純移動**で、
//! 歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。
//!
//! 判定 enum `MainCheck` は**親に残る**——極性一覧（`crate::polarity`・snapshot `polarity_external_form`）が
//! 境界の型名 `pipe::land::MainCheck` で pin しており、ここへ移すと `pub use` を足しても外形が変わる
//! （§41「決定的な制約」）。
//!
//! 可視性: 親が呼ぶ 4 本（[`verify_main`] / [`main_red`] / [`main_unmeasured`] は `land` から、[`measure_main`]
//! は `finish` から）だけが `pub(super)` で、残りはこの module に閉じる。逆向き（子 → 親）は `super::` で
//! そのまま見える（Rust の可視性＝子孫は祖先の私有を見る）ので、**親側の可視性は 1 語も上げていない**。

// flip-check: moved s2-07l.457

use super::super::declaration::Effective;
use super::super::gate::{is_unreadable, records_of, run_checks, Checks, DetectionSkip, Skipped, Step};
use super::super::retire::verdict_field;
use super::super::{emit, git_bytes, git_line, git_ok, worktrees_dir, Emit};
use super::{broken, detection_needed, nul_paths, refused, AnchorSync, Land, MainCheck, MAIN_REF};
use crate::cli_outcome::Outcome;
use crate::fleet::store::append_line;
use crate::fleet::{EventKind, Stage};
use std::path::{Path, PathBuf};

/// main 実測用の tmp worktree を置く dir 名。
///
/// **`std::env::temp_dir` を使わない**（`TMPDIR` を読む＝憲法 C2.2 に反する）。置き場は
/// 便の worktree と同じ repo 配下から導く。run id は `<bead>-<stamp>` なのでこの名と
/// 衝突しない。
const CHECK_DIR: &str = "verify";

/// main 実測用の tmp worktree（着地後の検出も同じ置き場に別名で出す・[`super::detection`]）。
pub(super) fn check_path(repo: &Path, id: &str) -> PathBuf {
    worktrees_dir(repo).join(CHECK_DIR).join(id)
}

/// 進めた main を別の worktree で実測する。
///
/// **木が同じ周は 1 本も撃たない**（設計 gate-cost.md §27・ADR-0043 §2.1）: gate が判定した木（verdict の `tree`）と
/// 着地の木が同じなら、主実測は同じ木を同じ verify で撃ち直すだけ＝worktree も材料も要らない。撃たなかった事実は
/// `verify-main.jsonl` の skip record 1 本（[`Skipped::main`]・木を必ず持つ）に残し、**書けない周は緑を名乗らない**
/// （撃たずに緑と読める形を残さない・C10）。木が違う周・`tree` の無い周・比べられない周は従来どおり全段へ倒す。
pub(super) fn verify_main(entry: &Land<'_>, new: &str) -> MainCheck {
    verify_main_from(entry, new, None)
}

/// 着地の列の主実測（設計 pipeline.md §40）: [`verify_main`] と同じ 1 本を、`{base}` と write-set 照合の base を
/// **候補の木を切った main**（`old`）に据えて撃つ。便の記録の base からの差分は列の他の便と main の動きを含むので、
/// 照合は呼び手が列の write-set を合わせた契約（`entry.contract`）で行う。record は先頭の便の `verify-main.jsonl`。
pub(super) fn verify_train_main(entry: &Land<'_>, new: &str, old: &str) -> MainCheck {
    verify_main_from(entry, new, Some(old))
}

/// [`verify_main`] の本体（`base` が `Some` の周だけ材料の base を差し替える）。
fn verify_main_from(entry: &Land<'_>, new: &str, base: Option<&str>) -> MainCheck {
    let skipped = main_detection(entry, new);
    if let Some((DetectionSkip::SameTree, tree)) = &skipped {
        return match record_main(entry, &[], Some(Skipped::main(tree))) {
            Ok(()) => MainCheck::Green,
            Err(reason) => MainCheck::Unmeasurable(reason),
        };
    }
    let tmp = check_path(entry.repo, entry.run);
    if let Some(parent) = tmp.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return MainCheck::Unmeasurable(format!("{} を作れない: {err}", parent.display()));
        }
    }
    let path = tmp.display().to_string();
    if !git_ok(entry.repo, &["worktree", "add", "--detach", &path, new]) {
        // **ここで赤を名乗らない**: verify 行を 1 本も撃てていない。
        return MainCheck::Unmeasurable(format!("{} を切れない", tmp.display()));
    }
    // **gate と同じ順序を同じ関数で撃つ**（write-set 照合 → 写しの共通 verify → 検出線 → 契約 verify）。
    // 材料が揃わない周は**赤を名乗らない**——読めなかったを落ちたに化けさせない。
    let materials = materials(entry);
    let (base, frozen) = match materials {
        Ok((recorded, frozen)) => (base.map_or(recorded, str::to_owned), frozen),
        Err(reason) => {
            let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
            return MainCheck::Unmeasurable(reason);
        }
    };
    let steps = run_checks(&Checks {
        worktree: &tmp,
        base: &base,
        contract: entry.contract,
        common: frozen.common_verify(),
        detection: if skipped.is_some() { &[] } else { frozen.detection_verify() },
        // gate と同じ遮断器を同じ `Limits` から通す（設計 gate-cost.md §32 約束 9）。
        host: entry.limits.breaker(),
    });
    // 成果は `new` に載っているので、この tmp だけは remove してよい（設計 §5.4）。
    // `--force` は verify が tmp に生んだ中間物ごと畳むためで、履歴・データは触らない。
    let _ = git_ok(entry.repo, &["worktree", "remove", "--force", &path]);
    let record = skipped
        .as_ref()
        .map(|(reason, tree)| Skipped::detection(*reason, Some(tree.as_str())));
    if let Err(reason) = record_main(entry, &steps, record) {
        return MainCheck::Unmeasurable(reason);
    }
    // 段①を読めなかった周（gate と**同じ 1 本の判定**・rc だけでは見ない）は**赤の集計より先に**
    // 「測れなかった」へ倒す——読めなかったを落ちたに化けさせない（gate §6 と同じ極性・
    // `s2-07l.103`）。Red と同じく main-green にも finish にも進まない（fail-closed）。
    if let Some(step) = steps.iter().find(|step| is_unreadable(step)) {
        return MainCheck::Unmeasurable(format!(
            "main で verify の段を読めない（cmd={} stderr={}）",
            step.cmd,
            step.stderr.lines().next().unwrap_or_default()
        ));
    }
    // 遮断器が閉じて撃たなかった行が在る周も**赤の集計より先**に「測れなかった」へ倒す（gate と同じ極性・
    // 設計 gate-cost.md §32 約束 6 / 9）——撃っていない行の rc -1 を赤に数えない。
    if let Some(step) = steps.iter().find(|step| step.is_closed()) {
        return MainCheck::Unmeasurable(format!(
            "host が混んだまま待ちの上限を超えた（cmd={} 以後の verify の行を撃っていない）",
            step.cmd
        ));
    }
    let red = steps.iter().filter(|step| step.rc != 0).count();
    if red > 0 {
        return MainCheck::Red(format!("main で verify の {red} 行が rc≠0"));
    }
    MainCheck::Green
}

/// main の実測に要る材料（便の base と、写しの共通 verify・検出線）を揃える。
///
/// **写しからしか読まない**（repo / worktree の `.vessel.toml` は読み直さない・ADR-0010 §2.4）。
fn materials(entry: &Land<'_>) -> Result<(String, Effective), String> {
    let found = super::base_of_run(entry.state_dir, entry.run);
    let unreadable = found.is_unreadable();
    let base = found.known().ok_or_else(|| match unreadable {
        true => format!("run {} の base を読めない（置き場）", entry.run),
        false => format!("run {} に base が無い", entry.run),
    })?;
    let path = super::vessel_path(entry.state_dir, entry.run);
    let frozen = Effective::load(&path).map_err(|errors| {
        let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
        format!("{} を読めない: {}", path.display(), lines.join(" / "))
    })?;
    Ok((base, frozen))
}

/// main 実測の record を書く file の名（gate の `verify.jsonl` と同じ dir・同じ record 形・別 file）。
///
/// 別 file にするのは、gate の周の `n` と main 実測の `n` を重ねないためである（設計 gate-cost.md §5）。着地後の検出の
/// record（`landed` 付き）も同じ file に追記する（[`super::detection`]・設計 gate-cost.md §44 形 (4)）。
pub(super) const VERIFY_MAIN_FILE: &str = "verify-main.jsonl";

/// main 実測の赤い行の stderr の写しを残す診断 file の名（[`VERIFY_MAIN_FILE`] と同じ dir・同じ stem・設計 pipeline.md §35 (3)）。
///
/// gate の `verify.stderr.log` の対で、**機械は読まない**（人が「main の何の歯がどう赤いか」を読む）。
pub(super) const VERIFY_MAIN_STDERR_FILE: &str = "verify-main.stderr.log";

/// 主実測で何を省くか（省く周はその理由と land した木の sha・設計 §30 (ii)・ADR-0021 §2.4）。
///
/// gate を撃った木（verdict の `tree`）と land した木が**同じ**なら `same-tree`（[`verify_main`] は主実測ごと省く・
/// 設計 gate-cost.md §27）。違う周は
/// `git diff-tree -r --name-only -z <gated> <landed>` の path を [`super::DETECTION_SCOPE`] と照らし、1 つも触れなければ
/// `outside-scope`。`tree` の無い verdict（旧 gate）・読めない木・読めない diff はどれも `None`＝全段を撃つ側へ
/// 倒す（省く側へ倒すと、測っていない検出線を main で通したことになる）。
fn main_detection(entry: &Land<'_>, new: &str) -> Option<(DetectionSkip, String)> {
    let gated = verdict_field(entry.state_dir, entry.run, "tree")?;
    let landed = git_line(entry.repo, &["rev-parse", &format!("{new}^{{tree}}")])?;
    if landed == gated {
        return Some((DetectionSkip::SameTree, landed));
    }
    let bytes = git_bytes(entry.repo, &["diff-tree", "-r", "--name-only", "-z", &gated, &landed])?;
    let paths = nul_paths(&bytes);
    if detection_needed(paths.iter().map(String::as_str)) {
        return None;
    }
    Some((DetectionSkip::OutsideScope, landed))
}

/// main 実測の段を `verify-main.jsonl` へ逐条で残す。検出線を省いた周は、その段の位置に
/// `skipped=detection tree=<sha> reason=<理由>` の record を 1 件置き、主実測ごと省いた周は段が空なので
/// `kind=main skipped=main` の record 1 件だけになる（**撃たなかった事実を黙って落とさない**・
/// 形と位置は gate の `verify.jsonl` と同じ [`records_of`] の 1 本）。赤い行の stderr の写しは
/// [`VERIFY_MAIN_STDERR_FILE`] へ gate と同じ書き口（`Record::diagnose`）で残す（設計 pipeline.md §35 (3)）。
fn record_main(entry: &Land<'_>, steps: &[Step], skipped: Option<Skipped<'_>>) -> Result<(), String> {
    let path = super::verify_log_path(entry.state_dir, entry.run).with_file_name(VERIFY_MAIN_FILE);
    let diagnosis = path.with_file_name(VERIFY_MAIN_STDERR_FILE);
    for record in records_of(steps, skipped) {
        record.diagnose(&diagnosis, entry.policy)?;
        append_line(&path, &record.body, entry.policy).map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// 終端で `refs/heads/main` を読めなかった周の実測値の字面（`main=unknown` / `main:unknown`）。
///
/// 読めないを「一致した」にも「動いた」にも化けさせない（C10）。land 自体は成立している
/// （ref は既に進み実測も緑）ので落とさず、理由は stderr 1 行に残す。着地後の検出の record の `tree` も、親か木を
/// 読めない周はこの語を書く（[`super::detection`]）。
pub(super) const MAIN_UNKNOWN: &str = "unknown";

/// 終端の直前に `refs/heads/main` を 1 回実測する（設計 §27・`s2-07l.379`）。
///
/// `landed=` / verdicts.jsonl の `sha` は **宣言値**（squash で main に載せた `new`）で、
/// CAS の後に main がさらに動いた周（追随の chain・別の便・手の操作）をそこからは見分けられない。
/// 実測値は宣言値と**別の列**に置く（一致する周も省かない）。読めない周は
/// [`MAIN_UNKNOWN`] と stderr の理由 1 行。
pub(super) fn measure_main(repo: &Path) -> (String, Vec<String>) {
    match git_line(repo, &["rev-parse", MAIN_REF]) {
        Some(found) => (found, Vec::new()),
        None => (
            MAIN_UNKNOWN.to_owned(),
            vec![format!("pipe: 終端で {MAIN_REF} を読めない（main={MAIN_UNKNOWN}・land は成立している）")],
        ),
    }
}

/// main の実測が赤だった周。**auto revert しない**（main は進んだまま・anchor は揃え済み＝stderr に token）。
pub(super) fn main_red(entry: &Land<'_>, reason: &str, anchor: &AnchorSync) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-red".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => with_anchor(refused(format!("main が赤い（{reason}）・revert しない")), anchor),
    }
}

/// failure exit の stderr に anchor の token（と warning）を足す（ref は進んでいるので黙らない）。
fn with_anchor(mut outcome: Outcome, anchor: &AnchorSync) -> Outcome {
    outcome.err.push(format!("pipe: {}", anchor.token()));
    outcome.err.extend(anchor.warning());
    outcome
}

/// main を実測できなかった周。**赤とは別の名で残す**（rc 2 = 対象が壊れている・anchor は揃え済み）。
pub(super) fn main_unmeasured(entry: &Land<'_>, reason: &str, anchor: &AnchorSync) -> Outcome {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunStage,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Failed),
            seat: None,
            pid: None,
            detail: Some("main-unmeasured".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => with_anchor(broken(format!("main を実測できない（{reason}）・revert しない")), anchor),
    }
}
