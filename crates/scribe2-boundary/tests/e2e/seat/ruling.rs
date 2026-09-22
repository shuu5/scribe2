//! run 無しの裁定の歯（設計 docs/design/fleet-event-log.md §9・ADR-0037・接頭辞 `fleet_ruling_`）: 対話面の席の口
//! `seat ruling add` / `ls` が `RulingReceived` を逐語付き・run 無しで書き、空の逐語と対話面でない席は typed に断られ、
//! `pipe report` が `rulings=` を数え、doctor が manifest の `user <ts>` の行と同じ分の event を突き合わせる。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。

use super::*;
use vessel::cli_outcome::RC_BROKEN;
use vessel::fleet::{EventKind, ACTOR_HUMAN};

/// 対話面の席の target（登録 row を持つ）。
const DIALOGUE: &str = "ruling:dialogue";

/// 改行・`"`・前後の空白を含む逐語（要約せずそのまま detail に載ることを測る）。
const WORDS: &str = "  推奨で進めて \"A\" を採る\n2 行目も逐語  ";

/// 登録 row を 1 件持つ置き場（target [`DIALOGUE`]・役割 orchestrator = 埋め込みの R-C7-1 の値）。
fn dialogue_place() -> RolePlace {
    let place = role_place();
    role_stamp(&place, DIALOGUE, Some("sid-ruling"));
    let out = role_register(&place, DIALOGUE, "orchestrator", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    place
}

/// `seat ruling <args…> --state-dir <place>` を撃つ。
fn ruling(place: &RolePlace, args: &[&str]) -> Output {
    let state = place.state.display().to_string();
    let mut all = vec!["ruling"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--state-dir", &state]);
    run_seat(&all)
}

/// event log の本文（無ければ空）。
fn log_of(place: &RolePlace) -> String {
    fs::read_to_string(vessel::fleet::store::events_path(&place.state)).unwrap_or_default()
}

/// log の裁定の event（物理順）。
fn rulings_of(place: &RolePlace) -> Vec<vessel::fleet::Event> {
    vessel::fleet::store::read_all(&place.state)
        .unwrap_or_default()
        .into_iter()
        .filter(|event| event.kind == EventKind::RulingReceived)
        .collect()
}

/// (1) 対話面の役割の登録 row を持つ target で `seat ruling add` が `RulingReceived` を **1 件**書く: actor = human・`run` を
/// 行に持たない・逐語は detail に**逐語で**（前後の空白と改行と `"` も）・`bead` / `rule` は渡した値。stdout の 1 行の ts が
/// event の ts（裁定 id）で、`seat ruling ls` は 1 件 1 行で ts・bead・rule・逐語を出す。便も席も作らない。
#[test]
fn fleet_ruling_add_writes_one_human_event_without_a_run_and_ls_lists_it() {
    let place = dialogue_place();
    let before = log_of(&place).lines().count();
    let out = ruling(&place, &["add", "--target", DIALOGUE, "--words", WORDS, "--bead", "s2-x.1", "--rule", "R-C9-1"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let found = rulings_of(&place);
    assert_eq!(found.len(), 1, "1 件だけ: {found:?}");
    let event = found.first().cloned().expect("裁定が 1 件在る");
    assert_eq!(event.actor, ACTOR_HUMAN, "人由来");
    assert_eq!(event.detail.as_deref(), Some(WORDS), "逐語をそのまま");
    assert_eq!((event.run.as_str(), event.bead.as_str(), event.rule.as_deref()), ("", "s2-x.1", Some("R-C9-1")));
    assert_eq!((event.stage, event.seat.as_deref(), event.pid), (None, None, None), "段も席も pid も持たない");
    assert_eq!(
        stdout_of(&out),
        format!("seat ruling: recorded ts={} target={DIALOGUE} bead=s2-x.1 rule=R-C9-1\n", event.ts),
        "ts が裁定 id"
    );
    let log = log_of(&place);
    assert_eq!(log.lines().count(), before + 1, "追記は 1 行: {log}");
    let line = log.lines().last().unwrap_or_default();
    assert!(line.contains("\"kind\":\"RulingReceived\"") && !line.contains("\"run\":"), "run の key を書かない: {line}");
    let state = vessel::fleet::replay(&vessel::fleet::store::read_all(&place.state).expect("log を読める"));
    assert!(state.runs.is_empty() && state.seats.is_empty(), "便も席も作らない: {state:?}");

    let listed = ruling(&place, &["ls"]);
    assert_eq!(rc_of(&listed), i32::from(RC_OK), "stderr={}", stderr_of(&listed));
    assert_eq!(
        stdout_of(&listed),
        format!("ruling: ts={} bead=s2-x.1 rule=R-C9-1 words={WORDS:?}\n", event.ts),
        "1 件 1 行（逐語は escape して 1 行に収める）"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (2) 空の逐語（空文字・空白だけ）は `reason=empty-words`、登録 row の無い target は `reason=not-dialogue-surface` で
/// rc 1・stdout 0 byte・log は 1 byte も変わらない。`--words` そのものが無い周は使い方で断る。逐語を先に測る（空の逐語は
/// 対話面でない target でも `empty-words`）。
#[test]
fn fleet_ruling_add_refuses_empty_words_and_non_dialogue_targets_without_writing() {
    let place = dialogue_place();
    let before = log_of(&place);
    for (target, words, reason) in [
        (DIALOGUE, "", "empty-words"),
        (DIALOGUE, "  \n ", "empty-words"),
        ("stranger:w", "", "empty-words"),
        ("stranger:w", "推奨で", "not-dialogue-surface"),
    ] {
        let out = ruling(&place, &["add", "--target", target, "--words", words]);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{target} {words:?}: {}", stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{target} {words:?}: stdout 0 byte");
        assert_eq!(stderr_of(&out), format!("seat ruling: refused reason={reason} target={target}\n"), "{words:?}");
        assert_eq!(log_of(&place), before, "{target} {words:?}: log は不変");
    }
    let missing = ruling(&place, &["add", "--target", DIALOGUE]);
    assert_eq!(rc_of(&missing), i32::from(RC_REFUSED), "--words 無し");
    assert!(stderr_of(&missing).starts_with("usage: seat "), "使い方: {}", stderr_of(&missing));
    assert_eq!(log_of(&place), before, "使い方の周も log は不変");
    // 登録の無い置き場（log も無い）でも断る（row が無い＝FailClosed）。
    let empty = role_place();
    let out = ruling(&empty, &["add", "--target", DIALOGUE, "--words", "推奨で"]);
    assert_eq!(stderr_of(&out), format!("seat ruling: refused reason=not-dialogue-surface target={DIALOGUE}\n"));
    assert!(!vessel::fleet::store::events_path(&empty.state).exists(), "event を 1 件も書かない");
    // 読めない log も断る（読めなさを対話面に倒さない）・`ls` は rc 2 で 0 件に潰さない。
    fs::create_dir_all(empty.state.join("fleet")).expect("dir を作れる");
    fs::write(vessel::fleet::store::events_path(&empty.state), "こわれ\n").expect("壊れた行を書ける");
    let broken = ruling(&empty, &["add", "--target", DIALOGUE, "--words", "推奨で"]);
    assert_eq!(rc_of(&broken), i32::from(RC_REFUSED), "{}", stderr_of(&broken));
    assert!(stderr_of(&broken).contains("reason=not-dialogue-surface"), "{}", stderr_of(&broken));
    let listed = ruling(&empty, &["ls"]);
    assert_eq!(rc_of(&listed), i32::from(RC_BROKEN), "読めない log の ls は rc 2");
    assert!(stdout_of(&listed).is_empty(), "0 件を名乗らない");
    fs::remove_dir_all(&place.dir).ok();
    fs::remove_dir_all(&empty.dir).ok();
}

/// (3) `pipe report` は `rulings=<n>` を `human_events_other_than_approval=` の直後に出し、裁定は承認の kind として数える＝
/// `human_events` は増えるが `human_events_other_than_approval` は 0 のまま。裁定の無い置き場の行は従来の字面（token を出さない）。
#[test]
fn fleet_ruling_report_counts_rulings_and_keeps_other_human_events_at_zero() {
    let place = dialogue_place();
    let report = |place: &RolePlace| {
        let out = Command::new(bin()).args(["pipe", "report", "--state-dir"]).arg(&place.state).output().expect("binary を起動できる");
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
        stdout_of(&out).lines().next().unwrap_or_default().to_owned()
    };
    let before = report(&place);
    assert!(before.starts_with("runs=0 landed=0 human_events=0 human_events_other_than_approval=0 review_fail=0 "), "{before}");
    assert!(!before.contains("rulings="), "裁定の無い置き場は token を出さない: {before}");
    for words in ["一つ目", "二つ目"] {
        let out = ruling(&place, &["add", "--target", DIALOGUE, "--words", words]);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    }
    let after = report(&place);
    assert!(
        after.starts_with("runs=0 landed=0 human_events=2 human_events_other_than_approval=0 rulings=2 review_fail=0 "),
        "裁定は承認の kind（approval 以外の人由来は増えない）: {after}"
    );
    fs::remove_dir_all(&place.dir).ok();
}

/// (4) doctor の 1 行: `--rules` の manifest の `ruling` が `user <分>` の行を母集団にし、同じ分の裁定が在る行を matched・
/// 無い行を id で名指す（分の曖昧な行は skipped・`user ` で始まらない行は数えない）。裁定も母集団も無い周は行を出さない
/// （doctor の外形を変えない）。行は登録 row の行の前に並び、rc は 0 のまま（判定しない）。
#[test]
fn fleet_ruling_doctor_matches_user_ts_rows_by_the_same_minute() {
    let place = dialogue_place();
    let bare = doctor_rows(&place, NO_ACCOUNT_RULES);
    assert!(!bare.iter().any(|line| line.starts_with("rulings=")), "数えるものが無い周は行を出さない: {bare:?}");

    let out = ruling(&place, &["add", "--target", DIALOGUE, "--words", "推奨で", "--rule", "ruled.hit"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let ts = rulings_of(&place).first().map(|event| event.ts.clone()).unwrap_or_default();
    let minute = ts.get(..16).unwrap_or_default().to_owned();
    assert_eq!(minute.len(), 16, "分まで取れる: {ts}");
    // 裁定の分と必ず違う分（器の ts は「いま」なので過去の固定の分と一致しない）。
    let other = "2001-01-01T00:00";
    let row = |id: &str, ruling: &str| {
        format!("\n[[rule]]\nid = \"{id}\"\nkind = \"CoreLines\"\nvalue = 1\nenabled = true\nruling = \"{ruling}\"\nruled_at = \"d\"\n")
    };
    let rules = [
        NO_ACCOUNT_RULES.to_owned(),
        row("ruled.hit", &format!("user {minute}Z")),
        row("ruled.miss", &format!("user {other}Z by the other minute")),
        row("ruled.vague", "user 2026-09-15T11:2xZ"),
        row("ruled.outside", "grill U3"),
    ]
    .concat();
    let lines = doctor_rows(&place, &rules);
    let line = lines.iter().find(|line| line.starts_with("rulings=")).cloned().unwrap_or_default();
    assert_eq!(line, "rulings=1 rule-rulings=1/2 unmatched=ruled.miss skipped=1", "{lines:?}");
    let at = lines.iter().position(|found| *found == line);
    let first_seat = lines.iter().position(|found| found.starts_with("seat: "));
    assert_eq!(at.map(|found| found + 1), first_seat, "登録 row の行の直前: {lines:?}");
    assert_eq!(lines.len(), bare.len() + 1, "足すのは 1 行だけ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}
