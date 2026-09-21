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
use super::super::gate::Verdict;
use super::super::queue::Order;
use super::super::{emit, git_bytes, git_line, git_ok, size, verdict_path, Emit};
use super::verify::measure_main;
use super::{
    broken, refused, retire_worktree, verdicts_path, AnchorSync, Land, Landing, Terminal, MAIN_REF, RUN_TRAILER,
};
use crate::cli_outcome::Outcome;
use crate::fleet::json_lite::{self, Value};
use crate::fleet::store::{self, append_line};
use crate::fleet::{ci_now, cli::now_utc, CiRun, Completion, EventKind, Stage, SCHEMA};
use crate::name::NAME;
use std::path::Path;

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
    let ran = std::process::Command::new("sh")
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
const GENERATION: &str = env!("SCRIBE2_BUILD_COMMIT");

/// 台帳の close に書く理由の書き出し（`landed <sha> ci=success`）。
const CLOSE_REASON: &str = "landed";

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

/// land の終端（設計 contract-source.md §5）: push → CI の照合 → 台帳の close。
///
/// **各段が typed な event を 1 件ずつ記す**（`RunDone` の detail で弁別）＝通った周は `Landed` の後ろに
/// 3 件並ぶ。止まった段から先は撃たず、記録もそこで終わる（起きていない段の event を積まない）。
pub(in crate::pipe) fn terminal(entry: &Land<'_>, sha: &str) -> Terminal {
    let facts = match super::declaration::terminal_facts(entry.repo) {
        Ok(found) => found,
        // **push を 1 度も撃っていない**ので「push が失敗した」に畳まない（C10）。押す先が在るかを
        // 測れていない周である。
        Err(_) => {
            note(entry, "unreadable");
            return Terminal::Unreadable;
        }
    };
    // 押す先を宣言していない repo は**終端を持たない**（A1 の「出す」を既定で撃たない）。1 件も記帳しない
    // ——走らなかった段の event を積むと、記録から「何が起きたか」でなく「何が在るか」が読めなくなる。
    let Some(remote) = facts.remote.as_deref() else {
        return Terminal::Undeclared;
    };
    // (1) push。**main:main だけ**を押す（便の branch は押さない）。
    if super::git_bytes(entry.repo, &["push", remote, "main:main"]).is_none() {
        note(entry, "push:failed:git");
        return Terminal::PushFailed("git".to_owned());
    }
    note(entry, &format!("push:{remote}"));
    // (2) CI の照合。上限まで待ち、**success 以外は close しない**（FailClosed）。
    let watch = Completion::CiResult {
        repo: entry.repo.to_path_buf(),
        sha: sha.to_owned(),
        cmd: facts.ci_cmd.clone(),
    };
    let _ = crate::fleet::wait(watch, std::time::Duration::from_secs(entry.ci_wait_s));
    match ci_now(entry.repo, sha, &facts.ci_cmd) {
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
    // (3) 台帳の close。閉じられない周も着地は取り消さない（やり直しは `--terminal-only`・冪等）。
    match crate::ledger::close(entry.bd, entry.bead, &format!("{CLOSE_REASON} {sha} ci=success")) {
        Ok(()) => {
            note(entry, "close:ok");
            Terminal::Closed
        }
        Err(err) => {
            let reason = err.render();
            note(entry, &reason);
            Terminal::CloseFailed(reason)
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
pub(super) fn finish(entry: &Land<'_>, worktree: &Path, landing: &Landing, anchor: &AnchorSync, order: Order) -> Outcome {
    let new = landing.sha();
    let (measured, mut err) = measure_main(entry.repo);
    if let Err(reason) = export_verdict(entry, new, order) {
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
            detail: Some(format!("{SHA_PREFIX}{new} main:{measured}{}", landing.detail_suffix())),
        },
        entry.policy,
    );
    if let Err(err) = emitted {
        return broken(err.to_string());
    }
    // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr 1 行**）。anchor の warning も同じ列。
    err.extend(retire_worktree(entry.repo, entry.run, worktree));
    err.extend(anchor.warning());
    // **終端**（設計 contract-source.md §5）: push → CI の照合 → 台帳の close。着地は既に成立している
    // ので、終端が止まっても取り消さない——止まった事実を typed な event と token で残し rc を 1 にする。
    let terminal = terminal(entry, new);
    Outcome {
        out: vec![format!(
            "run={} landed={new} main={measured} {} order={}{} terminal={}",
            entry.run,
            anchor.token(),
            order.as_value(),
            landing.stdout_suffix(),
            terminal.as_token()
        )],
        // 後始末の失敗は land を取り消さない（**rc 0 のまま stderr**）。
        err,
        rc: terminal.rc(),
    }
}

/// 面 5 の 1 行を `verdicts.jsonl` へ append する（跨版 契約・key 列は固定）。
///
/// `order` は schema 1 のまま足した**任意 field**（ADR-0021 §2.6 (iv)・古い読み手は無視する）で、
/// 列の後ろに置く（既存の 7 key の並びは動かさない）。便の規模の 4 field（[`size::fields`]・git を読めない周は欠く）はその後ろ。
fn export_verdict(entry: &Land<'_>, new: &str, order: Order) -> Result<(), String> {
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
    let line = json_lite::write_object(&pairs);
    append_line(&verdicts_path(entry.state_dir), &line, entry.policy)
        .map(|_| ())
        .map_err(|err| err.to_string())
}
