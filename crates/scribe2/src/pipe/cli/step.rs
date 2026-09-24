//! 段の手（`pipe approve` / `answer` / `gate` / `land` / `retire`・設計 §5「subcommand」）。
//!
//! どの手も段の前提を [`super::resolve`] で確かめてから、段の本体（`pipe::approve` / `pipe::gate` /
//! `pipe::land`）へ渡す。`s2-07l.295` で `cli.rs` から純移動した（本文は不変・各段の手順は宣言順のまま）。
//! 規則の値は rules 行から読む（数値を焼かない・C1 / C5）。

use super::{broken, flag, int_row, list_row, need, refused, resolve, state_dir_of, Extra};
use crate::cli_outcome::{Outcome, RC_BROKEN};
use crate::fleet::store::{LockPolicy, StoreError};
use crate::fleet::Stage;
use crate::pipe::approve::{Approve, RC_BLOCKED};
use crate::pipe::current;
use crate::pipe::declaration::{self, Ceiling, CEILING_ROW, DENIED_ROW};
use crate::pipe::follow::Runner;
use crate::pipe::gate::{Detection, Gate, Limits};
use crate::pipe::land::detection::Detect;
use crate::pipe::land::{Land, Retire};
use crate::pipe::lens_record::{self, LensSource};
use crate::pipe::ratelimit::Pool;
use crate::pipe::review::{review, Review};
use crate::pipe::run_dir;
use crate::rules::manifest::Manifest;
use std::path::Path;

/// 追随が衝突した便を起こし直す回数の上限を持つ rules 行。
const ROW_RETRIES: &str = "pipe.follow_retries";

/// land が着地待ちの列で自分の番を待つ上限（秒）を持つ rules 行（設計 gate-cost.md §6）。
const ROW_LAND_WAIT: &str = "pipe.land_wait_s";

/// 着地の列を候補の木 1 つに積む本数の上限（先頭を含む）を持つ rules 行（設計 pipeline.md §40・ADR-0039）。
const ROW_TRAIN_MAX: &str = "land.train_max";

/// 終端が CI の判定を待つ上限を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ROW_CI_WAIT: &str = "pipe.ci_wait_s";

/// 終端だけを撃ち直す flag（値なし・設計 contract-source.md §5 手順 3）。
const TERMINAL_ONLY: &str = "--terminal-only";

/// 終端の材料（CI の上限と台帳 client）を引数と規則から解く（**land と `--terminal-only` が共有**）。
fn terminal_input<'a>(args: &'a [String], manifest: &Manifest) -> Result<(u64, &'a str), Outcome> {
    let ci_wait_s = int_row(manifest, ROW_CI_WAIT).map_err(broken)?;
    let bd = flag(args, "--bd").map_err(refused)?.unwrap_or(crate::ledger::DEFAULT_BD);
    Ok((ci_wait_s, bd))
}

/// `pipe land --run <id> --terminal-only`: **着地をやり直さず終端だけ**を撃つ（冪等）。
///
/// 前提の段は `Landed`（着地は済んでいる）。着地した sha は記録から読む——HEAD の今の sha に
/// 読み替えると、その後に別の便が main を進めた周に**別の commit の CI を照合する**（C10）。
fn terminal_only(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Landed], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let Some(sha) = super::land::landed_sha(&resolved.state_dir, id) else {
        return refused(format!("run {id} の着地した sha を記録から読めない"));
    };
    let (ci_wait_s, bd) = match terminal_input(args, manifest) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match Limits::of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let entry = Land {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        pr_cmd: None,
        lens: &LensSource::Absent,
        limits,
        runner: None,
        retries: 0,
        land_wait_s: 0,
        ci_wait_s,
        bd,
        approved: resolved.approved,
        policy,
        train_max: 1,
    };
    let terminal = super::land::terminal(&entry, &sha);
    Outcome {
        out: vec![format!("run={id} terminal={}", terminal.as_token())],
        err: Vec::new(),
        rc: terminal.rc(),
    }
}

/// 着地後の検出だけを撃つ flag（値なし・設計 gate-cost.md §44 形 (1)・`--terminal-only` と同じ「着地をやり直さない口」）。
const DETECTION_ONLY: &str = "--detection-only";

/// 着地をやり直さない口の振り分け（どちらの flag も無い周は `None`＝着地の本体へ進む）。
///
/// - `--terminal-only`（設計 contract-source.md §5 手順 3）: 着地は成立しているのに終端が止まった便（push の失敗・
///   CI の未確定・台帳を閉じられなかった周）を、着地をやり直さずに継ぐ。
/// - `--detection-only`（設計 gate-cost.md §44 行 ak）: 着地した便の検出線を人が撃つ（撃ち直す）形。
fn settled_port(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Option<Outcome> {
    if super::present(args, TERMINAL_ONLY) {
        return Some(terminal_only(args, id, manifest, policy));
    }
    super::present(args, DETECTION_ONLY).then(|| detection_only(args, id, manifest, policy))
}

/// `pipe land --run <id> --detection-only`: **着地をやり直さず**、着地した commit に検出線を 1 回撃つ（人が撃つ口）。
///
/// 前提の段は `Landed`（他の段は何も書かずに rc 1）。着地した sha は [`super::land::landed_sha`] で記録から読み、読めない
/// 周は断る（HEAD の今の sha に読み替えない・`--terminal-only` と同じ理由）。受付札と遮断器の線は `--rules` か埋め込み。
fn detection_only(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Landed], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let Some(sha) = super::land::landed_sha(&resolved.state_dir, id) else {
        return refused(format!("run {id} の着地した sha を記録から読めない"));
    };
    let limits = match Limits::of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    super::land::detection::detect(
        &Detect {
            run: id,
            bead: &resolved.bead,
            repo: &resolved.repo,
            state_dir: &resolved.state_dir,
            contract: &resolved.contract,
            limits,
            policy,
        },
        &sha,
    )
}

/// `pipe approve`。**逐語を event へ写すだけ**で、段は動かさない（resume が進める）。
pub(super) fn approve_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    // 段は問わない（承認は「これから起こすこと」への許しで、遅れて来ても記帳する）が、
    // 便が在ることは確かめる＝無い run へ承認を書くと宛先の無い記録が残る。
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    super::approve::approve(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// `pipe answer`。**`Questioned` の run にだけ**逐語を event へ写す（段は動かさない・resume が進める）。
///
/// 承認（[`approve_run`]）と同型だが、段は問う——質問の無い便へ回答を書くと、後で来た質問の
/// 関門が前の回答で開く。段違いは `Blocked` の未承認と同じ **rc 3 で何も書かない**。
pub(super) fn answer_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let words = match need(args, "--words") {
        Ok(found) => found.to_owned(),
        Err(reason) => return refused(reason),
    };
    let state_dir = match state_dir_of(args) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let state = match current(&state_dir) {
        Ok(found) => found,
        Err(errors) => {
            return Outcome::failed(RC_BROKEN, errors.iter().map(StoreError::to_string).collect())
        }
    };
    let Some(run) = state.runs.get(id) else {
        return refused(format!("run {id} が無い"));
    };
    if run.stage != Stage::Questioned {
        return Outcome::failed_line(
            RC_BLOCKED,
            format!("pipe: run {id} は質問で止まっていない（段 {}）", run.stage.as_str()),
        );
    }
    super::approve::answer(&Approve {
        run: id,
        bead: &run.bead,
        state_dir: &state_dir,
        words: &words,
        policy,
    })
}

/// 契約の審査の段（`pipe intake` の直後・`resume` の `Intake`・前提 stage = `Intake`・FR49・設計
/// contract-source.md §4）。
///
/// lens は gate と同じ `--lens`（無ければ INCONCLUSIVE＝終端・fail-closed）。**受けた cmd は run dir の写し
/// （`lens.toml`・[`lens_record::keep`]）に残す**——gate / land / resume が `--lens` 無しで同じ lens を読む面で
/// あり（設計 pipeline.md §26）、写せない周（改行を含む cmd・書けない dir）は判定に届かず rc 2（fail-closed）。
/// 要件面の path は HEAD の宣言から読む（`contracts check` と同じ読み口・無ければ既定）。読めない周は判定に
/// 届かず rc 2（判定を書かない）。
pub(super) fn review_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Intake], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let lens = match flag(args, "--lens") {
        Ok(Some(cmd)) => match lens_record::keep(&run_dir(&resolved.state_dir, id), cmd) {
            Ok(()) => LensSource::Cmd(cmd.to_owned()),
            Err(reason) => return broken(reason),
        },
        Ok(None) => LensSource::Absent,
        Err(reason) => return refused(reason),
    };
    let requirements = match requirements_of(&resolved.repo, manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    review(&Review {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        requirements: &requirements,
        lens: &lens,
        policy,
    })
}

/// 要件面の repo 相対 path（HEAD の宣言 `requirements`・無ければ既定・[`declaration::table_facts`] の 1 本）。
fn requirements_of(repo: &Path, manifest: &Manifest) -> Result<String, String> {
    let commands = list_row(manifest, CEILING_ROW)?;
    let denied = list_row(manifest, DENIED_ROW)?;
    // 表の検査を撃たない組み立て＝クラスの語列表は読まず空の列（設計 contract-source.md §48 の 5）。
    let ceiling = Ceiling { row: CEILING_ROW, commands: &commands, denied: &denied, classes: &[] };
    declaration::table_facts(repo, &ceiling)
        .map(|facts| facts.requirements)
        .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<String>>().join(" / "))
}

/// `pipe gate`。前提 stage = `Implemented` ∨ (`Gated` ∧ verdict が INCONCLUSIVE)。
///
/// **測り直せるのは「測れなかった」周だけ**である。INCONCLUSIVE は道具が足りなくて
/// 判定に届かなかった印（`--lens` 無し / diff が cap 超 / lens の不備）なので、道具を
/// 揃えれば同じ便を撃ち直せる。PASS / FAIL は判定に届いた周ゆえ**終端のまま**で、
/// 段違いの一般則どおり何もせず rc 1 を返す——FAIL から撃ち直す口を開けると、契約の
/// verify が赤い便が「壊れたまま進む」経路になる。
///
/// lens は `--lens` が在れば flag、無ければ審査が残した run dir の写し（[`lens_source`]・設計 pipeline.md §26）。
/// 検出線は**常に撃つ**（省けるのは main が動いた便の追随の再 gate だけ・設計 §30）。
pub(super) fn gate_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    let resolved = match resolve(args, id, &[Stage::Implemented, Stage::Gated], &Extra::Regate) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match Limits::of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let lens = match lens_source(args, id, &resolved.state_dir) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // lens の口座も器が選ぶ（設計 account-autonomy.md §15・FR36）。宣言（`--rules` の tracked の面 + 置き場の
    // host の面）が 0 の周は `None`＝lens は親の環境を継承する。新しい flag は足さない（`--rules` / `--curl` の写し）。
    let pool = match Pool::declared(args, manifest, &resolved.state_dir) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    super::gate::gate(&Gate {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        lens: &lens,
        pool: pool.as_ref(),
        limits,
        detection: Detection::Run,
        policy,
    })
}

/// gate / land（と `resume` が通る同じ関数）の lens の出所: `--lens` が在れば flag が勝ち、無ければ審査が残した
/// run dir の写しを読む（[`lens_record::resolve`] の 1 本・設計 pipeline.md §26）。写しの無い / 読めないは判定側が
/// 3 値で分ける（ここで INCONCLUSIVE へ先回りしない＝再 gate の要らない land まで倒さない）。`Err` は flag の値欠け。
fn lens_source(args: &[String], id: &str, state_dir: &Path) -> Result<LensSource, String> {
    let flagged = flag(args, "--lens")?;
    Ok(lens_record::resolve(flagged, &run_dir(state_dir, id)))
}

/// `pipe land`。前提 stage = Gated（PASS の検査は land 側が持つ）。
///
/// `--pr-cmd` は自 repo への PR の口ゆえ**承認 event を前提としない**（A4.3・ADR-0008）。
/// lens（`--lens` か run dir の写し・[`lens_source`]）と規則の線は main が動いた便の追随（rebase → gate の
/// 撃ち直し・設計 §5.4）で gate へ渡すために読む（land 自身は数値を見ない）。
pub(super) fn land_run(args: &[String], id: &str, manifest: &Manifest, policy: LockPolicy) -> Outcome {
    // **着地をやり直さない口**（終端だけ・着地後の検出だけ）は着地の前提を見ずに先に分ける。
    if let Some(outcome) = settled_port(args, id, manifest, policy) {
        return outcome;
    }
    // `Landed` も通す（設計 pipeline.md §40）: 列の先頭が候補の木に積んで着地させた便の land は、段を読んで何もせず
    // rc 0 で終わる（判定は land の番待ちの直後の 1 点）。`--pr-cmd` の形は列を見ないので従来どおり段違いで断る。
    let resolved = match resolve(args, id, &[Stage::Gated, Stage::Landed], &Extra::Nothing) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    let limits = match Limits::of(manifest) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    let pr_cmd = match flag(args, "--pr-cmd") {
        Ok(Some(_)) if resolved.stage == Stage::Landed => return refused(format!("run {id} の段は Landed である")),
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    let lens = match lens_source(args, id, &resolved.state_dir) {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // 追随が衝突した周は実装役を起こし直す（設計 pipeline-conflict.md §3）。`pipe run` は
    // 自分の runner をそのまま渡し、`--runner` を持たない `pipe land` は起こし直せない
    // ——衝突の記帳だけ残して断り、`pipe resume --runner` で続けられる。
    let runner = match flag(args, "--runner") {
        Ok(found) => found,
        Err(reason) => return refused(reason),
    };
    // 起こし直しの口座は器が選ぶ（設計 account-autonomy.md §4）。宣言は runner を持つ周だけ解く（runner の無い
    // `pipe land` は起こし直さない＝選定の入力も要らず、宣言の読めなさで land を止めない）。
    let pool = match runner.map(|_| Pool::declared(args, manifest, &resolved.state_dir)).transpose() {
        Ok(found) => found.flatten(),
        Err(reason) => return refused(reason),
    };
    let runner = runner.map(|cmd| Runner { cmd, pool: pool.as_ref() });
    let retries = match int_row(manifest, ROW_RETRIES) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // 着地の順番を待つ上限（設計 gate-cost.md §6）。`--rules` の manifest から読む＝上限を振る歯の
    // fixture が land へ届く口はここだけである。
    let land_wait_s = match int_row(manifest, ROW_LAND_WAIT) {
        Ok(found) => found,
        Err(reason) => return broken(reason),
    };
    // 終端の材料（CI の上限と台帳 client）は `--terminal-only` と**同じ 1 本**で解く。
    let (ci_wait_s, bd) = match terminal_input(args, manifest) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    super::land::land(&Land {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        contract: &resolved.contract,
        pr_cmd,
        lens: &lens,
        limits,
        runner,
        retries,
        land_wait_s,
        ci_wait_s,
        bd,
        approved: resolved.approved,
        policy,
        // 着地の列を積む上限（設計 pipeline.md §40）。**行が無い・読めない周は 1**＝先頭だけ（従来の経路・止めない）。
        train_max: int_row(manifest, ROW_TRAIN_MAX).unwrap_or(1),
    })
}

/// `pipe retire`。前提 stage = `Landed` ∨ (`Failed` ∧ 最後の `RunStage` の detail が
/// `rebase-empty` / `rebase-conflict`) ∨ (`Gated` ∧ verdict が FAIL) ∨ `Stopped` ∨
/// (`Reviewed` ∧ 審査の verdict が PASS でない)（worktree 在り・clean の検査は retire 側が持つ）。
///
/// **段を動かさない口である**。`--pr-cmd` 形の便は main を動かさず worktree も残して
/// `Landed` で終端するので、merge の後に入れ物だけを畳む段が要る。同一変更の便が
/// `rebase-empty` で終端した周も**成果は既に main に在る**ので入れ物だけが残る形は同じで、
/// 畳める側に数える（`s2-07l.128`）。起こし直しの上限に達した便（`rebase-conflict`）と
/// 判定に届いた `Gated(FAIL)` も、終端して入れ物だけが残る形は同じである（設計
/// pipeline-conflict.md §5）。`pipe stop --run` で終端した `Stopped` も同じ——stop は畳まない
/// （C2・段の関数は 1 つずつ）ので、commit 0 の clean な worktree が残る唯一の畳み口が
/// ここである（`s2-07l.284`・理由の弁別は持たず `Landed` と同じ扱い）。審査の段で終端した
/// `Reviewed`（verdict が FAIL / INCONCLUSIVE）も同じ形で、前の周が残した worktree を畳める側に
/// 数える（`s2-07l.353`・設計 pipeline.md §12）——畳めないままだと再開（FR14）の続きの段が
/// 別の worktree に割れる。走っている便・他の理由で落ちた便を通すと「まだ読まれていない現物を
/// 動かす」経路になるため、段違いは一般則どおり rc 1。
///
/// 残す event の段は [`super::Resolved::stage`] のまま＝**`Landed` に決め打ちしない**（終端を動かさない）。
pub(super) fn retire_run(args: &[String], id: &str, policy: LockPolicy) -> Outcome {
    let allowed = [Stage::Landed, Stage::Failed, Stage::Gated, Stage::Stopped, Stage::Reviewed];
    let resolved = match resolve(args, id, &allowed, &Extra::Retire) {
        Ok(found) => found,
        Err(outcome) => return outcome,
    };
    super::land::retire(&Retire {
        run: id,
        bead: &resolved.bead,
        repo: &resolved.repo,
        state_dir: &resolved.state_dir,
        stage: resolved.stage,
        policy,
    })
}
