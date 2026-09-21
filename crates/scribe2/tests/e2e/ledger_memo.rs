//! memo の入口（設計 docs/design/ledger-form.md §3 の 8・§6 行 c・接頭辞 `ledger_memo_plan_`）。
//!
//! 置き場は tmp の state dir（event log は固定 ts で積み、run dir に `verdict.json` / `review.json` を置く）で、実 binary の
//! `ledger memo` を撃つ。PATH の先頭には argv を記録する偽の `bd` を置き、**1 回も呼ばれない**ことを毎回測る。

use super::{make_tmp_dir, TmpDir};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vessel::fleet::{Event, EventKind, Stage};

/// 歯の置き場（state dir・偽の client の dir・argv の記録）。
struct Place {
    dir: TmpDir,
    state: PathBuf,
    bin: PathBuf,
    record: PathBuf,
}

/// 置き場を作り、argv を記録する偽の `bd` を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn place() -> Place {
    use std::os::unix::fs::PermissionsExt;
    let dir = make_tmp_dir().expect("置き場を作れる");
    let (state, bin, record) = (dir.join("state"), dir.join("bin"), dir.join("bd-args"));
    fs::create_dir_all(&state).expect("state dir を作れる");
    fs::create_dir_all(&bin).expect("bin を作れる");
    let shim = bin.join("bd");
    fs::write(&shim, format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", record.display())).expect("偽の client を書ける");
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).expect("実行権を付ける");
    Place { dir, state, bin, record }
}

/// 便の event を 1 件積む（固定 ts・`run` の run dir も作る）。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn push(place: &Place, run: &str, (kind, stage): (EventKind, Option<Stage>), detail: Option<&str>, ts: &str) {
    fs::create_dir_all(vessel::pipe::run_dir(&place.state, run)).expect("run dir を作れる");
    let event = Event {
        schema: vessel::fleet::SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: run.to_owned(),
        bead: "s2-x.1".to_owned(),
        host: "h".to_owned(),
        actor: vessel::fleet::ACTOR_MACHINE.to_owned(),
        stage,
        seat: None,
        pid: None,
        detail: detail.map(str::to_owned),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
    };
    let policy = vessel::fleet::store::LockPolicy::embedded().expect("埋め込みの lock 行を読める");
    vessel::fleet::store::append(&place.state, &event, policy).expect("event を積める");
}

/// 段の event を 1 件積む。
fn stage(place: &Place, run: &str, found: Stage, detail: Option<&str>, ts: &str) {
    push(place, run, (EventKind::RunStage, Some(found)), detail, ts);
}

/// run dir に file を置く。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn put(place: &Place, run: &str, name: &str, body: &str) {
    fs::write(vessel::pipe::run_dir(&place.state, run).join(name), body).expect("run dir に file を置ける");
}

/// 撃った結果（rc・stdout・stderr）。
struct Shot {
    rc: Option<i32>,
    out: String,
    err: String,
}

/// 偽の `bd` を先頭に積んだ PATH で `ledger <args>` を撃ち、**偽の `bd` が呼ばれていない**ことを確かめて返す。
#[expect(
    clippy::expect_used,
    reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く"
)]
fn shoot(place: &Place, args: &[&str]) -> Shot {
    let path = format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default());
    let out = Command::new(env!("CARGO_BIN_EXE_scribe2"))
        .arg("ledger")
        .args(args)
        .env("PATH", path)
        .output()
        .expect("binary を起動できる");
    assert!(!place.record.exists(), "台帳 client を 1 回も呼ばない: {args:?}");
    Shot {
        rc: out.status.code(),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// `--run <run>` の plan を撃ち、rc 0・stderr 0 byte を確かめて stdout の行を返す。
fn plan_of(place: &Place, run: &str, extra: &[&str]) -> Vec<String> {
    let state = place.state.display().to_string();
    let mut args = vec!["memo", "--run", run, "--state-dir", state.as_str(), "--parent", "s2-epic"];
    args.extend_from_slice(extra);
    let shot = shoot(place, &args);
    assert_eq!(shot.rc, Some(0), "plan は rc 0: {}", shot.err);
    assert_eq!(shot.err, "", "plan の周は stderr 0 byte");
    shot.out.lines().map(str::to_owned).collect()
}

/// 便の終端の plan の全行（引数の 4 行 → `body:` → 4 節・候補と昇格条件は空の見出し）。
fn want_run(title: &str, source: &[&str], observed: &[&str]) -> Vec<String> {
    let mut lines = vec![
        format!("arg: --title={title}"),
        "arg: --type=task".to_owned(),
        "arg: --labels=intake:memo".to_owned(),
        "arg: --parent=s2-epic".to_owned(),
        "body:".to_owned(),
        "## memo".to_owned(),
        "### 出所".to_owned(),
    ];
    lines.extend(source.iter().map(|line| (*line).to_owned()));
    lines.push("### 観測".to_owned());
    lines.extend(observed.iter().map(|line| (*line).to_owned()));
    lines.extend(["### 候補".to_owned(), "### 昇格条件".to_owned()]);
    lines
}

/// (1) gate の終端: `verdict.json` の evidence と at が観測へ写る（FAIL は at つき・INCONCLUSIVE は at の無い file＝`-`）。
#[test]
fn ledger_memo_plan_gated_copies_the_verdict_evidence_and_at() {
    let place = place();
    stage(&place, "g-fail", Stage::Spawned, None, "2026-09-20T00:00:01Z");
    stage(&place, "g-fail", Stage::Gated, Some("verdict:FAIL"), "2026-09-20T00:00:02Z");
    put(&place, "g-fail", "verdict.json", "{\"schema\":1,\"run\":\"g-fail\",\"verdict\":\"FAIL\",\"evidence\":\"verify 2/3 赤 q7\",\"at\":\"crates/q7.rs\",\"ts\":\"2026-09-20T00:00:02Z\"}\n");
    stage(&place, "g-inc", Stage::Gated, Some("verdict:INCONCLUSIVE"), "2026-09-20T00:00:03Z");
    put(&place, "g-inc", "verdict.json", "{\"schema\":1,\"run\":\"g-inc\",\"verdict\":\"INCONCLUSIVE\",\"evidence\":\"lens の出力が壊れ k4\"}\n");
    assert_eq!(
        plan_of(&place, "g-fail", &[]),
        want_run(
            "[memo] g-fail Gated FAIL",
            &["- run: g-fail", "- stage: Gated", "- kind: FAIL"],
            &["- evidence: verify 2/3 赤 q7", "- at: crates/q7.rs"],
        ),
        "gate の FAIL"
    );
    assert_eq!(
        plan_of(&place, "g-inc", &[]),
        want_run(
            "[memo] g-inc Gated INCONCLUSIVE",
            &["- run: g-inc", "- stage: Gated", "- kind: INCONCLUSIVE"],
            &["- evidence: lens の出力が壊れ k4", "- at: -"],
        ),
        "gate の INCONCLUSIVE（at の無い file は `-`・空に潰さない）"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 審査の終端: `review.json` の evidence と at が観測へ、理由の型が出所の kind へ写る。
#[test]
fn ledger_memo_plan_reviewed_copies_the_review_evidence_and_at() {
    let place = place();
    stage(&place, "r-fail", Stage::Reviewed, Some("verdict:FAIL kind:section-material-missing"), "2026-09-20T00:01:00Z");
    put(&place, "r-fail", "review.json", "{\"schema\":1,\"run\":\"r-fail\",\"verdict\":\"FAIL\",\"evidence\":\"§3 に原本の形が無い z9\",\"kind\":\"section-material-missing\",\"at\":\"§3,行 c\",\"ts\":\"2026-09-20T00:01:00Z\"}\n");
    assert_eq!(
        plan_of(&place, "r-fail", &[]),
        want_run(
            "[memo] r-fail Reviewed FAIL section-material-missing",
            &["- run: r-fail", "- stage: Reviewed", "- kind: FAIL section-material-missing"],
            &["- evidence: §3 に原本の形が無い z9", "- at: §3,行 c"],
        ),
        "審査の FAIL"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 質問の終端: event log の質問の逐語と about が観測へ写る（run dir に file は無い）。
#[test]
fn ledger_memo_plan_questioned_copies_the_question_and_about() {
    let place = place();
    stage(&place, "q-run", Stage::Spawned, None, "2026-09-20T00:02:00Z");
    push(&place, "q-run", (EventKind::QuestionRaised, None), Some("write-set に w5.rs が無い"), "2026-09-20T00:02:01Z");
    stage(&place, "q-run", Stage::Questioned, Some("about:write-set"), "2026-09-20T00:02:02Z");
    assert_eq!(
        plan_of(&place, "q-run", &[]),
        want_run(
            "[memo] q-run Questioned question",
            &["- run: q-run", "- stage: Questioned", "- kind: question"],
            &["- question: write-set に w5.rs が無い", "- about: write-set"],
        ),
        "質問の逐語と about"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (1) 落ちた終端: **最後の** `RunStage(Failed)` の detail と ts が観測へ写る（前の Failed の字面は写らない）。
#[test]
fn ledger_memo_plan_failed_copies_the_last_failed_detail_and_ts() {
    let place = place();
    stage(&place, "f-run", Stage::Failed, Some("main-red"), "2026-09-20T00:03:00Z");
    stage(&place, "f-run", Stage::Spawned, None, "2026-09-20T00:03:01Z");
    stage(&place, "f-run", Stage::Failed, Some("rebase-conflict:a1..b2"), "2026-09-20T00:03:07Z");
    let lines = plan_of(&place, "f-run", &[]);
    assert_eq!(
        lines,
        want_run(
            "[memo] f-run Failed failed",
            &["- run: f-run", "- stage: Failed", "- kind: failed"],
            &["- detail: rebase-conflict:a1..b2", "- ts: 2026-09-20T00:03:07Z"],
        ),
        "最後の Failed の detail と ts"
    );
    assert!(!lines.iter().any(|line| line.contains("main-red")), "前の Failed は写らない: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (1)(2) 断りは閉じた理由の 1 行（stderr）・rc 1・stdout 0 byte: run dir の無い id／終端でない段／verdict が PASS／
/// log に便が無い／原本の無い終端（detail 空・file 無し・evidence 空・質問無し）。
#[test]
fn ledger_memo_plan_refuses_non_terminal_and_materialless_runs() {
    let place = place();
    stage(&place, "s-run", Stage::Spawned, None, "2026-09-20T00:04:00Z");
    stage(&place, "p-run", Stage::Gated, Some("verdict:PASS"), "2026-09-20T00:04:01Z");
    put(&place, "p-run", "verdict.json", "{\"verdict\":\"PASS\",\"evidence\":\"ok\"}\n");
    stage(&place, "l-run", Stage::Landed, None, "2026-09-20T00:04:02Z");
    fs::create_dir_all(vessel::pipe::run_dir(&place.state, "bare-run")).ok();
    stage(&place, "fe-run", Stage::Failed, Some(" "), "2026-09-20T00:04:03Z");
    stage(&place, "gn-run", Stage::Gated, Some("verdict:FAIL"), "2026-09-20T00:04:04Z");
    stage(&place, "re-run", Stage::Reviewed, Some("verdict:FAIL"), "2026-09-20T00:04:05Z");
    put(&place, "re-run", "review.json", "{\"verdict\":\"FAIL\",\"evidence\":\"\",\"at\":\"x\"}\n");
    stage(&place, "qn-run", Stage::Questioned, Some("about:goal"), "2026-09-20T00:04:06Z");
    let cases = [
        ("nope", "memo: refused reason=no-run-dir run=nope"),
        ("s-run", "memo: refused reason=not-terminal run=s-run stage=Spawned"),
        ("p-run", "memo: refused reason=not-terminal run=p-run stage=Gated"),
        ("l-run", "memo: refused reason=not-terminal run=l-run stage=Landed"),
        ("bare-run", "memo: refused reason=not-terminal run=bare-run stage=-"),
        ("fe-run", "memo: refused reason=no-material run=fe-run stage=Failed"),
        ("gn-run", "memo: refused reason=no-material run=gn-run stage=Gated"),
        ("re-run", "memo: refused reason=no-material run=re-run stage=Reviewed"),
        ("qn-run", "memo: refused reason=no-material run=qn-run stage=Questioned"),
    ];
    let state = place.state.display().to_string();
    for (run, want) in cases {
        let shot = shoot(&place, &["memo", "--run", run, "--state-dir", &state, "--parent", "s2-epic"]);
        assert_eq!(shot.rc, Some(1), "{run}: rc 1");
        assert_eq!(shot.out, "", "{run}: plan を出さない");
        assert_eq!(shot.err, format!("{want}\n"), "{run}: 閉じた理由の 1 行");
    }
    fs::remove_dir_all(&place.dir).ok();
}

/// (3) `--from user` の plan: 出所は逐語の在り処の 1 行と日付（UTC の今日）・観測は空の見出し。
#[test]
fn ledger_memo_plan_from_user_carries_the_source_line_and_date() {
    let place = place();
    let before = vessel::fleet::cli::now_utc().get(..10).unwrap_or_default().to_owned();
    let shot = shoot(&place, &["memo", "--from", "user", "--parent", "s2-epic"]);
    let after = vessel::fleet::cli::now_utc().get(..10).unwrap_or_default().to_owned();
    assert_eq!(shot.rc, Some(0), "rc 0: {}", shot.err);
    assert_eq!(shot.err, "", "stderr 0 byte");
    let lines: Vec<String> = shot.out.lines().map(str::to_owned).collect();
    let date = if lines.contains(&format!("- date: {after}")) { after } else { before };
    let want = vec![
        format!("arg: --title=[memo] user {date}"),
        "arg: --type=task".to_owned(),
        "arg: --labels=intake:memo".to_owned(),
        "arg: --parent=s2-epic".to_owned(),
        "body:".to_owned(),
        "## memo".to_owned(),
        "### 出所".to_owned(),
        "- user 逐語は本 bead の notes".to_owned(),
        format!("- date: {date}"),
        "### 観測".to_owned(),
        "### 候補".to_owned(),
        "### 昇格条件".to_owned(),
    ];
    assert_eq!(lines, want, "出所の 1 行と日付・観測は空");
    assert_eq!(date.len(), 10, "日付は YYYY-MM-DD: {date}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) label `intake:memo`・引数の parent・引数の関連 bead ごとの `relates-to` が plan の引数に載る（両方の出所で）。
/// parent の無い周は plan を出さない（使い方の誤り・rc 1）。
#[test]
fn ledger_memo_plan_carries_label_parent_and_relates_to() {
    let place = place();
    stage(&place, "f-rel", Stage::Failed, Some("main-red"), "2026-09-20T00:05:00Z");
    let head = |lines: &[String]| -> Vec<String> { lines.iter().take_while(|line| *line != "body:").cloned().collect() };
    let from_run = plan_of(&place, "f-rel", &["--relates", "s2-a.1", "--relates", "s2-b.2"]);
    let tail = [
        "arg: --type=task",
        "arg: --labels=intake:memo",
        "arg: --parent=s2-epic",
        "arg: --deps=relates-to:s2-a.1",
        "arg: --deps=relates-to:s2-b.2",
    ];
    assert_eq!(head(&from_run).get(1..).unwrap_or_default(), tail, "便の終端の plan の引数");
    let shot = shoot(&place, &["memo", "--from", "user", "--relates", "s2-a.1", "--parent", "s2-epic", "--relates", "s2-b.2"]);
    let from_user: Vec<String> = shot.out.lines().map(str::to_owned).collect();
    assert_eq!(head(&from_user).get(1..).unwrap_or_default(), tail, "user の要望の plan の引数（flag の順に依らない）");
    let other = shoot(&place, &["memo", "--from", "user", "--parent", "s2-other"]);
    assert!(other.out.lines().any(|line| line == "arg: --parent=s2-other"), "parent は引数の値: {}", other.out);
    assert!(!other.out.contains("--deps="), "関連 bead の無い周は edge 0: {}", other.out);
    let bare = shoot(&place, &["memo", "--from", "user"]);
    assert_eq!((bare.rc, bare.out.as_str()), (Some(1), ""), "parent の無い周は plan を出さない");
    fs::remove_dir_all(&place.dir).ok();
}

/// (5) 出力は標準出力だけで、偽の `bd` は 1 回も呼ばれない（[`shoot`] が毎回測る）。対照: 同じ PATH の `bd` は
/// 撃てば記録を残す（偽の client が生きている＝不在の assert は空虚でない）。
#[test]
fn ledger_memo_plan_never_calls_the_ledger_client() {
    let place = place();
    stage(&place, "f-nobd", Stage::Failed, Some("main-red"), "2026-09-20T00:06:00Z");
    let state = place.state.display().to_string();
    let shots = [
        shoot(&place, &["memo", "--run", "f-nobd", "--state-dir", &state, "--parent", "s2-epic", "--relates", "s2-a.1"]),
        shoot(&place, &["memo", "--from", "user", "--parent", "s2-epic"]),
        shoot(&place, &["memo", "--run", "nope", "--state-dir", &state, "--parent", "s2-epic"]),
    ];
    assert_eq!(shots.iter().map(|shot| shot.rc).collect::<Vec<_>>(), [Some(0), Some(0), Some(1)], "3 周の rc");
    let entries: Vec<PathBuf> = fs::read_dir(&place.dir)
        .map(|found| found.filter_map(|entry| entry.ok().map(|item| item.path())).collect())
        .unwrap_or_default();
    assert_eq!(entries.len(), 2, "置き場に file を足さない（state と bin だけ）: {entries:?}");
    let path = format!("{}:{}", place.bin.display(), std::env::var("PATH").unwrap_or_default());
    let control = Command::new("sh").args(["-c", "bd create x"]).env("PATH", path).status();
    assert!(control.is_ok_and(|status| status.success()), "対照の起動");
    assert_eq!(fs::read_to_string(&place.record).unwrap_or_default(), "create x\n", "偽の client は撃てば記録を残す");
    fs::remove_dir_all(&place.dir).ok();
}

/// 撃った結果を snapshot の 1 段にする（args・rc・stdout・stderr）。
fn form_of(place: &Place, args: &[&str]) -> String {
    let shot = shoot(place, args);
    let cmd = args.iter().fold("$ ledger".to_owned(), |line, arg| format!("{line} {arg}"));
    format!("{cmd}\nrc={}\nstdout={}\nstderr={}", shot.rc.unwrap_or(-1), shot.out.trim_end(), shot.err.trim_end())
}

/// (6) usage の 1 枚の外形 snapshot（verb 無し・未知の verb・mode 無し・parent 無し・2 形の混在・未知の出所・空白を含む id）。
#[test]
fn ledger_memo_plan_usage_external_form() {
    let place = place();
    let forms: Vec<String> = [
        &[][..],
        &["search"][..],
        &["memo", "--parent", "s2-epic"][..],
        &["memo", "--run", "r", "--state-dir", "s"][..],
        &["memo", "--from", "user", "--run", "r", "--parent", "s2-epic"][..],
        &["memo", "--from", "bot", "--parent", "s2-epic"][..],
        &["memo", "--from", "user", "--parent", "s2 epic"][..],
    ]
    .iter()
    .map(|args| form_of(&place, args))
    .collect();
    insta::assert_snapshot!(forms.join("\n"));
    fs::remove_dir_all(&place.dir).ok();
}
