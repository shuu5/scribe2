//! 裁定の歯（設計 docs/design/fleet-event-log.md §9 / §14・ADR-0037・ADR-0087・ADR-0089）: 器の結びの口 `seat ruling bind`
//! （接頭辞 `seat_ruling_bind_`）が、記帳された発話と開いた台帳の問いを結び、notes → close → 裁定 event の順に書き、閉じた 4 語で
//! 断り、半端な結びを撃ち直しで仕上げる。`pipe report` が `rulings=` を数え、doctor が manifest の `user <ts>` の行と同じ分の
//! event を突き合わせる（接頭辞 `fleet_ruling_`）。逐語を受ける `seat ruling add` は無い。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。台帳は偽の bd（sh の script）で、
//! 撃たれた引数を `calls.log` に 1 撃ち 1 行（tab 区切り）で残す。

use super::*;
use vessel::cli_outcome::RC_BROKEN;
use vessel::fleet::json_lite;
use vessel::fleet::store::{self, LockPolicy};
use vessel::fleet::{Case, Channel, Event, EventKind, ACTOR_HUMAN, SCHEMA};

/// 対話面の席の target（登録 row を持つ）。
const DIALOGUE: &str = "ruling:dialogue";

/// 改行・`"`・非 ASCII を含む逐語。cmd の引数と stdout の字に無い字（採・Ω・二）で作る。
const WORDS: &str = "  推奨で進めて \"Ω\" を採る\n二行目も逐語  ";

/// 逐語にだけ在る字（stdout と stderr に出ないことを測る）。
const WORDS_MARKS: [char; 3] = ['採', 'Ω', '二'];

/// 記帳された発話の ts と、記帳されていない ts。
const TS_A: &str = "2026-09-30T07:05:09.123Z";
const TS_MISSING: &str = "2026-09-30T09:00:00.000Z";

/// 偽の bd（sh）。撃たれた引数を tab 区切りで `calls.log` に足し、`--readonly show <id> --json` は `show-<id>.json` を返す
/// （無ければ rc 1）・`update <id> --append-notes <行>` は `notes.txt` に行を足し・`close` は `close.fail` が在れば 1 回だけ rc 1・
/// `close.swap` が在れば書く前に中の path を dir に替える。update と close の撃ちの時点の裁定 event の件数を `*.rulings` に残す。
const FAKE_BD: &str = "#!/bin/sh
d=$(dirname \"$0\")
first=$1; second=$2; third=$3; fourth=$4
line=$1; shift
for a in \"$@\"; do line=\"$line\t$a\"; done
printf '%s\\n' \"$line\" >> \"$d/calls.log\"
case \"$first\" in
--readonly)
  [ -f \"$d/show-$third.json\" ] || exit 1
  cat \"$d/show-$third.json\" ;;
update)
  grep -c RulingReceived \"$(cat \"$d/events.path\")\" > \"$d/update.rulings\"
  printf '%s\\n' \"$fourth\" >> \"$d/notes.txt\" ;;
close)
  grep -c RulingReceived \"$(cat \"$d/events.path\")\" > \"$d/close.rulings\"
  if [ -f \"$d/close.fail\" ]; then rm -f \"$d/close.fail\"; echo 'close refused' >&2; exit 1; fi
  if [ -f \"$d/close.swap\" ]; then p=$(cat \"$d/close.swap\"); rm -f \"$p\"; mkdir \"$p\"; fi ;;
esac
exit 0
";

/// 偽の bd と event log を持つ置き場。
struct Fake {
    dir: TmpDir,
    state: PathBuf,
    repo: PathBuf,
    bd: String,
}

impl Fake {
    /// 偽の bd を置いた置き場を作る。
    fn new() -> Self {
        let dir = tmp();
        let state = dir.join("state");
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).ok();
        let bd = fixture(&dir, "bd", FAKE_BD);
        assert!(fs::set_permissions(&bd, fs::Permissions::from_mode(0o755)).is_ok(), "実行権を付ける");
        fixture(&dir, "events.path", &store::events_path(&state).display().to_string());
        Self { dir, state, repo, bd }
    }

    /// event を log へ足す（器の書き手と同じ 1 本）。
    fn put(&self, event: &Event) {
        let policy = LockPolicy::embedded();
        assert!(policy.is_ok(), "lock の値を読める");
        if let Ok(policy) = policy {
            assert!(store::append(&self.state, event, policy).is_ok(), "event を足せる");
        }
    }

    /// 発話 event を 1 件足す（chat は session を持つ・gui は持たない）。
    fn say(&self, ts: &str, channel: Channel, words: &str) {
        let session = (channel == Channel::Chat).then(|| "sid-chat".to_owned());
        self.put(&event(EventKind::UtteranceReceived, ts, "", Some(words), Some(Case::Utterance { channel, session })));
    }

    /// 同じ組（発話 ts・問い）を結んだ裁定 event を足す（結び済みの fixture）。
    fn tie(&self, question: &str, utterance: &str) {
        self.put(&ruling_event(TS_MISSING, question, utterance));
    }

    /// 問い 1 本の show の JSON（状態）を置く。
    fn show(&self, question: &str, json: &str) {
        fixture(&self.dir, &format!("show-{question}.json"), json);
    }

    /// event log の本文（無ければ空）。
    fn log(&self) -> String {
        fs::read_to_string(store::events_path(&self.state)).unwrap_or_default()
    }

    /// 偽の bd への撃ち（tab で割った引数の列・撃った順）。
    fn calls(&self) -> Vec<Vec<String>> {
        let text = fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default();
        text.lines().map(|line| line.split('\t').map(str::to_owned).collect()).collect()
    }

    /// 台帳への書き（show でない撃ち）。
    fn writes(&self) -> Vec<Vec<String>> {
        self.calls().into_iter().filter(|call| call.first().is_none_or(|word| word != "--readonly")).collect()
    }

    /// 偽の bd が読んだ回数（show の撃ち）。
    fn reads(&self) -> usize {
        self.calls().len().saturating_sub(self.writes().len())
    }

    /// 偽の bd が update で受けた notes（1 撃ち 1 行・bd の notes と同じに足される）。
    fn notes(&self) -> String {
        fs::read_to_string(self.dir.join("notes.txt")).unwrap_or_default()
    }

    /// 偽の bd の記録を空にする（次の撃ちの前）。
    fn forget(&self) {
        fs::remove_file(self.dir.join("calls.log")).ok();
    }

    /// 偽の bd の書きの時点の裁定 event の件数（`update.rulings` / `close.rulings`）。
    fn seen(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(name)).unwrap_or_default().trim().to_owned()
    }

    /// `seat ruling bind` を撃つ。
    fn bind(&self, question: &str, utterance: &str) -> Output {
        let (repo, state) = (self.repo.display().to_string(), self.state.display().to_string());
        run_seat(&["ruling", "bind", "--repo", &repo, "--state-dir", &state, "--question", question, "--utterance", utterance, "--bd", &self.bd])
    }

    /// log の裁定の event（物理順）。
    fn rulings(&self) -> Vec<Event> {
        rulings_in(&self.state)
    }
}

/// event 1 件（共通の欄を固定する・kind と ts と本体だけ選ぶ）。
fn event(kind: EventKind, ts: &str, bead: &str, detail: Option<&str>, case: Option<Case>) -> Event {
    Event {
        schema: SCHEMA,
        ts: ts.to_owned(),
        kind,
        run: String::new(),
        bead: bead.to_owned(),
        host: "h".to_owned(),
        actor: ACTOR_HUMAN.to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: detail.map(str::to_owned),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case,
    }
}

/// 結びの形の裁定 event（fixture・問いと発話の組だけを持つ）。
fn ruling_event(ts: &str, question: &str, utterance: &str) -> Event {
    let case = Case::Ruling {
        ruling: format!("{question}:20260930T0705Z-1"),
        utterance: utterance.to_owned(),
        channel: Channel::Chat,
        question_ts: "2026-09-30T06:00:00Z".to_owned(),
        asked: None,
    };
    event(EventKind::RulingReceived, ts, question, Some("推奨で"), Some(case))
}

/// 問い 1 本の show の JSON（bd の配列 1 要素）。`notes` は行の列を改行で繋いだ字。
fn show_json(status: &str, labels: &[&str], created_at: &str, asked: Option<&str>, notes: &str) -> String {
    let labels: Vec<String> = labels.iter().map(|label| json_lite::quote(label)).collect();
    let metadata = asked.map_or_else(|| "{}".to_owned(), |found| format!("{{\"asked\":{}}}", json_lite::quote(found)));
    format!(
        "[{{\"id\":\"x\",\"status\":{},\"labels\":[{}],\"created_at\":{},\"notes\":{},\"metadata\":{metadata}}}]",
        json_lite::quote(status),
        labels.join(","),
        json_lite::quote(created_at),
        json_lite::quote(notes)
    )
}

/// 開いた問い（label intake:question）の show。
fn open_question(created_at: &str, asked: Option<&str>) -> String {
    show_json("open", &["intake:question"], created_at, asked, "")
}

/// 裁定の行の期待（設計 §14 約束 5・字面は契約から組む）。
fn row_of(question: &str, ts: &str, channel: &str) -> String {
    format!("{question}:20260930T0705Z-1 | {question} | {ts} | {channel} | {}", json_lite::quote(WORDS))
}

/// 行の最後の欄（JSON の文字列の字面）を戻した字。
fn unquote(text: &str) -> String {
    let pairs = json_lite::parse_object(&format!("{{\"w\":{text}}}")).unwrap_or_default();
    pairs.first().and_then(|(_, value)| value.as_str()).unwrap_or_default().to_owned()
}

/// 置き場の裁定の event（物理順）。
fn rulings_in(state: &Path) -> Vec<Event> {
    store::read_all(state).unwrap_or_default().into_iter().filter(|found| found.kind == EventKind::RulingReceived).collect()
}

/// 断りの 1 行（設計 §14 約束 2）。
fn refused(reason: &str, question: &str, utterance: &str) -> String {
    format!("seat ruling: refused reason={reason} question={question} utterance={utterance}\n")
}

/// 結べた周の stdout の 1 行（設計 §14 約束 9）。
fn bound_line(question: &str, ts: &str, channel: &str) -> String {
    format!("ruling: id={question}:20260930T0705Z-1 question={question} utterance={ts} channel={channel}\n")
}

/// 置き場の file の一覧（相対 path・整列）。
fn tree_of(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            out.push(path.strip_prefix(root).map(|found| found.display().to_string()).unwrap_or_default());
            if path.is_dir() {
                walk(&path, root, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// 登録 row を 1 件持つ置き場（target [`DIALOGUE`]・役割 orchestrator）。
fn dialogue_place() -> RolePlace {
    let place = role_place();
    role_stamp(&place, DIALOGUE, Some("sid-ruling"));
    let out = role_register(&place, DIALOGUE, "orchestrator", &["--anchor", "/repo"]);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    place
}

/// `seat ruling <args…> --state-dir <state>` を撃つ。
fn ruling(state: &Path, args: &[&str]) -> Output {
    let state = state.display().to_string();
    let mut all = vec!["ruling"];
    all.extend_from_slice(args);
    all.extend_from_slice(&["--state-dir", &state]);
    run_seat(&all)
}

/// (a) 先に在った問い（metadata に asked=seat・発話の gui）と、発話の後に起こした問い（asked 無し・発話の chat）の 2 形で 1 回ずつ通る。
/// notes の行が 1 行（5 欄・経路は発話 event の channel・逐語は最後の欄を JSON の文字列として戻すと発話の detail と 1 byte も
/// 違わない）・close の理由が `裁定 <id>`・裁定 event が 1 件（asked のある形は 5 key・無い形は 4 key）・id は
/// `<問い id>:<発話の YYYYMMDDTHHMMZ>-1`・notes → close → event の順・rc 0・stdout は 1 行で逐語の字を含まず stderr は 0 byte。
#[test]
fn seat_ruling_bind_binds_an_utterance_to_an_open_question_in_two_forms() {
    let forms = [
        ("s2-q1", "2026-09-30T06:00:00Z", Some("seat"), Channel::Gui, TS_A, "gui"),
        ("s2-q2", "2026-09-30T08:00:00Z", None, Channel::Chat, TS_A, "chat"),
    ];
    for (question, created_at, asked, channel, ts, word) in forms {
        let fake = Fake::new();
        fake.say(ts, channel, WORDS);
        fake.show(question, &open_question(created_at, asked));
        let out = fake.bind(question, ts);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{question}: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), bound_line(question, ts, word), "{question}: 1 行だけ");
        assert!(stderr_of(&out).is_empty(), "{question}: stderr 0 byte");
        assert!(!stdout_of(&out).contains(WORDS_MARKS), "{question}: 逐語を載せない");
        assert_notes_then_close(&fake, question, ts, word);
        assert_event_after_close(&fake, question, (created_at, asked), (channel, ts));
    }
}

/// 偽の bd への書きが notes（5 欄・最後の欄が逐語）→ close（理由 `裁定 <id>`）の順に 1 回ずつで、event はどちらの撃ちの時点でも 0 件。
fn assert_notes_then_close(fake: &Fake, question: &str, ts: &str, word: &str) {
    let row = row_of(question, ts, word);
    let expected = [
        vec!["update".to_owned(), question.to_owned(), "--append-notes".to_owned(), row.clone()],
        vec!["close".to_owned(), question.to_owned(), "--reason".to_owned(), format!("裁定 {question}:20260930T0705Z-1")],
    ];
    assert_eq!(fake.writes(), expected, "{question}: notes → close の順に 1 回ずつ");
    assert_eq!(fake.reads(), 1, "{question}: 台帳の読みは 1 回");
    let fields: Vec<&str> = row.splitn(5, " | ").collect();
    assert_eq!(fields.len(), 5, "{question}: 5 欄");
    assert_eq!(unquote(fields.last().copied().unwrap_or_default()), WORDS, "{question}: 最後の欄が逐語");
    assert_eq!(fake.notes().lines().count(), 1, "{question}: notes の行は 1 行（改行を含む逐語でも）");
    assert_eq!((fake.seen("update.rulings"), fake.seen("close.rulings")), ("0".to_owned(), "0".to_owned()), "{question}: event は close の後");
}

/// 裁定 event が 1 件で、actor human・run 無し・bead は問い id・detail は発話の逐語・rule 無し・本体は結びの 5 key（asked は metadata に
/// 在る周だけ）。`origin` は問いの（起票の時刻・asked）、`said` は発話の（経路・ts）。
fn assert_event_after_close(fake: &Fake, question: &str, origin: (&str, Option<&str>), said: (Channel, &str)) {
    let ((created_at, asked), (channel, ts)) = (origin, said);
    let found = fake.rulings();
    assert_eq!(found.len(), 1, "{question}: 裁定 event が 1 件: {found:?}");
    let event = found.first().cloned().unwrap_or_else(event_none);
    assert_eq!((event.actor.as_str(), event.run.as_str(), event.bead.as_str()), (ACTOR_HUMAN, "", question), "{question}");
    assert_eq!(event.detail.as_deref(), Some(WORDS), "{question}: detail は発話の逐語");
    assert_eq!(event.rule, None, "{question}: rule を持たない");
    let case = Case::Ruling {
        ruling: format!("{question}:20260930T0705Z-1"),
        utterance: ts.to_owned(),
        channel,
        question_ts: created_at.to_owned(),
        asked: asked.map(str::to_owned),
    };
    assert_eq!(event.case, Some(case), "{question}: 本体");
    let line = fake.log().lines().last().unwrap_or_default().to_owned();
    let keys: Vec<String> = json_lite::parse_object(&line).unwrap_or_default().into_iter().map(|(key, _)| key).collect();
    let has = |name: &str| keys.iter().any(|key| key == name);
    let held = ["ruling", "utterance", "channel", "question_ts", "asked"].iter().filter(|name| has(name)).count();
    assert_eq!(held, if asked.is_some() { 5 } else { 4 }, "{question}: 5 key（asked は metadata に在る周だけ）: {line}");
    assert_eq!(has("asked"), asked.is_some(), "{question}: asked の key: {line}");
    assert!(!(has("run") || has("rule") || has("session")), "{question}: run・rule・session の key を持たない: {line}");
}

/// 読めなかった fixture の代わり（`unwrap_or_else` の腕・到達すれば直前の件数の assert が先に落ちている）。
fn event_none() -> Event {
    event(EventKind::RulingReceived, "", "", None, None)
}

/// (b) 断り 4 形（無い ts・結び済み・閉じた問い・問いでない bead）がどれも rc 1・stdout 0 byte・stderr が 1 行（語は形の順に
/// no-utterance・bound・closed・not-question）で、event log も偽の bd の書きも変えない。
#[test]
fn seat_ruling_bind_refuses_the_four_forms_without_writing() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-a", &open_question("2026-09-30T06:00:00Z", None));
    fake.show("s2-b", &open_question("2026-09-30T06:00:00Z", None));
    fake.tie("s2-b", TS_A);
    fake.show("s2-c", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-d", &show_json("open", &[], "2026-09-30T06:00:00Z", None, ""));
    let cases = [("s2-a", TS_MISSING, "no-utterance"), ("s2-b", TS_A, "bound"), ("s2-c", TS_A, "closed"), ("s2-d", TS_A, "not-question")];
    for (question, ts, reason) in cases {
        let before = fake.log();
        let out = fake.bind(question, ts);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{reason}: {}", stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{reason}: stdout 0 byte");
        assert_eq!(stderr_of(&out), refused(reason, question, ts), "{reason}");
        assert_eq!(fake.log(), before, "{reason}: event log は不変");
        assert!(fake.writes().is_empty(), "{reason}: 偽の bd の書きは 0 回: {:?}", fake.writes());
    }
    // bead が無い（show が要素 0 件の配列）周も問いでない。
    fake.show("s2-gone", "[]");
    let out = fake.bind("s2-gone", TS_A);
    assert_eq!(stderr_of(&out), refused("not-question", "s2-gone", TS_A), "無い bead");
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED));
    assert!(fake.writes().is_empty(), "書かない");
}

/// (b2) 断りの順: 隣り合う語の対を全部持つ 3 形で順を 1 列に決める。無い ts ∧ 結び済み・結び済み ∧ 閉じた問い・閉じた問い ∧ 問いでない bead で、
/// stderr の語はそれぞれ先の語（no-utterance・bound・closed）だけ。加えて無い ts ∧ 問いでない bead で no-utterance。
#[test]
fn seat_ruling_bind_refusal_order_is_one_line_across_adjacent_pairs() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    // 無い ts ∧ 結び済み: 同じ組の裁定 event だけを直書きし、その ts の発話は置かない。
    fake.tie("s2-a", TS_MISSING);
    fake.show("s2-a", &open_question("2026-09-30T06:00:00Z", None));
    // 結び済み ∧ 閉じた問い: 結んだ後の問い（closed）。
    fake.tie("s2-b", TS_A);
    fake.show("s2-b", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, &row_of("s2-b", TS_A, "chat")));
    // 閉じた問い ∧ 問いでない bead: label の無い閉じた bead。
    fake.show("s2-c", &show_json("closed", &[], "2026-09-30T06:00:00Z", None, ""));
    // 無い ts ∧ 問いでない bead。
    fake.show("s2-d", &show_json("open", &[], "2026-09-30T06:00:00Z", None, ""));
    let cases = [("s2-a", TS_MISSING, "no-utterance"), ("s2-b", TS_A, "bound"), ("s2-c", TS_A, "closed"), ("s2-d", TS_MISSING, "no-utterance")];
    for (question, ts, reason) in cases {
        let out = fake.bind(question, ts);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{question}: {}", stderr_of(&out));
        assert_eq!(stderr_of(&out), refused(reason, question, ts), "{question}: 先の語だけ");
    }
    assert!(fake.writes().is_empty(), "どの周も書かない");
}

/// (c) 1 つの発話を 2 つの問いへ結べる（同じ発話と同じ問いの組だけが `bound`）: 2 件の裁定 event が別の id で残り、どちらの問いも閉じる。
#[test]
fn seat_ruling_bind_binds_one_utterance_to_two_questions() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    for question in ["s2-q1", "s2-q2"] {
        fake.show(question, &open_question("2026-09-30T06:00:00Z", None));
        let out = fake.bind(question, TS_A);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{question}: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), bound_line(question, TS_A, "chat"), "{question}");
    }
    let ids: Vec<String> = fake
        .rulings()
        .iter()
        .filter_map(|found| match &found.case {
            Some(Case::Ruling { ruling, .. }) => Some(ruling.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["s2-q1:20260930T0705Z-1", "s2-q2:20260930T0705Z-1"], "問いごとに別の id");
    let closes: Vec<String> = fake.writes().iter().filter(|call| call.first().is_some_and(|word| word == "close")).filter_map(|call| call.get(1).cloned()).collect();
    assert_eq!(closes, ["s2-q1", "s2-q2"], "2 つとも閉じる");
    let again = fake.bind("s2-q1", TS_A);
    assert_eq!(stderr_of(&again), refused("bound", "s2-q1", TS_A), "同じ組の 2 度目は bound");
}

/// (d) 半端な結び（notes と close だけ済み・event が無い）の撃ち直しが event だけを足す: 偽の bd の書きは 0 回（同じ行を重ねない）・
/// rc 0・stdout は結べた 1 行・裁定 event が 1 件。3 度目は結び済みで断る。
#[test]
fn seat_ruling_bind_finishes_a_half_bound_question_with_the_event_only() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    let notes = format!("前の行\n{}\n", row_of("s2-q1", TS_A, "chat"));
    fake.show("s2-q1", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", Some("user"), &notes));
    let out = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), bound_line("s2-q1", TS_A, "chat"));
    assert!(fake.writes().is_empty(), "append-notes と close は 0 回: {:?}", fake.writes());
    assert_eq!(fake.rulings().len(), 1, "event は 1 件");
    assert!(fake.notes().is_empty(), "notes に足さない");
    let again = fake.bind("s2-q1", TS_A);
    assert_eq!(stderr_of(&again), refused("bound", "s2-q1", TS_A), "仕上げた後は結び済み");
    assert_eq!(fake.rulings().len(), 1, "event は重ならない");
}

/// (d2) close の前で落ちた結び: 偽の bd の close が 1 回目だけ rc 1 を返す周は rc 1 で `partial`・notes の行が 1 行・event 0 件・問いは
/// open。同じ組の撃ち直しは rc 0 で、append-notes は 0 回・close は 1 回・event は 1 件、notes の裁定の行は 1 行のまま。
#[test]
fn seat_ruling_bind_finishes_a_question_whose_close_failed_without_a_second_notes_row() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", None));
    fixture(&fake.dir, "close.fail", "");
    let out = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "stdout 0 byte");
    assert_eq!(stderr_of(&out), "seat ruling: partial stage=close id=s2-q1:20260930T0705Z-1 question=s2-q1 utterance=2026-09-30T07:05:09.123Z\n");
    assert!(!stderr_of(&out).contains(WORDS_MARKS), "stderr にも逐語を載せない");
    assert_eq!(fake.notes().lines().count(), 1, "notes の行は 1 行");
    assert_eq!(fake.rulings().len(), 0, "event は 0 件");
    assert_eq!(fake.writes().len(), 2, "notes と close の 2 撃ち");
    // bd は notes を持ったまま open の問いを返す。
    fake.show("s2-q1", &show_json("open", &["intake:question"], "2026-09-30T06:00:00Z", None, &fake.notes()));
    fake.forget();
    let again = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&again), i32::from(RC_OK), "stderr={}", stderr_of(&again));
    assert_eq!(stdout_of(&again), bound_line("s2-q1", TS_A, "chat"));
    let writes = fake.writes();
    let verbs: Vec<&str> = writes.iter().filter_map(|call| call.first().map(String::as_str)).collect();
    assert_eq!(verbs, ["close"], "append-notes は 0 回・close は 1 回: {writes:?}");
    assert_eq!(fake.rulings().len(), 1, "event は 1 件");
    assert_eq!(fake.notes().lines().count(), 1, "notes の裁定の行は 1 行のまま");
}

/// (e) 使い方の行と `scribe2 help seat` の頁（FORM の行と SUBCOMMANDS）の両方に `ruling bind` が在り、`ruling add` が無い。
#[test]
fn seat_ruling_bind_is_in_the_usage_and_the_help_page_and_add_is_not() {
    let usage = stderr_of(&run_seat(&[]));
    assert!(usage.contains("|ruling bind --repo R --state-dir S --question ID --utterance TS [--bd B]|"), "使い方: {usage}");
    assert!(!usage.contains("ruling add"), "使い方に add は無い: {usage}");
    let page = Command::new(bin()).args(["help", "seat"]).output().map(|out| stdout_of(&out)).unwrap_or_default();
    assert!(page.contains("|ruling bind --repo R --state-dir S --question ID --utterance TS [--bd B]|"), "頁の FORM: {page}");
    assert!(page.lines().any(|line| line.trim_start().starts_with("ruling bind ")), "頁の SUBCOMMANDS: {page}");
    assert!(!page.contains("ruling add"), "頁に add は無い: {page}");
}

/// (e2) 消えた口: 対話面の役割の登録 row を持つ置き場で、今までの `add` の全引数の形を撃つと rc 2（使い方の誤り）・stdout 0 byte・
/// stderr の使い方の行が `ruling bind` を持ち `ruling add` を持たず、event log に `RulingReceived` が 0 件・置き場の file の一覧が
/// 撃つ前と同じ。引数の無い `ruling add` は rc 1 の使い方。
#[test]
fn seat_ruling_bind_the_removed_add_mouth_is_a_usage_error_and_writes_nothing() {
    let place = dialogue_place();
    let before = tree_of(&place.state);
    let state = place.state.display().to_string();
    let out = run_seat(&["ruling", "add", "--state-dir", &state, "--target", DIALOGUE, "--words", "推奨で", "--bead", "s2-x.1", "--rule", "R-C9-1"]);
    assert_eq!(rc_of(&out), 2, "使い方の誤り: {}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "stdout 0 byte");
    let usage = vessel::seat::cli::usage();
    assert_eq!(stderr_of(&out).lines().last(), Some(usage.as_str()), "使い方の行で終わる: {}", stderr_of(&out));
    assert!(usage.contains("ruling bind") && !stderr_of(&out).contains("ruling add"), "{}", stderr_of(&out));
    assert!(rulings_in(&place.state).is_empty(), "裁定 event は 0 件");
    assert_eq!(tree_of(&place.state), before, "置き場は不変");
    let bare = ruling(&place.state, &["add"]);
    assert_eq!(rc_of(&bare), i32::from(RC_REFUSED), "引数の無い add は使い方");
    assert_eq!(stderr_of(&bare), format!("{usage}\n"));
    assert_eq!(tree_of(&place.state), before, "置き場は不変");
    fs::remove_dir_all(&place.dir).ok();
}

/// (f) `seat ruling ls` は新しい形の行で `rule=` の代わりに `ruling=<id>` を出す（古い `rule` だけの行は今までの字面のまま）。
#[test]
fn seat_ruling_bind_ls_prints_the_ruling_column() {
    let fake = Fake::new();
    let old = Event { rule: Some("R-C9-1".to_owned()), ..event(EventKind::RulingReceived, "2026-09-22T01:02:03Z", "s2-x.1", Some("古い逐語"), None) };
    fake.put(&old);
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", Some("user")));
    assert_eq!(rc_of(&fake.bind("s2-q1", TS_A)), i32::from(RC_OK));
    let ts = fake.rulings().last().map(|found| found.ts.clone()).unwrap_or_default();
    let listed = ruling(&fake.state, &["ls"]);
    assert_eq!(rc_of(&listed), i32::from(RC_OK), "stderr={}", stderr_of(&listed));
    let expected = format!(
        "ruling: ts=2026-09-22T01:02:03Z bead=s2-x.1 rule=R-C9-1 words=\"古い逐語\"\nruling: ts={ts} bead=s2-q1 ruling=s2-q1:20260930T0705Z-1 words={WORDS:?}\n"
    );
    assert_eq!(stdout_of(&listed), expected, "1 件 1 行");
    let broken = tmp();
    fs::create_dir_all(broken.join("fleet")).ok();
    fs::write(store::events_path(&broken), "こわれ\n").ok();
    let out = ruling(&broken, &["ls"]);
    assert_eq!(rc_of(&out), i32::from(RC_BROKEN), "読めない log の ls は rc 2");
    assert!(stdout_of(&out).is_empty(), "0 件を名乗らない");
}

/// (g) 偽の bd の show が読めない JSON を返す周・show が rc 1 の周は rc 1 で `reason=ledger-unreadable`・event log も偽の bd の書きも変えない。
/// 同じ偽の bd で、無い ts の周と結び済みの組の周は no-utterance と bound で断り、偽の bd の show の呼び出しは 0 回（台帳は結び済みの後に読む）。
#[test]
fn seat_ruling_bind_ledger_unreadable_writes_nothing_and_is_read_after_the_first_two_refusals() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.tie("s2-b", TS_A);
    fake.show("s2-q1", "こわれた JSON");
    for question in ["s2-q1", "s2-nofile"] {
        let before = fake.log();
        let out = fake.bind(question, TS_A);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{question}: {}", stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{question}: stdout 0 byte");
        assert_eq!(stderr_of(&out), refused("ledger-unreadable", question, TS_A), "{question}");
        assert_eq!(fake.log(), before, "{question}: event log は不変");
        assert!(fake.writes().is_empty(), "{question}: 偽の bd の書きは 0 回");
    }
    assert_eq!(fake.reads(), 2, "ここまでは show を撃った");
    fake.forget();
    let missing = fake.bind("s2-q1", TS_MISSING);
    assert_eq!(stderr_of(&missing), refused("no-utterance", "s2-q1", TS_MISSING), "無い ts");
    let tied = fake.bind("s2-b", TS_A);
    assert_eq!(stderr_of(&tied), refused("bound", "s2-b", TS_A), "結び済み");
    assert!(fake.calls().is_empty(), "偽の bd は 1 回も撃たれない: {:?}", fake.calls());
}

/// (h) 偽の bd が close の撃ちの中で event log の path を dir に替えた周は rc 1 で `partial`（stage=event）・notes と close の書きは残る。
/// log を戻した後の同じ組の撃ち直しは (d) と同じく event だけを足す。
#[test]
fn seat_ruling_bind_event_failure_is_partial_and_the_retry_writes_only_the_event() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", None));
    let path = store::events_path(&fake.state);
    let saved = fake.log();
    fixture(&fake.dir, "close.swap", &path.display().to_string());
    let out = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "stdout 0 byte");
    assert_eq!(stderr_of(&out), "seat ruling: partial stage=event id=s2-q1:20260930T0705Z-1 question=s2-q1 utterance=2026-09-30T07:05:09.123Z\n");
    let verbs: Vec<String> = fake.writes().iter().filter_map(|call| call.first().cloned()).collect();
    assert_eq!(verbs, ["update", "close"], "notes と close の書きは残る");
    assert!(path.is_dir(), "log の path は dir に替わった");
    // log を戻し、bd が閉じた問いと notes を返す形にして撃ち直す。
    fs::remove_dir(&path).ok();
    fs::remove_file(fake.dir.join("close.swap")).ok();
    fs::write(&path, saved).ok();
    fake.show("s2-q1", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, &fake.notes()));
    fake.forget();
    let again = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&again), i32::from(RC_OK), "stderr={}", stderr_of(&again));
    assert_eq!(stdout_of(&again), bound_line("s2-q1", TS_A, "chat"));
    assert!(fake.writes().is_empty(), "撃ち直しは台帳へ書かない: {:?}", fake.writes());
    assert_eq!(fake.rulings().len(), 1, "event だけを足す");
    assert_eq!(fake.notes().lines().count(), 1, "notes は 1 行のまま");
}

/// 結びの形の裁定 event を置き場へ足す（report と doctor の fixture・`add` の撃ちの代わり）。
fn put_ruling(state: &Path, ts: &str, question: &str) {
    let policy = LockPolicy::embedded();
    assert!(policy.is_ok(), "lock の値を読める");
    if let Ok(policy) = policy {
        assert!(store::append(state, &ruling_event(ts, question, TS_A), policy).is_ok(), "裁定 event を足せる");
    }
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
    for (ts, question) in [("2026-09-30T07:05:09Z", "s2-q1"), ("2026-09-30T07:06:09Z", "s2-q2")] {
        put_ruling(&place.state, ts, question);
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

    put_ruling(&place.state, "2026-09-30T07:05:09Z", "s2-q1");
    let minute = "2026-09-30T07:05";
    // 裁定の分と必ず違う分。
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
