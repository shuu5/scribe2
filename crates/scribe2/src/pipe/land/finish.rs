//! land の squash と finish（設計 docs/design/pipeline.md §5.4・§43・FR50・`s2-07l.498`）。
//!
//! PR の seam（[`open_pr`]）、squash commit と message（[`squash`] / [`squash_message`]）、着地の後の
//! export → `Landed` → 後始末（[`finish`]）と終端（[`terminal`]・push → CI の照合 → 台帳の close）の群である。
//! `pipe/land.rs` からの**純移動**で、歯は 1 本も足していない（親に残る in-file の歯と e2e が従来どおり測る）。
//!
//! 判定 enum `Terminal` と `TERMINAL_TOKENS` / `TERMINAL_POLARITY` は**親に残る**——極性一覧（`crate::polarity`・
//! snapshot `polarity_external_form`）が境界の型名 `pipe::land::Terminal` で pin している（§43「決定的な制約」）。
//!
//! 可視性: 親の `land` / `attempt` が呼ぶ 3 本（[`open_pr`] / [`squash`] / [`finish`]）と親の歯が読む item は
//! `pub(super)`、`pipe` の中で `land::` として引かれる 2 本（[`landed_sha`] / [`terminal`]）は親が再輸出する
//! ので `pub(in crate::pipe)`（再輸出は可視性を広げられない）。逆向き（子 → 親）は `super::` でそのまま見える
//! （Rust の可視性＝子孫は祖先の私有を見る）ので、**親側の可視性は 1 語も上げていない**。

// flip-check: moved s2-07l.498

use super::super::contract::Contract;
use super::super::declaration::Effective;
use super::super::dispatch::spawn_self;
use super::super::gate::{LandedMark, Unfired, Verdict};
use super::super::queue::{Order, Turned};
use super::super::{emit, git_bytes, git_line, git_ok, size, verdict_path, vessel_path, Emit};
use super::anchor::Anchored;
use super::detection::{daily_floor, deferred, unfired, wake, Detect, Wake, DEFERRED, DETAIL_HEAD, SPAWNED, UNSPAWNED};
use super::verify::{main_red, main_unmeasured, measure_main, verify_train_main, MAIN_UNKNOWN};
use super::{broken, refused, retire_worktree, verdicts_path, Land, Landing, MainCheck, Terminal, MAIN_REF, RUN_TRAILER};
use crate::cli_outcome::{Outcome, RC_OK};
use crate::fleet::json_lite::{self, Value};
use crate::fleet::lifecycle::{self, Place};
use crate::fleet::store::{self, append_line};
use crate::fleet::{ci_now, cli::now_utc, CiRun, Completion, EventKind, Stage, SCHEMA};
use crate::name::{BUILD_COMMIT, NAME};
use std::path::{Path, PathBuf};

/// squash commit の件名に載せる要旨の長さ（**char 単位**・byte でない・`s2-07l.130`）。
///
/// git の慣習（件名は短く 1 行）に合わせて切るが、**切った goal は本文に逐語で残す**——
/// 要旨だけを残すと契約の中身が履歴から落ちる。
pub(super) const SUBJECT_CHARS: usize = 72;

/// 要旨を切ったことを示す印（件名の末尾に 1 文字だけ足す）。
const ELLIPSIS: char = '…';

/// PR を作る seam を通す（設計 §5.4 の `--pr-cmd`・**main を動かさない**）。
///
/// **承認 event は前提でない**。自 repo へ branch を push して PR を出す行為は main を
/// 動かさず、branch も PR も閉じられる＝可逆ゆえ、憲法 A4.3（merge・自 repo への
/// dispatch・依存なしの code 変更は A4.2 の目的において可逆）により Ask-first の「出す」
/// に当たらない（ADR-0008）。3 クラスの判定は契約の自己申告（`classes`）だけに効き、
/// seam を使ったことから導出しない。
///
/// **stale base は見ない**。CAS の old が要るのは ref を進める周だけで、この形は ref を
/// 1 本も動かさない——PR が載るかどうかは forge が決める。逆にここで base を縛ると、
/// main が動いた瞬間に PR を出せなくなる（自己ホストの便が最も踏みやすい）。
///
/// **道具の失敗で便を終端させない**（rc 1・event を書かない）。push や PR 作成は network
/// で落ちうるので、`Failed` を焼くと再試行できない便が残る。
pub(super) fn open_pr(entry: &Land<'_>, base: &str, cmd: &str) -> Outcome {
    // **空の seam を通さない**（使い方の誤り・rc 1・何も書かない）。`sh -c ""` は rc 0 で
    // 終わるので、素通しすると「PR を出した」を記帳しながら **1 行も公開していない**便が
    // 生まれる（何もしていないのに「やった」が永続面に残る——最も避けたい嘘である）。
    if cmd.trim().is_empty() {
        return refused("--pr-cmd が空である".to_owned());
    }
    let branch = super::branch_name(entry.run);
    let line = cmd.replace("{branch}", &branch).replace("{base}", base);
    let ran = crate::invocation::Invocation::new("sh")
        .arg("-c")
        .arg(&line)
        .current_dir(entry.repo)
        .status();
    match ran {
        Err(err) => return refused(format!("PR の道具を起動できない: {err}")),
        Ok(status) if !status.success() => {
            return refused(format!(
                "PR の道具が rc {} で終わった",
                status.code().unwrap_or(-1)
            ))
        }
        Ok(_) => {}
    }
    // **面 5 へは書かない**: `verdicts.jsonl` は main に載った便の記録で、この形の便は
    // まだ載っていない（merge は人が押す）。worktree も畳まない（PR は生きている）。
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some("pr".to_owned()),
        },
        entry.policy,
    );
    match emitted {
        Err(err) => broken(err.to_string()),
        Ok(()) => Outcome::ok_line(format!("run={} landed=pr", entry.run)),
    }
}

/// worktree の tree を 1 commit にして main を CAS で進める。**tree の同一を実測する**。
pub(super) fn squash(entry: &Land<'_>, worktree: &Path, old: &str) -> Result<String, String> {
    let tree = git_line(worktree, &["rev-parse", "HEAD^{tree}"])
        .ok_or_else(|| format!("{} の tree を読めない", worktree.display()))?;
    let message = squash_message(entry.bead, &entry.contract.goal, entry.run, entry.contract);
    let new = git_line(entry.repo, &["commit-tree", &tree, "-p", old, "-m", &message])
        .ok_or_else(|| "squash commit を作れない".to_owned())?;
    if !git_ok(entry.repo, &["update-ref", MAIN_REF, &new, old]) {
        return Err(format!("{MAIN_REF} を付け替えられない（CAS が外れた）"));
    }
    let landed = git_line(entry.repo, &["rev-parse", &format!("{new}^{{tree}}")])
        .ok_or_else(|| "land した tree を読めない".to_owned())?;
    if landed != tree {
        return Err(format!("tree が同一でない（{tree} → {landed}）"));
    }
    Ok(new)
}

/// squash commit の message（設計 §5.4 手順 1・**3 部**・`s2-07l.130`）。
///
/// (1) 件名 `<bead>: <要旨>` (2) 空行 (3) 本文 = goal 全文（**逐語・改行を保つ**）+ 空行 +
/// `run: <run id>` の 1 行（trailer）。
///
/// 件名は goal の先頭の文を [`SUBJECT_CHARS`] 文字で切った要約ゆえ中身が落ちる——だから
/// **同じ message の中に落とさない側（本文の goal 全文）を必ず持つ**。`git log --oneline` は
/// 件名だけを読み、便の現物を追う人は本文と trailer から fleet の記録へ辿る。
pub(super) fn squash_message(bead: &str, goal: &str, run: &str, contract: &Contract) -> String {
    let mut trailers = format!("{RUN_TRAILER}{run}\n");
    // **着地の正本は record（面 5・event log）である**（設計 contract-source.md §5 手順 5）。trailer は器が
    // squash message に同時に書く**導出面**で、RTM（別 repo）は trailer だけを読み、無ければ「まだ分からない」
    // と出す（「未着地」とは言わない）。空の欄は行ごと書かない——空の trailer は「無い」と読めない。
    if !contract.design.trim().is_empty() {
        trailers.push_str(&format!("{}{}\n", trailer_key(CONTRACT_TRAILER), contract.design.trim()));
    }
    if !contract.req.is_empty() {
        trailers.push_str(&format!("{}{}\n", trailer_key(REQUIREMENTS_TRAILER), contract.req.join(" ")));
    }
    format!("{}\n\n{goal}\n\n{trailers}", subject_of(bead, goal))
}

/// 契約を名指す trailer の語幹。
pub(super) const CONTRACT_TRAILER: &str = "Contract";

/// 要件を名指す trailer の語幹。
pub(super) const REQUIREMENTS_TRAILER: &str = "Requirements";

/// 発端の trailer の語幹（設計 vessel-hook.md §21 形 2 (b)・器の便でない merge の本文が持つ）。
const SOURCE_TRAILER: &str = "Source";

/// 発端の trailer の key（`<Name>-Source: `・merge の門が引く 1 本＝門は NAME から key を組み直さない・§21 形 7）。
pub(crate) fn source_key() -> String {
    trailer_key(SOURCE_TRAILER)
}

/// 契約の trailer の key（`<Name>-Contract: `・値は設計 pointer の字・局面の出力が main の commit から契約を読む 1 本）。
pub(crate) fn contract_key() -> String {
    trailer_key(CONTRACT_TRAILER)
}

/// trailer の key（**器の名から導く**・C2.2＝名を 2 か所に焼かない）。
///
/// 先頭を大文字にした器の名を前置するので、他の道具の trailer（`Co-Authored-By` 等）と衝突しない。
pub(super) fn trailer_key(stem: &str) -> String {
    let mut chars = NAME.chars();
    let head: String = chars.next().map(|first| first.to_uppercase().to_string()).unwrap_or_default();
    format!("{head}{}-{stem}: ", chars.as_str())
}

/// 件名。要旨が空の周は **`<bead>` だけ**にして落とさない（契約の検査で goal は非空のはずで、
/// 件名を組めないことは land を止める理由ではない＝ここを fail-closed に倒すと、message の
/// 形の不備で main に載らない便が生まれる）。
pub(super) fn subject_of(bead: &str, goal: &str) -> String {
    let gist = gist_of(goal);
    if gist.is_empty() {
        return bead.to_owned();
    }
    format!("{bead}: {gist}")
}

/// goal の先頭の文（最初の改行または「。」の手前まで・前後の空白と markdown の見出し記号 `#` を除く）
/// を [`SUBJECT_CHARS`] 文字で切る。切った周だけ末尾に [`ELLIPSIS`] を足す。
///
/// 切るのは **char 単位**である（byte で切ると UTF-8 の途中で割れる＝slice 禁止・C11）。
fn gist_of(goal: &str) -> String {
    let head = goal.split(['\n', '。']).next().unwrap_or_default();
    let sentence = head.trim().trim_start_matches('#').trim();
    let cut: String = sentence.chars().take(SUBJECT_CHARS).collect();
    if sentence.chars().count() > SUBJECT_CHARS {
        return format!("{cut}{ELLIPSIS}");
    }
    cut
}

/// この binary の build 元 commit（`build.rs` が compile time に焼く・設計 consumer-sync.md §2）。
///
/// `--version` の括弧の中身と**同じ 1 つの値**である（3 形: `<sha12>` / `<sha12>+dirty` / `unknown`）。
const GENERATION: &str = BUILD_COMMIT;

/// 台帳の close に書く理由の書き出し（`landed <sha> ci=success` / `landed <sha> ci=none`）。
pub(in crate::pipe) const CLOSE_REASON: &str = "landed";

/// close の理由の尾（**閉じた 2 値**・設計 contract-source.md §5・FR50）。書き手は [`close_reason`] の 1 本で、
/// 経路 (1)（push → CI の照合 → close）と経路 (2)（remote を持たない repo の close）が同じ関数を通る。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::pipe) enum CloseTail<'a> {
    /// CI が success だった（先端で照合した周だけ先端の sha を持つ・§53）。
    CiSuccess(Option<&'a str>),
    /// CI の照合をしていない（remote を持たない repo・先端は持たない）。
    NoCi,
}

/// 台帳の close の理由（`landed <sha> ci=success` / `landed <sha> ci=success tip=<先端>` / `landed <sha> ci=none` の 3 形）。
pub(in crate::pipe) fn close_reason(sha: &str, tail: CloseTail<'_>) -> String {
    match tail {
        CloseTail::CiSuccess(None) => format!("{CLOSE_REASON} {sha} ci=success"),
        CloseTail::CiSuccess(Some(head)) => format!("{CLOSE_REASON} {sha} ci=success tip={head}"),
        CloseTail::NoCi => format!("{CLOSE_REASON} {sha} ci=none"),
    }
}

/// `Landed` の `RunDone` の detail が載せる着地した sha の前置き（[`finish`] が書く字面と同じ 1 本）。
pub(super) const SHA_PREFIX: &str = "sha:";

/// 着地した commit の sha を記録から読む（`pipe land --terminal-only` の入力・設計 §5 手順 3）。
///
/// **終端の event（`terminal:`）は飛ばす**——終端をやり直した周にも、読むのは着地そのものを記した
/// 行の `sha:` である。読めない周は `None`（**HEAD の今の sha に読み替えない**・別の commit の CI を
/// 照合することになる・C10）。
pub(in crate::pipe) fn landed_sha(state_dir: &Path, run: &str) -> Option<String> {
    let events = store::read_all(state_dir).ok()?;
    events
        .iter()
        .rev()
        .filter(|event| event.run == run && event.kind == EventKind::RunDone)
        // **`sha:` を持つ行を探す**（新しい順）。終端の行（`terminal:`）は sha を持たないので、
        // 「最後の RunDone の detail」から読むと終端をやり直した周に読めなくなる。
        .find_map(|event| {
            event.detail.as_deref()?.split_whitespace().find_map(|token| token.strip_prefix(SHA_PREFIX))
        })
        .map(str::to_owned)
}

/// 便の sha が push の先端かどうか（**閉じた 2 値**・設計 contract-source.md §52・§53・行 bd / be）。
///
/// 着地の周で先端を知るのは [`land_train`] の 1 か所だけで、列の最後の便の外に [`Self::Behind`] を先端の sha つきで渡す。
/// 単独の着地は [`Self::Tip`]、`--terminal-only` は anchor の main の今の先端で側を選ぶ（設計 contract-source.md §58）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::pipe) enum PushTip<'a> {
    /// push の先端の commit（forge の CI が run を作る側）。
    Tip,
    /// 先端でない commit（CI の run が付かない＝自分を祖先に持つ先端の commit〔値〕の CI で照合する）。
    Behind(&'a str),
}

/// land の終端（設計 contract-source.md §5）: push → CI の照合 → 台帳の close。
///
/// **各段が typed な event を 1 件ずつ記す**（`RunDone` の detail で弁別）＝通った周は `Landed` の後ろに
/// 3 件並ぶ。止まった段から先は撃たず、記録もそこで終わる（起きていない段の event を積まない）。
/// remote を持たない repo（宣言を読めた上で `remote` の行が無い）の便は push も CI の照合も撃たず、台帳の close だけを
/// `landed <sha> ci=none` の理由で撃つ（記すのは `close:ok` の 1 件・結末は [`Terminal::ClosedWithoutCi`]）。
/// `tip` が [`PushTip::Behind`] の周は push の後に自分の sha が先端の祖先かを測り、祖先の周だけ CI の照合（待ちも
/// `ci_now` も）を先端の sha で撃つ。祖先でない周と測れない周は照合を撃たず `ci:unmeasurable` で止まる（§53）。
pub(in crate::pipe) fn terminal(entry: &Land<'_>, sha: &str, tip: PushTip<'_>) -> Terminal {
    let facts = match super::declaration::terminal_facts(entry.repo) {
        Ok(found) => found,
        // **push を 1 度も撃っていない**ので「push が失敗した」に畳まない（C10）。押す先が在るかを
        // 測れていない周である。
        Err(_) => {
            note(entry, "unreadable");
            return Terminal::Unreadable;
        }
    };
    // 押す先を宣言していない repo は push も CI の照合も撃たない（A1 の「出す」を既定で撃たない）が、bead は閉じる
    // （経路 (2)・ADR-0094）。走らなかった段の event を積むと、記録から「何が起きたか」でなく「何が在るか」が読めなく
    // なるので、記すのは close の 1 件だけである。先端は持たない（`Behind` の周も同じ・照合する CI が無い）。
    let Some(remote) = facts.remote.as_deref() else {
        return match close_bead(entry, &close_reason(sha, CloseTail::NoCi)) {
            Terminal::Closed => Terminal::ClosedWithoutCi,
            failed => failed,
        };
    };
    // (1) push。**main:main だけ**を押す（便の branch は押さない）。
    if super::git_bytes(entry.repo, &["push", remote, "main:main"]).is_none() {
        note(entry, "push:failed:git");
        return Terminal::PushFailed("git".to_owned());
    }
    note(entry, &format!("push:{remote}"));
    // 先端でない sha には forge の CI の run が付かない＝自分を祖先に持つ先端の CI で照合する（§53）。祖先でない周と
    // 測れない周（rc 0 以外は区別しない）は照合を撃たない（close しない極性）。
    let checked = match tip {
        PushTip::Tip => sha,
        PushTip::Behind(head) if git_ok(entry.repo, &["merge-base", "--is-ancestor", sha, head]) => head,
        PushTip::Behind(_) => {
            note(entry, "ci:unmeasurable");
            return Terminal::CiUnmeasurable;
        }
    };
    // (2) CI の照合。上限まで rules 行の間隔で撃ち（設計 §50）、**success 以外は close しない**（FailClosed）。
    let watch = Completion::CiResult {
        repo: entry.repo.to_path_buf(),
        sha: checked.to_owned(),
        cmd: facts.ci_cmd.clone(),
        every: std::time::Duration::from_secs(entry.ci_poll_s),
    };
    let _ = crate::fleet::wait(watch, std::time::Duration::from_secs(entry.ci_wait_s));
    match ci_now(entry.repo, checked, &facts.ci_cmd) {
        None => {
            note(entry, "ci:unmeasurable");
            return Terminal::CiUnmeasurable;
        }
        Some(CiRun::Failure) => {
            note(entry, "ci:failure");
            return Terminal::CiFailed;
        }
        Some(CiRun::Success) => note(entry, "ci:success"),
    }
    // (3) 台帳の close。閉じられない周も着地は取り消さない（やり直しは `--terminal-only`・冪等）。先端で照合した周は
    // reason に先端の id を後置する（FR50・§53）。
    let tail = match tip {
        PushTip::Tip => CloseTail::CiSuccess(None),
        PushTip::Behind(head) => CloseTail::CiSuccess(Some(head)),
    };
    close_bead(entry, &close_reason(sha, tail))
}

/// 台帳の close を撃ち、結末（[`Terminal::Closed`] か [`Terminal::CloseFailed`]）を記す（経路 (1) と (2) が共有する 1 本）。
fn close_bead(entry: &Land<'_>, reason: &str) -> Terminal {
    match crate::ledger::close(entry.bd, entry.repo, entry.bead, reason) {
        Ok(()) => {
            note(entry, "close:ok");
            Terminal::Closed
        }
        Err(err) => {
            let failed = err.render();
            note(entry, &failed);
            Terminal::CloseFailed(failed)
        }
    }
}

/// 終端の 1 段を記す（`RunDone stage=Landed` の detail・**段の数だけ呼ばれる**）。
///
/// 記帳できない周も結末は変えない——着地は成立していて取り消せないので、記録の欠けは store の
/// error として別に出る（段の判定を記録の可否に従わせない）。
fn note(entry: &Land<'_>, detail: &str) {
    let _ = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("terminal:{detail}")),
        },
        entry.policy,
    );
}

/// export → `Landed` → 後始末。ここまで来た周は land が成立している（anchor は呼び手が揃え済み）。
///
/// export の前に main を実測し、stdout の `main=` と `Landed` の detail の `main:` に写す
/// （`sha:` は宣言値のまま・verdicts.jsonl の key 列は触らない）。
///
/// `landing` は着地の形と宣言値の sha（設計 §29）: [`Landing::AlreadyLanded`] の周は stdout の末尾に
/// `already-landed=1`、detail の `main:` の後ろに `already-landed` を後置する。[`Landing::Fresh`] の周の stdout と
/// detail は不変。verdicts.jsonl の行はどちらも従来の key 列（`sha` = 宣言値・任意 field を足さない）。
///
/// `turned` は番待ちの結果（設計 pipeline.md §36）: 札が死んでいて列から外した便が在る周は stdout の `order=` の値の
/// 直後に `skipped-dead=<n>`、面 5 の行に `skipped_dead` を足す（0 本の周は書かない）。
/// `anchor` は揃えた結果と印（設計 §57 形 2）: 印を置いた周だけ detail の末尾に ` anchor=skipped:<理由>`（synced と not-main
/// は空）。印の stderr の行は呼び手が 1 度だけ足す（列の便ごとに重ねない）。[`Landing::Behind`] の周だけ [`terminal`] へ
/// 先端の sha つきの [`PushTip::Behind`] を運ぶ（設計 contract-source.md §52・§53）。
pub(super) fn finish(entry: &Land<'_>, worktree: &Path, landing: &Landing, anchor: &Anchored, turned: &Turned) -> Outcome {
    let new = landing.sha();
    let order = turned.order;
    let (measured, mut err) = measure_main(entry.repo);
    if let Err(reason) = export_verdict(entry, new, turned) {
        return broken(reason);
    }
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("{SHA_PREFIX}{new} main:{measured}{}{}", landing.detail_suffix(), anchor.detail)),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    // 着地後の検出を切り離して起こす（設計 gate-cost.md §44 形 (11)・待たない）。起こせなかった周も rc と着地は変えない。
    err.extend(spawn_detection(entry, new));
    // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr 1 行**）。anchor の warning も同じ列。
    err.extend(retire_worktree(entry.repo, entry.run, worktree));
    err.extend(anchor.sync.warning());
    // **終端**（設計 contract-source.md §5）: push → CI の照合 → 台帳の close。着地は既に成立している
    // ので、終端が止まっても取り消さない——止まった事実を typed な event と token で残し rc を 1 にする。
    let tip = match landing {
        Landing::Behind { tip, .. } => PushTip::Behind(tip),
        Landing::Fresh(_) | Landing::AlreadyLanded(_) => PushTip::Tip,
    };
    let terminal = terminal(entry, new, tip);
    // **局面の出力の書き直し（契機 (d)）は終端が close した周（rc 0）に**（設計 case-lifecycle.md §12 約束 8）: 呼び手の rc と stdout は変えず、
    // `Written`・`Unchanged`・`Coalesced` の外の語だけ stderr の 1 行にする。
    if let (RC_OK, Ok(rules)) = (terminal.rc(), crate::rules::read(entry.rules, Some(entry.state_dir))) {
        let place = Place { state_dir: entry.state_dir, repo: entry.repo, manifest: &rules, bd: entry.bd, policy: entry.policy };
        err.extend(lifecycle::after_close(&place));
    }
    let skipped = match turned.skipped_dead.len() {
        0 => String::new(),
        count => format!(" skipped-dead={count}"),
    };
    Outcome {
        out: vec![format!(
            "run={} landed={new} main={measured} {} order={}{skipped}{} terminal={}",
            entry.run,
            anchor.sync.token(),
            order.as_value(),
            landing.stdout_suffix(),
            terminal.as_token()
        )],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr**）。
        err,
        rc: terminal.rc(),
    }
}

/// 着地後の検出の口（`pipe land --run <id> --detection-only`）を子 process で起こし、終わりを待たない（設計 gate-cost.md
/// §44 形 (11)・行 am）。起こすのは列が便を起こすのと同じ 1 本（[`spawn_self`]・新しい process group・stderr は起動の log）。
///
/// 宣言の写しに検出線の行が在る便だけ起こす（写しを読めない周も起こさない＝口が断る形を起こさない）。起こせた周は
/// `detection:spawned`、起こせなかった周は `detection:unspawned` を 1 件記し、record と理由の file（`unmeasured=unspawned`）
/// は行 ak の書き手（[`unfired`]）が置いて stderr の行を返す。どの周も rc と着地は変えない（検出線は止めない線・C12.4）。
fn spawn_detection(entry: &Land<'_>, sha: &str) -> Vec<String> {
    let declared = Effective::load(&vessel_path(entry.state_dir, entry.run))
        .is_ok_and(|frozen| !frozen.detection_verify().is_empty());
    if !declared {
        return Vec::new();
    }
    let tree = git_line(entry.repo, &["rev-parse", &format!("{sha}^{{tree}}")]);
    let mark = LandedMark { sha, tree: tree.as_deref().unwrap_or(MAIN_UNKNOWN), since: None, runs: 0 };
    let detect = Detect {
        run: entry.run,
        bead: entry.bead,
        repo: entry.repo,
        state_dir: entry.state_dir,
        contract: entry.contract,
        limits: entry.limits,
        policy: entry.policy,
        daily: crate::rules::read(entry.rules, Some(entry.state_dir)).ok().as_ref().and_then(daily_floor),
    };
    // 日次の検出（設計 gate-cost.md §50）: 下限の内の周は口を起こさず deferred を記す。行を読めない周は着地ごとに起こす。
    let mut err = match wake(&detect) {
        Wake::Fire(notes) => notes,
        Wake::Defer => return defer_detection(entry, &detect),
    };
    let mut argv: Vec<String> = ["land", "--run", entry.run, "--detection-only", "--state-dir"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    argv.extend([entry.state_dir.display().to_string(), "--repo".to_owned(), entry.repo.display().to_string()]);
    if let Some(rules) = entry.rules {
        argv.extend(["--rules".to_owned(), rules.display().to_string()]);
    }
    let spawned = spawn_self(entry.state_dir, &argv);
    let word = if spawned { SPAWNED } else { UNSPAWNED };
    if !spawned {
        err.push(format!("pipe: run {} の着地後の検出を起こせなかった（unmeasured={UNSPAWNED}）", entry.run));
        if let Err(reason) = unfired(&detect, mark, Unfired::Unmeasured(UNSPAWNED)) {
            err.push(format!("pipe: {reason}"));
        }
    }
    err.extend(note_detection(entry, word));
    err
}

/// 下限の内で口を起こさなかった周: 理由の file（`skipped=deferred`・書き手は口の書き手の file の 1 本）と detail
/// `detection:deferred` を 1 件残す。record は足さない・起点は書かない。書けない周は stderr の 1 行（rc と着地は変えない）。
fn defer_detection(entry: &Land<'_>, detect: &Detect<'_>) -> Vec<String> {
    let mut err = Vec::new();
    if let Err(reason) = deferred(detect) {
        err.push(format!("pipe: {reason}"));
    }
    err.extend(note_detection(entry, DEFERRED));
    err
}

/// `RunDone stage=Landed` の detail に `detection:<語>` を 1 件記す（記帳できない周は stderr の 1 行）。
fn note_detection(entry: &Land<'_>, word: &str) -> Option<String> {
    let emitted = emit(
        entry.state_dir,
        &Emit {
            kind: EventKind::RunDone,
            run: entry.run,
            bead: entry.bead,
            stage: Some(Stage::Landed),
            seat: None,
            pid: None,
            detail: Some(format!("{DETAIL_HEAD}{word}")),
        },
        entry.policy,
    );
    emitted.err().map(|reason| format!("pipe: {reason}"))
}

/// 候補の木に積んだ便 1 本（[`land_train`] の材料・設計 pipeline.md §40・行 ah）。
pub(in crate::pipe) struct Car<'a> {
    /// 便の land の材料（後続は先頭の材料から契約・bead・承認だけを差し替えた形）。
    pub(in crate::pipe) entry: Land<'a>,
    /// 便の worktree（着地の後に退避する）。
    pub(in crate::pipe) worktree: PathBuf,
    /// 候補の木で自分を積んだ段の tree（squash の材料＝worktree の tree の代わり）。
    pub(in crate::pipe) tree: String,
    /// 面 5 と stdout の `order=`（先頭は自分の番・後続は [`Order::Train`]）。
    pub(in crate::pipe) order: Order,
}

/// 候補の木の緑を受けて列を着地させる（設計 pipeline.md §40・行 ah）。
///
/// 段の tree ごとに `commit-tree`（親 = 直前の着地 commit・message は便ごとの [`squash_message`]）で連ね、main を
/// **CAS で N 本ぶん**進める（old = 候補の木を切った main）。進められなかった周は `Err`＝main は 1 byte も動いて
/// いない（呼び手は列を解く）。進めた後は従来と同じ順（anchor を揃える → 主実測 [`verify_main`] を**先端の木で
/// 1 回**）で、緑なら便ごとに [`finish`]（面 5 の 1 行・`Landed`・worktree の退避・終端）、赤 / 測れない周は列の便
/// すべてに `main-red` / `main-unmeasured`（main は進んだまま・巻き戻さない・どの便が赤かは帰属しない）。
///
/// **記帳は後続 → 先頭の順**である: 後ろの便の番待ち（`await_turn`）は先頭が列を空けた瞬間に起きて段を読むので、
/// 先頭を先に終端させると、自分の `Landed` が書かれる前に起きた後続が追随へ進む窓が開く。stdout は列の順に並べる。
pub(in crate::pipe) fn land_train(cars: &[Car<'_>], old: &str) -> Result<Outcome, String> {
    let head = cars.first().ok_or_else(|| "候補の木に便が無い".to_owned())?;
    let repo = head.entry.repo;
    // anchor の見立ては ref を進める前に読む（[`super::attempt`] と同じ順）。
    let plan = super::anchor_plan(repo);
    let mut parent = old.to_owned();
    let mut shas = Vec::new();
    for car in cars {
        let message = squash_message(car.entry.bead, &car.entry.contract.goal, car.entry.run, car.entry.contract);
        let new = git_line(repo, &["commit-tree", &car.tree, "-p", &parent, "-m", &message])
            .ok_or_else(|| format!("run {} の段の commit を作れない", car.entry.run))?;
        shas.push(new.clone());
        parent = new;
    }
    if !git_ok(repo, &["update-ref", MAIN_REF, &parent, old]) {
        return Err(format!("{MAIN_REF} を付け替えられない（CAS が外れた）"));
    }
    let anchor = super::anchor::record(repo, super::sync_anchor(repo, &plan, old, &parent), old, &parent);
    // 主実測の write-set 照合は `old..先端` を列の write-set の和で測る（契約 verify は先頭の分・従来どおり）。
    let mut joined = head.entry.contract.clone();
    for item in cars.iter().skip(1).flat_map(|car| &car.entry.contract.write_set) {
        if !joined.write_set.contains(item) {
            joined.write_set.push(item.clone());
        }
    }
    let check = verify_train_main(&Land { contract: &joined, ..head.entry }, &parent, old);
    let mut outcomes: Vec<Option<Outcome>> = cars.iter().map(|_| None).collect();
    let followers_first = (1..cars.len()).chain(std::iter::once(0));
    for index in followers_first {
        let (Some(car), Some(sha)) = (cars.get(index), shas.get(index)) else {
            continue;
        };
        let outcome = match &check {
            MainCheck::Green => {
                let turned = Turned { order: car.order, skipped_dead: Vec::new() };
                // 先端（main を進めた最後の commit）を知るのはここだけ（設計 contract-source.md §52）。
                let landing = if index + 1 == cars.len() {
                    Landing::Fresh(sha.clone())
                } else {
                    Landing::Behind { sha: sha.clone(), tip: parent.clone() }
                };
                finish(&car.entry, &car.worktree, &landing, &anchor, &turned)
            }
            MainCheck::Red(reason) => main_red(&car.entry, reason, &anchor.sync),
            MainCheck::Unmeasurable(reason) => main_unmeasured(&car.entry, reason, &anchor.sync),
        };
        if let Some(slot) = outcomes.get_mut(index) {
            *slot = Some(outcome);
        }
    }
    let mut merged = Outcome { out: Vec::new(), err: Vec::new(), rc: RC_OK };
    for outcome in outcomes.into_iter().flatten() {
        merged.out.extend(outcome.out);
        merged.err.extend(outcome.err);
        merged.rc = merged.rc.max(outcome.rc);
    }
    merged.err.extend(anchor.err);
    Ok(merged)
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
///
/// `order` は schema 1 のまま足した**任意 field**（ADR-0021 §2.6 (iv)・古い読み手は無視する）で、
/// 列の後ろに置く（既存の 7 key の並びは動かさない）。便の規模の 4 field（[`size::fields`]・git を読めない周は欠く）はその後ろ。
/// 札が死んでいて列から外した便が在る周だけ、さらに後ろに任意 field `skipped_dead`（便 id を鍵の順に `,` で連ねた
/// 文字列・設計 pipeline.md §36）を足す。
fn export_verdict(entry: &Land<'_>, new: &str, turned: &Turned) -> Result<(), String> {
    let order = turned.order;
    let evidence = verdict_path(entry.state_dir, entry.run).display().to_string();
    let mut pairs = vec![
        ("schema", Value::Num(SCHEMA)),
        ("run", Value::Str(entry.run.to_owned())),
        ("bead", Value::Str(entry.bead.to_owned())),
        ("sha", Value::Str(new.to_owned())),
        ("verdict", Value::Str(Verdict::Pass.as_str().to_owned())),
        ("evidence", Value::Str(evidence)),
        ("ts", Value::Str(now_utc())),
        ("order", Value::Str(order.as_value())),
        // **binary の世代**（設計 contract-source.md §5 手順 4）= **この着地を作った binary の build 元 commit**
        // （§2 の値・`--version` の括弧の中身と同じ 1 本）。自分の版が古い周に起動を断るかは後続（§12）で、
        // ここは事実を残すだけである。**着地した sha は同じ行の `sha` が既に持つ**ので、同値の欄を 2 つ
        // 並べない——2 つ在ると読み手はどちらを版の比較に使うのか判じられない（C10）。
        ("generation", Value::Str(GENERATION.to_owned())),
    ];
    // verdict の size の材料は base が分かった周だけ載る（無い周も読めない周も同じ＝欄を持たない）。
    let base = super::base_of_run(entry.state_dir, entry.run).known();
    pairs.extend(size::fields(&entry.contract.size, base.as_deref(), new, |args| git_bytes(entry.repo, args)));
    if !turned.skipped_dead.is_empty() {
        pairs.push(("skipped_dead", Value::Str(turned.skipped_dead.join(","))));
    }
    let line = json_lite::write_object(&pairs);
    append_line(&verdicts_path(entry.state_dir), &line, entry.policy)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{open_pr, Land};
    use crate::cli_outcome::RC_OK;
    use crate::fleet::store::LockPolicy;
    use crate::pipe::fixture::{contract, exited, scratch, Stub};
    use crate::pipe::gate::Limits;
    use crate::pipe::lens_record::LensSource;

    /// PR を開く行は起動の記述を通る（設計 core-boundary.md §9 行 d）: 記録の program は `sh`・引数は `-c` と穴を
    /// 埋めた行・cwd は repo。stub の rc 0 は「PR を出した」として従来どおり `landed=pr` を返す。行は stub を据えない
    /// 周に撃たれても何もしない `true` で始める。
    #[test]
    fn invocation_pipe_flow_finish_pr_line_goes_through_the_seam() {
        let root = scratch("finish-pr-line");
        let (repo, state) = (root.join("repo"), root.join("state"));
        let _ = std::fs::create_dir_all(&repo);
        let _ = std::fs::create_dir_all(&state);
        let (policy, contract) = (LockPolicy::embedded().expect("埋め込みの lock 規則を読める"), contract(&[], &[]));
        let limits = Limits {
            lens_count: 0,
            token_cap: 0,
            mutants_jobs: 0,
            job_memory_mb: 0,
            reserve_memory_mb: 0,
            slot_wait_s: 0,
            runnable_per_core: 0,
            blocked_per_core: 0,
        };
        let entry = Land {
            run: "r-pr",
            bead: "s2-mutant",
            repo: &repo,
            state_dir: &state,
            contract: &contract,
            pr_cmd: None,
            lens: &LensSource::Absent,
            limits,
            runner: None,
            retries: 0,
            land_wait_s: 0,
            ci_wait_s: 0,
            ci_poll_s: 0,
            bd: crate::ledger::DEFAULT_BD,
            approved: false,
            policy,
            train_max: 1,
            rules: None,
        };
        let stub = Stub::install(|_| exited(0, b""));
        let out = open_pr(&entry, "base-sha", "true {branch} {base}");
        assert_eq!((out.rc, out.out), (RC_OK, vec!["run=r-pr landed=pr".to_owned()]), "rc 0 は PR を出した");
        let calls = stub.calls();
        assert_eq!(calls.len(), 1, "起動は 1 回: {calls:?}");
        let found: Vec<(String, Vec<String>, Option<std::path::PathBuf>)> =
            calls.into_iter().map(|call| (call.program, call.args, call.cwd)).collect();
        let line = format!("true {} base-sha", super::super::branch_name("r-pr"));
        assert_eq!(found, vec![("sh".to_owned(), vec!["-c".to_owned(), line], Some(repo.clone()))], "sh -c の行と cwd");
        let _ = std::fs::remove_dir_all(&root);
    }
}
