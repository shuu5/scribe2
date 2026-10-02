//! 裁定の歯（設計 docs/design/fleet-event-log.md §9 / §14・ADR-0037・ADR-0087・ADR-0089）: 器の結びの口 `seat ruling bind`
//! （接頭辞 `seat_ruling_bind_`）が、記帳された発話と開いた台帳の問いを結び、notes → close → 裁定 event の順に書き、閉じた 4 語で
//! 断り、半端な結びを撃ち直しで仕上げる。`pipe report` が `rulings=` を数え、doctor が manifest の `user <ts>` の行と同じ分の
//! event を突き合わせる（接頭辞 `fleet_ruling_`）。逐語を受ける `seat ruling add` は無い。
//!
//! 共有の helper と fixture は親 module（`tests/e2e/seat.rs`）に在り、`use super::*` で使う。台帳は偽の bd（sh の script）で、
//! 撃たれた引数を `calls.log` に 1 撃ち 1 行（tab 区切り）で残す。

use super::*;
use std::io::Write;
use std::process::Stdio;
use vessel::cli_outcome::RC_BROKEN;
use vessel::fleet::json_lite;
use vessel::fleet::store::{self, LockPolicy};
use vessel::fleet::{Case, Channel, Event, EventKind, Sorting, ACTOR_HUMAN, SCHEMA};

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
/// `close.swap` が在れば中の path を `<path>.aside` へ移して同じ path に dir を置く（移した log は撃ち直しの前に戻せる）・`notes.fail` が在れば
/// update は書かずに rc 1。update と close の撃ちの時点の裁定 event の件数を `*.rulings` に残す。`--readonly list` の撃ち（全部の書き直しの子）は
/// その時点の件数を `list.rulings` に残し・`list.hold` が在れば `list.go` が置かれるまで（上限 30 秒）待ってから `list.ended` を置き・rc 1 を返す。
/// close は repo の `.beads/issues.jsonl` が在れば 1 行足す（台帳の印を書きの前と後で違える）。
const FAKE_BD: &str = "#!/bin/sh
d=$(dirname \"$0\")
first=$1; second=$2; third=$3; fourth=$4
line=$1; shift
for a in \"$@\"; do line=\"$line\t$a\"; done
printf '%s\\n' \"$line\" >> \"$d/calls.log\"
case \"$first\" in
--readonly)
  if [ \"$second\" = list ]; then
    grep -c RulingReceived \"$(cat \"$d/events.path\")\" > \"$d/list.rulings\"
    n=0; while [ -f \"$d/list.hold\" ] && [ ! -f \"$d/list.go\" ] && [ $n -lt 300 ]; do sleep 0.1; n=$((n+1)); done
    : > \"$d/list.ended\"; exit 1
  fi
  [ -f \"$d/show-$third.json\" ] || exit 1
  cat \"$d/show-$third.json\" ;;
update)
  if [ -f \"$d/notes.fail\" ]; then echo 'update refused' >&2; exit 1; fi
  grep -c RulingReceived \"$(cat \"$d/events.path\")\" > \"$d/update.rulings\"
  printf '%s\\n' \"$fourth\" >> \"$d/notes.txt\" ;;
close)
  grep -c RulingReceived \"$(cat \"$d/events.path\")\" > \"$d/close.rulings\"
  if [ -f \"$d/close.fail\" ]; then rm -f \"$d/close.fail\"; echo 'close refused' >&2; exit 1; fi
  if [ -f \"$d/close.swap\" ]; then p=$(cat \"$d/close.swap\"); mv \"$p\" \"$p.aside\"; mkdir \"$p\"; fi
  if [ -f \"$d/repo/.beads/issues.jsonl\" ]; then echo '{}' >> \"$d/repo/.beads/issues.jsonl\"; fi ;;
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
    // 結びの形の裁定 1 件は、突合の行の直後に問いの起票の行（行 l）も足す。
    let next = at.and_then(|found| lines.get(found + 1));
    assert!(next.is_some_and(|found| found.starts_with("binds=1 ")), "突合の行の直後は問いの起票の行: {lines:?}");
    assert_eq!(at.map(|found| found + 2), first_seat, "登録 row の行の直前: {lines:?}");
    assert_eq!(lines.len(), bare.len() + 2, "足すのは 2 行だけ: {lines:?}");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────── 発話の仕分けの口（設計 docs/design/dialogue-surface.md §10・行 i・接頭辞 `utterance_sort_`） ───────────

/// 同じ秒の 2 つの発話の ts（ミリ秒だけが違う）と、別の分の発話の ts。
const TS_B: &str = "2026-09-30T07:05:09.456Z";
const TS_C: &str = "2026-09-30T07:06:00.000Z";

/// `utterance <args…>` を binary で 1 回撃つ。
#[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
fn utterance(args: &[&str]) -> Output {
    Command::new(bin()).arg("utterance").args(args).output().expect("binary を起動できる")
}

/// 開いた memo（label intake:memo）の show。
fn open_memo() -> String {
    show_json("open", &["intake:memo"], "2026-09-30T06:00:00Z", None, "")
}

/// 断りの 1 行（設計 §10 約束 5）。
fn refused_sort(reason: &str, ts: &str) -> String {
    format!("utterance: refused reason={reason} ts={ts}\n")
}

impl Fake {
    /// `utterance sort` を撃つ（`--repo` と `--bd` は偽の bd・会話の周でも渡し、台帳が読まれないことを呼び出しの記録で測る）。
    fn sort_with(&self, ts: &str, how: &[&str]) -> Output {
        let (repo, state) = (self.repo.display().to_string(), self.state.display().to_string());
        let mut args = vec!["sort", "--state-dir", state.as_str(), "--ts", ts, "--repo", repo.as_str(), "--bd", self.bd.as_str()];
        args.extend_from_slice(how);
        utterance(&args)
    }

    /// 要望の仕分け（`--as request --memo <memo>`）。
    fn request(&self, ts: &str, memo: &str) -> Output {
        self.sort_with(ts, &["--as", "request", "--memo", memo])
    }

    /// 会話の仕分け（`--as chat`）。
    fn chat(&self, ts: &str) -> Output {
        self.sort_with(ts, &["--as", "chat"])
    }

    /// `utterance show` を撃つ。
    fn shown(&self, ts: &str) -> Output {
        utterance(&["show", "--state-dir", self.state.display().to_string().as_str(), "--ts", ts])
    }

    /// log の仕分けの event の (bead, 本体)（物理順）。
    fn sorted(&self) -> Vec<(String, Option<Case>)> {
        let all = store::read_all(&self.state).unwrap_or_default();
        all.into_iter().filter(|found| found.kind == EventKind::UtteranceSorted).map(|found| (found.bead, found.case)).collect()
    }
}

/// 仕分けの本体（発話の ts と仕分け）。
fn sorted_case(utterance: &str, sorting: Sorting) -> Option<Case> {
    Some(Case::Sorted { utterance: utterance.to_owned(), sorting })
}

/// 通った周の外形: rc 0・stderr 0 byte・stdout が期待の 1 行・逐語の字を出さない。
fn assert_ok_line(out: &Output, expected: &str, label: &str) {
    assert_eq!(rc_of(out), i32::from(RC_OK), "{label}: stderr={}", stderr_of(out));
    assert!(stderr_of(out).is_empty(), "{label}: stderr 0 byte");
    assert_eq!(stdout_of(out), expected, "{label}");
    assert!(!stdout_of(out).contains(WORDS_MARKS), "{label}: 逐語の字を出さない");
}

/// 断った周の外形: rc 1・stdout 0 byte・stderr が 1 行の断り・event log は撃つ前のまま・偽の bd の書きは 0 回。
fn assert_refused_sort(fake: &Fake, out: &Output, expected: &str, before: &str, label: &str) {
    assert_eq!(rc_of(out), i32::from(RC_REFUSED), "{label}: {}", stderr_of(out));
    assert!(stdout_of(out).is_empty(), "{label}: stdout 0 byte");
    assert_eq!(stderr_of(out), expected, "{label}");
    assert_eq!(fake.log(), before, "{label}: event log は不変");
    assert!(fake.writes().is_empty(), "{label}: 偽の bd の書きは 0 回: {:?}", fake.writes());
}

/// (a) 要望と会話がそれぞれ仕分けの event を 1 件だけ書き（要望は bead に memo の id・会話は無し・actor machine）、偽の bd の書きは 0 回。
/// 会話の周は偽の bd の呼び出しが 0 行（台帳を読まない）で、読めない JSON を返す memo が在っても通る。
#[test]
fn utterance_sort_writes_one_event_for_a_request_and_for_a_chat_without_touching_the_ledger() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.say(TS_B, Channel::Gui, WORDS);
    fake.show("s2-m1", &open_memo());
    fake.show("s2-broken", "こわれた JSON");
    let before = fake.log();
    assert_ok_line(&fake.request(TS_A, "s2-m1"), &format!("utterance: sorted ts={TS_A} as=request memo=s2-m1\n"), "要望");
    assert_eq!(fake.log().lines().count(), before.lines().count() + 1, "log は 1 行だけ増える");
    assert!(fake.log().starts_with(&before), "追記だけ");
    assert_eq!(fake.sorted(), [("s2-m1".to_owned(), sorted_case(TS_A, Sorting::Request))], "要望の 1 件");
    assert!(fake.writes().is_empty(), "偽の bd の書きは 0 回: {:?}", fake.writes());
    assert_eq!(fake.reads(), 1, "台帳の読みは memo 1 本");
    let last = store::read_all(&fake.state).unwrap_or_default().pop();
    assert_eq!(last.map(|found| (found.actor, found.detail)), Some(("machine".to_owned(), None)), "actor machine・逐語を持たない");
    fake.forget();
    assert_ok_line(&fake.chat(TS_B), &format!("utterance: sorted ts={TS_B} as=chat\n"), "会話");
    let expected = [("s2-m1".to_owned(), sorted_case(TS_A, Sorting::Request)), (String::new(), sorted_case(TS_B, Sorting::Chat))];
    assert_eq!(fake.sorted(), expected, "会話の 1 件が足される");
    assert!(fake.calls().is_empty(), "会話は台帳を 1 度も撃たない: {:?}", fake.calls());
}

/// (b) 1 つの発話を 2 つの memo へ仕分けられる（同じ組だけが already）。
#[test]
fn utterance_sort_one_utterance_can_be_sorted_to_two_memos() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    for memo in ["s2-m1", "s2-m2"] {
        fake.show(memo, &open_memo());
        assert_ok_line(&fake.request(TS_A, memo), &format!("utterance: sorted ts={TS_A} as=request memo={memo}\n"), memo);
    }
    let expected = [("s2-m1".to_owned(), sorted_case(TS_A, Sorting::Request)), ("s2-m2".to_owned(), sorted_case(TS_A, Sorting::Request))];
    assert_eq!(fake.sorted(), expected, "別の memo の 2 件");
    assert!(fake.writes().is_empty(), "台帳は書かない");
}

/// (c) 断りの 7 形（無い ts の no-utterance を request と chat の 2 形・答えを持つ発話への会話と要望を持つ発話への会話の linked 2 形・
/// 無い bead / 閉じた memo / label の無い bead の not-memo 3 形）が、どれも rc 1・stdout 0 byte・stderr が 1 行の断りと逐語で一致し、
/// event log も偽の bd の書きも変えない。
#[test]
fn utterance_sort_refuses_the_seven_forms_without_writing() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.tie("s2-q1", TS_A);
    fake.say(TS_B, Channel::Chat, WORDS);
    fake.say(TS_C, Channel::Chat, WORDS);
    fake.show("s2-m1", &open_memo());
    assert_eq!(rc_of(&fake.request(TS_B, "s2-m1")), i32::from(RC_OK), "要望を持つ発話を用意する");
    fake.show("s2-closed", &show_json("closed", &["intake:memo"], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-plain", &show_json("open", &[], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-gone", "[]");
    type Call = fn(&Fake) -> Output;
    let cases: [(&str, &str, &str, Call); 7] = [
        ("要望の無い ts", "no-utterance", TS_MISSING, |fake| fake.request(TS_MISSING, "s2-m1")),
        ("会話の無い ts", "no-utterance", TS_MISSING, |fake| fake.chat(TS_MISSING)),
        ("答えを持つ発話への会話", "linked", TS_A, |fake| fake.chat(TS_A)),
        ("要望を持つ発話への会話", "linked", TS_B, |fake| fake.chat(TS_B)),
        ("無い bead", "not-memo", TS_C, |fake| fake.request(TS_C, "s2-gone")),
        ("閉じた memo", "not-memo", TS_C, |fake| fake.request(TS_C, "s2-closed")),
        ("label の無い bead", "not-memo", TS_C, |fake| fake.request(TS_C, "s2-plain")),
    ];
    for (label, reason, ts, call) in cases {
        let before = fake.log();
        let out = call(&fake);
        assert_refused_sort(&fake, &out, &refused_sort(reason, ts), &before, label);
    }
}

/// (c2) 断りの順: 無い ts ∧ 読めない JSON の request と、無い ts ∧ 開いた memo でない名指しの request が、どちらも no-utterance だけを出し、
/// 台帳は 1 度も読まれない（event log の判定が台帳より先）。
#[test]
fn utterance_sort_a_missing_utterance_is_refused_before_the_ledger_is_read() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-broken", "こわれた JSON");
    fake.show("s2-plain", &show_json("open", &[], "2026-09-30T06:00:00Z", None, ""));
    for memo in ["s2-broken", "s2-plain"] {
        let before = fake.log();
        let out = fake.request(TS_MISSING, memo);
        assert_refused_sort(&fake, &out, &refused_sort("no-utterance", TS_MISSING), &before, memo);
    }
    assert!(fake.calls().is_empty(), "台帳は読まれない: {:?}", fake.calls());
}

/// (d) 同じ秒の 2 つの発話（ミリ秒だけが違う）を ts で別々に仕分けられる。秒までの ts は別の発話を指せず no-utterance。
#[test]
fn utterance_sort_two_utterances_of_the_same_second_are_sorted_apart_by_ts() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.say(TS_B, Channel::Chat, WORDS);
    fake.show("s2-m1", &open_memo());
    assert_eq!(rc_of(&fake.request(TS_A, "s2-m1")), i32::from(RC_OK));
    assert_eq!(rc_of(&fake.chat(TS_B)), i32::from(RC_OK), "隣の発話は要望を持たない");
    let expected = [("s2-m1".to_owned(), sorted_case(TS_A, Sorting::Request)), (String::new(), sorted_case(TS_B, Sorting::Chat))];
    assert_eq!(fake.sorted(), expected, "ts ごとに 1 件");
    let before = fake.log();
    let out = fake.chat("2026-09-30T07:05:09Z");
    assert_refused_sort(&fake, &out, &refused_sort("no-utterance", "2026-09-30T07:05:09Z"), &before, "秒までの ts");
}

/// (e) `utterance show` は逐語を 1 byte も違わずに 1 件だけ返し（末尾の改行 1 つ）、3 つの発話を持つ log で真ん中の ts は真ん中の逐語だけ。
/// 同じ ts を持つ発話でない event の逐語は返さず、無い ts は rc 1 で no-utterance。log も台帳も動かさない。
#[test]
fn utterance_sort_show_returns_the_verbatim_words_of_one_utterance() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, "最初の発話");
    fake.say(TS_B, Channel::Gui, WORDS);
    fake.say(TS_C, Channel::Chat, "最後の発話");
    fake.tie("s2-q9", TS_A);
    let before = fake.log();
    for (ts, words) in [(TS_A, "最初の発話"), (TS_B, WORDS), (TS_C, "最後の発話")] {
        let out = fake.shown(ts);
        assert_eq!(rc_of(&out), i32::from(RC_OK), "{ts}: stderr={}", stderr_of(&out));
        assert_eq!(stdout_of(&out), format!("{words}\n"), "{ts}: 逐語だけ");
        assert!(stderr_of(&out).is_empty(), "{ts}: stderr 0 byte");
    }
    let out = fake.shown(TS_MISSING);
    assert_refused_sort(&fake, &out, &refused_sort("no-utterance", TS_MISSING), &before, "無い ts（同じ ts の裁定 event が在っても）");
    assert!(fake.calls().is_empty(), "show は台帳を読まない");
}

/// (e2) 同じ ts と同じ memo の request と、会話の札が在る発話への chat は、どちらも rc 0・stdout が `already` の 1 行で、event log は不変。
/// 要望は台帳より先に見る（最初の周の後に memo が閉じても already で、台帳の読みは増えない）。
#[test]
fn utterance_sort_the_same_pair_twice_is_already_and_writes_nothing() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.say(TS_B, Channel::Chat, WORDS);
    fake.show("s2-m1", &open_memo());
    assert_eq!(rc_of(&fake.request(TS_A, "s2-m1")), i32::from(RC_OK));
    assert_eq!(rc_of(&fake.chat(TS_B)), i32::from(RC_OK));
    fake.show("s2-m1", &show_json("closed", &["intake:memo"], "2026-09-30T06:00:00Z", None, ""));
    let (before, reads) = (fake.log(), fake.reads());
    assert_ok_line(&fake.request(TS_A, "s2-m1"), "already\n", "同じ組の要望");
    assert_ok_line(&fake.chat(TS_B), "already\n", "会話の札が在る発話への会話");
    assert_eq!(fake.log(), before, "event log は不変");
    assert_eq!(fake.reads(), reads, "already は台帳を読まない");
}

/// (e3) 偽の bd の show が読めない JSON を返す周と、show が rc 1 の周の request は、rc 1・stdout 0 byte・stderr の 1 行が
/// `reason=ledger-unreadable` で、event log は不変。
#[test]
fn utterance_sort_an_unreadable_ledger_refuses_a_request_and_writes_nothing() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.show("s2-broken", "こわれた JSON");
    for memo in ["s2-broken", "s2-nofile"] {
        let before = fake.log();
        let out = fake.request(TS_A, memo);
        assert_refused_sort(&fake, &out, &refused_sort("ledger-unreadable", TS_A), &before, memo);
    }
    assert_eq!(fake.reads(), 2, "どちらも台帳を読みに行った");
}

/// (e4) 読みの範囲: 対象の発話の後に発話でない event を 2 MB 続けた log で、会話の sort と要望の sort と show が通る（末尾の窓だけを読む実装を落とす）。
#[test]
fn utterance_sort_reads_the_whole_log_past_two_megabytes_of_later_events() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    fake.say(TS_B, Channel::Chat, WORDS);
    fake.show("s2-m1", &open_memo());
    let filler = "あ".repeat(2700);
    for _ in 0..260 {
        fake.put(&event(EventKind::RulingReceived, "2026-09-30T08:00:00Z", "s2-x.1", Some(&filler), None));
    }
    assert!(fake.log().len() >= 2 * 1024 * 1024, "log は 2 MB を超える: {}", fake.log().len());
    assert_ok_line(&fake.chat(TS_A), &format!("utterance: sorted ts={TS_A} as=chat\n"), "会話");
    assert_ok_line(&fake.request(TS_B, "s2-m1"), &format!("utterance: sorted ts={TS_B} as=request memo=s2-m1\n"), "要望");
    let shown = fake.shown(TS_A);
    assert_eq!((rc_of(&shown), stdout_of(&shown)), (i32::from(RC_OK), format!("{WORDS}\n")), "show: {}", stderr_of(&shown));
}

/// 使い方の誤り（未知の語・`--as` の語・要る flag の欠け・`--memo` を持つ会話・空文字・余分な位置引数）は rc 2 で使い方の行で終わり、
/// 何も書かない。第 1 token の無い周と未知の語は使い方の 1 行と rc 1。`--help` は使い方を stdout に出して rc 0。
#[test]
fn utterance_sort_usage_errors_write_nothing_and_end_with_the_usage_line() {
    let fake = Fake::new();
    fake.say(TS_A, Channel::Chat, WORDS);
    let state = fake.state.display().to_string();
    let usage = vessel::utterance::cli::usage();
    let before = fake.log();
    let bad: [&[&str]; 8] = [
        &["sort", "--state-dir", &state, "--ts", TS_A, "--as", "maybe"],
        &["sort", "--state-dir", &state, "--ts", TS_A, "--as", "request"],
        &["sort", "--state-dir", &state, "--ts", TS_A, "--as", "chat", "--memo", "s2-m1"],
        &["sort", "--state-dir", &state, "--as", "chat"],
        &["sort", "--state-dir", &state, "--ts", "", "--as", "chat"],
        &["sort", "--state-dir", &state, "--ts", TS_A, "--as", "chat", "extra"],
        &["show", "--state-dir", &state],
        &["show", "--state-dir", &state, "--ts", TS_A, "--unknown", "x"],
    ];
    for args in bad {
        let out = utterance(args);
        assert_eq!(rc_of(&out), 2, "{args:?}: {}", stderr_of(&out));
        assert!(stdout_of(&out).is_empty(), "{args:?}: stdout 0 byte");
        assert_eq!(stderr_of(&out).lines().last(), Some(usage.as_str()), "{args:?}: 使い方の行で終わる");
    }
    for args in [&[][..], &["nosuch"]] {
        let out = utterance(args);
        assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{args:?}");
        assert_eq!(stderr_of(&out), format!("{usage}\n"), "{args:?}: 使い方の 1 行");
        assert!(stdout_of(&out).is_empty(), "{args:?}: stdout 0 byte");
    }
    let help = utterance(&["sort", "--help"]);
    assert_eq!((rc_of(&help), stdout_of(&help)), (i32::from(RC_OK), format!("{usage}\n")), "--help");
    assert_eq!(fake.log(), before, "どの周も log は不変");
    assert!(fake.calls().is_empty(), "台帳は撃たれない");
}

// ─────────── 裁定面の答えの口（設計 docs/design/dialogue-surface.md §11・行 j・接頭辞 `seat_ruling_answer_`） ───────────

/// 答えの逐語: 前後に空白・途中に改行と `"`・末尾に改行 2 つ（標準入力の byte をそのまま持つ）。
const ANSWER_WORDS: &str = "  推奨で進めて \"Ω\" を採る\n二行目も逐語  \n\n";

/// 標準入力の印（使い方の字）。
const STDIN_MARK: &str = "(stdin: WORDS)";

impl Fake {
    /// `seat ruling answer` を撃つ（逐語は標準入力へ書いて閉じる）。
    #[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
    fn answer(&self, question: &str, words: &str) -> Output {
        let (repo, state) = (self.repo.display().to_string(), self.state.display().to_string());
        let mut child = Command::new(bin())
            .args(["seat", "ruling", "answer", "--repo", &repo, "--state-dir", &state, "--question", question, "--bd", &self.bd])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("binary を起動できる");
        child.stdin.take().expect("stdin を開ける").write_all(words.as_bytes()).expect("逐語を書ける");
        child.wait_with_output().expect("終わりを待てる")
    }

    /// log の発話 event（物理順）。
    fn utterances(&self) -> Vec<Event> {
        store::read_all(&self.state).unwrap_or_default().into_iter().filter(|found| found.kind == EventKind::UtteranceReceived).collect()
    }

    /// log の承認 event の件数。
    fn approvals(&self) -> usize {
        store::read_all(&self.state).unwrap_or_default().iter().filter(|found| found.kind == EventKind::ApprovalReceived).count()
    }
}

/// 発話の ts から作る裁定 id（`<問い id>:<YYYYMMDDTHHMMZ>-1`）。
fn answered_id(question: &str, ts: &str) -> String {
    let part = |range: std::ops::Range<usize>| ts.get(range).unwrap_or_default().to_owned();
    format!("{question}:{}{}{}T{}{}Z-1", part(0..4), part(5..7), part(8..10), part(11..13), part(14..16))
}

/// 断った周の外形: rc 1・stdout 0 byte・stderr が 1 行の断りと逐語で一致し、event log（発話 event を含む）も偽の bd の書きも撃つ前と同じ。
fn assert_answer_refused(fake: &Fake, question: &str, words: &str, reason: &str) {
    let before = fake.log();
    let out = fake.answer(question, words);
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "{reason}: {}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "{reason}: stdout 0 byte");
    assert_eq!(stderr_of(&out), format!("seat ruling: refused reason={reason} question={question}\n"), "{reason}");
    assert_eq!(fake.log(), before, "{reason}: event log は不変（発話 event も書かない）");
    assert!(fake.writes().is_empty(), "{reason}: 偽の bd の書きは 0 回: {:?}", fake.writes());
}

/// 偽の bd への書きが notes（5 欄・経路 gui・最後の欄を戻した字が標準入力の byte と一致）→ close（理由 `裁定 <id>`）の順に 1 回ずつで、
/// 裁定 event はどちらの撃ちの時点でも 0 件。
fn assert_answer_notes_then_close(fake: &Fake, id: &str, ts: &str) {
    let notes = fake.notes();
    let row = notes.trim_end_matches('\n');
    assert_eq!(notes.lines().count(), 1, "notes の行は 1 行（改行を含む逐語でも）");
    let fields: Vec<&str> = row.splitn(5, " | ").collect();
    assert_eq!(fields.len(), 5, "5 欄: {notes}");
    assert_eq!(fields.iter().take(4).copied().collect::<Vec<_>>(), [id, "s2-q1", ts, "gui"], "先頭の 4 欄");
    assert_eq!(unquote(fields.last().copied().unwrap_or_default()), ANSWER_WORDS, "最後の欄を戻した字は標準入力の byte と一致");
    let expected = [
        vec!["update".to_owned(), "s2-q1".to_owned(), "--append-notes".to_owned(), row.to_owned()],
        vec!["close".to_owned(), "s2-q1".to_owned(), "--reason".to_owned(), format!("裁定 {id}")],
    ];
    assert_eq!(fake.writes(), expected, "notes → close の順に 1 回ずつ");
    assert_eq!((fake.seen("update.rulings"), fake.seen("close.rulings")), ("0".to_owned(), "0".to_owned()), "裁定 event は close の後");
}

/// (a) 通る周: 経路 gui の発話 event と 5 欄の行（経路 gui）と close と裁定 event が 1 件ずつ・この順に書かれ、stdout が裁定 id の 1 行だけ、
/// 承認 event は 0 件。標準入力は前後に空白と末尾の改行 2 つを持ち、発話 event の detail と 5 欄の行の最後の欄を戻した字と裁定 event の
/// detail が、どれも標準入力の byte と一致する。
#[test]
fn seat_ruling_answer_writes_the_four_records_and_prints_only_the_ruling_id() {
    let fake = Fake::new();
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", Some("user")));
    let out = fake.answer("s2-q1", ANSWER_WORDS);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let said = fake.utterances();
    assert_eq!(said.len(), 1, "発話 event は 1 件: {said:?}");
    let said = said.first().cloned().unwrap_or_else(event_none);
    assert_eq!(said.detail.as_deref(), Some(ANSWER_WORDS), "発話の detail は標準入力の byte と一致");
    assert_eq!(said.case, Some(Case::Utterance { channel: Channel::Gui, session: None }), "経路 gui・session 無し");
    assert_eq!((said.actor.as_str(), said.run.as_str()), (ACTOR_HUMAN, ""), "actor human・run 無し");
    let id = answered_id("s2-q1", &said.ts);
    assert_eq!(stdout_of(&out), format!("{id}\n"), "stdout は裁定 id の 1 行だけ");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
    assert!(!stdout_of(&out).contains(WORDS_MARKS), "逐語を載せない");
    assert_answer_notes_then_close(&fake, &id, &said.ts);
    let found = fake.rulings();
    assert_eq!(found.len(), 1, "裁定 event は 1 件: {found:?}");
    let event = found.first().cloned().unwrap_or_else(event_none);
    assert_eq!(event.detail.as_deref(), Some(ANSWER_WORDS), "裁定 event の detail も標準入力の byte と一致");
    let case = Case::Ruling {
        ruling: id,
        utterance: said.ts.clone(),
        channel: Channel::Gui,
        question_ts: "2026-09-30T06:00:00Z".to_owned(),
        asked: Some("user".to_owned()),
    };
    assert_eq!(event.case, Some(case), "本体は発話の ts と経路 gui");
    assert_eq!(fake.approvals(), 0, "承認 event は書かない");
    let order: Vec<EventKind> = store::read_all(&fake.state).unwrap_or_default().into_iter().map(|found| found.kind).collect();
    assert_eq!(order, [EventKind::UtteranceReceived, EventKind::RulingReceived], "発話 → 裁定の順");
}

/// (b) 断りの 4 語の全部: 空白だけの逐語（`words-empty`）・偽の bd の show が読めない JSON を返す（`ledger-unreadable`・show が無い bead の rc 1 も）・
/// 閉じた問い（`closed`）・問いでない bead（`not-question`・無い bead と label の無い bead）。どれも何も書かない（発話 event も）。
#[test]
fn seat_ruling_answer_refuses_the_four_words_without_writing() {
    let fake = Fake::new();
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", None));
    fake.show("s2-broken", "こわれた JSON");
    fake.show("s2-closed", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-plain", &show_json("open", &[], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-gone", "[]");
    let cases = [
        ("s2-q1", " \n\t  \n", "words-empty"),
        ("s2-q1", "", "words-empty"),
        ("s2-broken", ANSWER_WORDS, "ledger-unreadable"),
        ("s2-nofile", ANSWER_WORDS, "ledger-unreadable"),
        ("s2-closed", ANSWER_WORDS, "closed"),
        ("s2-plain", ANSWER_WORDS, "not-question"),
        ("s2-gone", ANSWER_WORDS, "not-question"),
    ];
    for (question, words, reason) in cases {
        assert_answer_refused(&fake, question, words, reason);
    }
    assert!(fake.utterances().is_empty(), "どの周も発話 event を書かない");
}

/// (d) 断りの順: 2 つの断りに同時に当たる 3 形で先の語だけが出る（空白だけの逐語 + 読めない台帳 → `words-empty`・空白だけの逐語 + 閉じた問い →
/// `words-empty`・閉じていて問いでない bead → `closed`）。
#[test]
fn seat_ruling_answer_refusal_order_is_words_then_ledger_then_closed_then_question() {
    let fake = Fake::new();
    fake.show("s2-broken", "こわれた JSON");
    fake.show("s2-closed", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, ""));
    fake.show("s2-closed-plain", &show_json("closed", &[], "2026-09-30T06:00:00Z", None, ""));
    let cases = [
        ("s2-broken", "  \n", "words-empty"),
        ("s2-closed", "\t", "words-empty"),
        ("s2-closed-plain", ANSWER_WORDS, "closed"),
    ];
    for (question, words, reason) in cases {
        assert_answer_refused(&fake, question, words, reason);
    }
}

/// (c) 頂点の 16 語の `help <語>` の FORM の行の全部（16 本・数を母集団として出す）で、`(stdin: WORDS)` は seat の行の `ruling answer` の form に
/// 1 回だけ在り、ほかの 15 本と seat の行のほかの form（`ruling bind` を含む）には無い。
#[test]
fn seat_ruling_answer_stdin_mark_sits_only_in_its_own_form_of_the_seat_line() {
    let words = crate::help_top_words();
    let mut forms: Vec<(String, String)> = Vec::new();
    for word in &words {
        let run = crate::help_run(&["help", word]);
        forms.extend(crate::help_section(&run.out, "FORM").into_iter().map(|line| (word.clone(), line.to_owned())));
    }
    assert_eq!(forms.len(), 16, "FORM の行の母集団は 16: {forms:?}");
    let holders: Vec<&str> = forms.iter().filter(|(_, line)| line.contains(STDIN_MARK)).map(|(word, _)| word.as_str()).collect();
    assert_eq!(holders, ["seat"], "印を持つ行は seat の 1 本だけ");
    let seat = forms.iter().find(|(word, _)| word == "seat").map(|(_, line)| line.clone()).unwrap_or_default();
    assert_eq!(seat.matches(STDIN_MARK).count(), 1, "seat の行に 1 回だけ: {seat}");
    let split = |from: &str, to: &str| seat.split_once(from).and_then(|(_, rest)| rest.split_once(to)).map(|(form, _)| form.to_owned()).unwrap_or_default();
    assert!(split("|ruling answer ", "|ruling ls").contains(STDIN_MARK), "ruling answer の form の中: {seat}");
    assert!(!split("|ruling bind ", "|ruling answer").contains(STDIN_MARK), "ruling bind の form には無い: {seat}");
    let tail = seat.split_once("|ruling ls").map(|(_, rest)| rest.to_owned()).unwrap_or_default();
    assert!(!tail.contains(STDIN_MARK), "ほかの form には無い: {seat}");
}

/// (e) 書きの途中の失敗: 偽の bd が close の撃ちの中で event log の file を脇へ移して同じ path に dir を置く。答えの口は rc 1 で
/// `partial utterance=<ts>` の 1 行を出し、その ts は脇へ移した log の経路 gui の発話 event の ts と一致し、notes と close の書きは残る。
/// log を戻して同じ問いと ts で `seat ruling bind` を撃つと rc 0 で、裁定 event が 1 件・発話 event は 1 件のまま。
#[test]
fn seat_ruling_answer_event_failure_is_partial_and_bind_finishes_the_same_utterance() {
    let fake = Fake::new();
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", None));
    let path = store::events_path(&fake.state);
    fixture(&fake.dir, "close.swap", &path.display().to_string());
    let out = fake.answer("s2-q1", ANSWER_WORDS);
    let aside = PathBuf::from(format!("{}.aside", path.display()));
    let moved: Vec<Event> = fs::read_to_string(&aside).unwrap_or_default().lines().filter_map(|line| Event::from_line(line).ok()).collect();
    let said: Vec<&Event> = moved.iter().filter(|found| found.kind == EventKind::UtteranceReceived).collect();
    assert_eq!(said.len(), 1, "脇へ移した log に発話 event が 1 件: {moved:?}");
    let ts = said.first().map(|found| found.ts.clone()).unwrap_or_default();
    assert_eq!(said.first().map(|found| found.case.clone()), Some(Some(Case::Utterance { channel: Channel::Gui, session: None })), "経路 gui");
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "stdout 0 byte");
    assert_eq!(stderr_of(&out), format!("seat ruling: partial utterance={ts} question=s2-q1\n"));
    let verbs: Vec<String> = fake.writes().iter().filter_map(|call| call.first().cloned()).collect();
    assert_eq!(verbs, ["update", "close"], "notes と close の書きは残る");
    assert!(path.is_dir(), "log の path は dir に替わった");
    fs::remove_dir(&path).ok();
    fs::rename(&aside, &path).ok();
    fake.show("s2-q1", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, &fake.notes()));
    fake.forget();
    let again = fake.bind("s2-q1", &ts);
    assert_eq!(rc_of(&again), i32::from(RC_OK), "stderr={}", stderr_of(&again));
    assert!(fake.writes().is_empty(), "結び直しは台帳へ書かない: {:?}", fake.writes());
    assert_eq!(fake.rulings().len(), 1, "裁定 event は 1 件");
    assert_eq!(fake.utterances().len(), 1, "発話 event は 1 件のまま");
}

/// (e2) notes の追記の失敗: 偽の bd の append-notes が rc 1 を返す周も、答えの口は rc 1 で `partial utterance=<ts>` の 1 行を出し、発話 event は
/// 1 件残り、close は撃たれない（close の途中の止まりだけを partial にする実装を落とす）。
#[test]
fn seat_ruling_answer_notes_failure_is_partial_and_keeps_the_utterance_without_a_close() {
    let fake = Fake::new();
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", None));
    fixture(&fake.dir, "notes.fail", "");
    let out = fake.answer("s2-q1", ANSWER_WORDS);
    let said = fake.utterances();
    assert_eq!(said.len(), 1, "発話 event は 1 件残る: {said:?}");
    let ts = said.first().map(|found| found.ts.clone()).unwrap_or_default();
    assert_eq!(rc_of(&out), i32::from(RC_REFUSED), "stderr={}", stderr_of(&out));
    assert!(stdout_of(&out).is_empty(), "stdout 0 byte");
    assert_eq!(stderr_of(&out), format!("seat ruling: partial utterance={ts} question=s2-q1\n"));
    let verbs: Vec<String> = fake.writes().iter().filter_map(|call| call.first().cloned()).collect();
    assert_eq!(verbs, ["update"], "append-notes だけを撃ち close は撃たない");
    assert!(fake.rulings().is_empty(), "裁定 event は書かない");
}

// ─────────── 問いの起票が発話より後の結びの doctor の 1 行（設計 docs/design/dialogue-surface.md §13・行 l・接頭辞 `doctor_asked_after_`） ───────────

/// 発話の ts（ミリ秒の形）と、それより後・前の問いの起票の時刻。
const SAID: &str = "2026-09-30T07:05:09.123Z";
const ASKED_AFTER: &str = "2026-09-30T08:00:00Z";
const ASKED_BEFORE: &str = "2026-09-30T06:00:00Z";

/// 古い裁定 event（utterance を持たない・rule だけの形）を置き場へ足す。
fn put_old_ruling(state: &Path) {
    let old = Event { rule: Some("R-C9-1".to_owned()), ..event(EventKind::RulingReceived, "2026-09-22T01:02:03Z", "s2-x.1", Some("古い逐語"), None) };
    let policy = LockPolicy::embedded();
    assert!(policy.is_ok(), "lock の値を読める");
    if let Ok(policy) = policy {
        assert!(store::append(state, &old, policy).is_ok(), "古い裁定を足せる");
    }
}

/// 結びの形の裁定 event を、裁定 id・（発話の ts・問いの起票の時刻）・asked つきで置き場へ足す。
fn put_bound(state: &Path, id: &str, (utterance, question_ts): (&str, &str), asked: Option<&str>) {
    let case = Case::Ruling {
        ruling: id.to_owned(),
        utterance: utterance.to_owned(),
        channel: Channel::Chat,
        question_ts: question_ts.to_owned(),
        asked: asked.map(str::to_owned),
    };
    let policy = LockPolicy::embedded();
    assert!(policy.is_ok(), "lock の値を読める");
    if let Ok(policy) = policy {
        let found = event(EventKind::RulingReceived, "2026-09-30T09:00:00Z", "s2-q", Some("推奨で"), Some(case));
        assert!(store::append(state, &found, policy).is_ok(), "結びの裁定 event を足せる");
    }
}

/// 7 件の fixture: asked が seat と user でそれぞれ発話の前と後の 4 結び・asked の無い後の結び 1（裁定 id が字の順で後ろ・log で先）・asked の無い
/// 前の結び 1（裁定 id が字の順で前・log で後）・utterance を持たない古い裁定 1。
fn put_seven(state: &Path) {
    put_old_ruling(state);
    put_bound(state, "s2-n9:20260930T0705Z-1", (SAID, ASKED_AFTER), None);
    put_bound(state, "s2-sb:20260930T0705Z-1", (SAID, ASKED_BEFORE), Some("seat"));
    put_bound(state, "s2-sa:20260930T0705Z-1", (SAID, ASKED_AFTER), Some("seat"));
    put_bound(state, "s2-ub:20260930T0705Z-1", (SAID, ASKED_BEFORE), Some("user"));
    put_bound(state, "s2-ua:20260930T0705Z-1", (SAID, ASKED_AFTER), Some("user"));
    put_bound(state, "s2-n1:20260930T0705Z-1", (SAID, ASKED_BEFORE), None);
}

/// doctor の出力行のうち `binds=` で始まる行（無ければ `None`）。
fn binds_line(place: &RolePlace) -> Option<String> {
    doctor_rows(place, NO_ACCOUNT_RULES).into_iter().find(|line| line.starts_with("binds="))
}

/// (a) 7 件の fixture で、doctor の行が 7 語の順・件数・裁定 id（asked-none は log の順で字の順と逆）と一致する。
#[test]
fn doctor_asked_after_counts_each_asked_value_and_lists_the_ids_in_log_order() {
    let place = dialogue_place();
    put_seven(&place.state);
    let expected = "binds=6 after-seat=1 after-seat-ids=s2-sa:20260930T0705Z-1 after-user=1 after-user-ids=s2-ua:20260930T0705Z-1 \
                    asked-none=2 asked-none-ids=s2-n9:20260930T0705Z-1,s2-n1:20260930T0705Z-1";
    assert_eq!(binds_line(&place).as_deref(), Some(expected));
    fs::remove_dir_all(&place.dir).ok();
}

/// (b) 古い裁定だけの置き場（母集団 0）は行を出さない。
#[test]
fn doctor_asked_after_prints_no_line_when_only_old_rulings_exist() {
    let place = dialogue_place();
    put_old_ruling(&place.state);
    let rows = doctor_rows(&place, NO_ACCOUNT_RULES);
    assert!(!rows.iter().any(|line| line.contains("binds=")), "母集団 0 は行を出さない: {rows:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (c) log が読めない置き場は `binds=unreadable`。
#[test]
fn doctor_asked_after_names_an_unreadable_log() {
    let place = dialogue_place();
    fs::create_dir_all(place.state.join("fleet")).ok();
    fs::write(store::events_path(&place.state), "こわれ\n").ok();
    let rows = doctor_rows(&place, NO_ACCOUNT_RULES);
    assert_eq!(rows.iter().filter(|line| line.as_str() == "binds=unreadable").count(), 1, "{rows:?}");
    fs::remove_dir_all(&place.dir).ok();
}

/// (d) asked が seat で question_ts が読めない結びを足すと、binds が 1 増え `unreadable=1` が末尾に付き、after-* と asked-none の数は変わらない。
/// utterance が読めない結びを足した形でも同じ。
#[test]
fn doctor_asked_after_keeps_unreadable_times_out_of_the_after_columns() {
    let base = "binds=7 after-seat=1 after-seat-ids=s2-sa:20260930T0705Z-1 after-user=1 after-user-ids=s2-ua:20260930T0705Z-1 \
                asked-none=2 asked-none-ids=s2-n9:20260930T0705Z-1,s2-n1:20260930T0705Z-1 unreadable=1";
    for (utterance, question_ts) in [(SAID, "not-a-time"), ("2026-09-30T07:05:09Z", ASKED_AFTER)] {
        let place = dialogue_place();
        put_seven(&place.state);
        put_bound(&place.state, "s2-bad:20260930T0705Z-1", (utterance, question_ts), Some("seat"));
        assert_eq!(binds_line(&place).as_deref(), Some(base), "{utterance} / {question_ts}");
        fs::remove_dir_all(&place.dir).ok();
    }
}

/// (e) asked が user の後の結びを持たない置き場（seat の後の結び 1・asked の無い結び 1）で、行が `after-user=0 after-user-ids=-` を持つ。
#[test]
fn doctor_asked_after_prints_a_dash_for_an_empty_id_column() {
    let place = dialogue_place();
    put_bound(&place.state, "s2-sa:20260930T0705Z-1", (SAID, ASKED_AFTER), Some("seat"));
    put_bound(&place.state, "s2-n1:20260930T0705Z-1", (SAID, ASKED_BEFORE), None);
    let line = binds_line(&place).unwrap_or_default();
    assert!(line.contains(" after-user=0 after-user-ids=- "), "{line}");
    assert_eq!(line.split(' ').count(), 7, "7 語: {line}");
    fs::remove_dir_all(&place.dir).ok();
}

// ─────────── 結びと答えの口が起こす全部の書き直しの子（設計 docs/design/case-lifecycle.md §21・行 k・接頭辞 `seat_ruling_rewrites_lifecycle_`） ───────────

/// 裏の子の撃ちを待つ期限。撃たないことを測る側も同じ期限まで待つ。
const DEADLINE: Duration = Duration::from_secs(10);

/// 期限の内に `done` が真になるか。
fn within(mut done: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + DEADLINE;
    while Instant::now() < end {
        if done() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    done()
}

impl Fake {
    /// 印の在る置き場にする（repo の `.beads` の下に files の形の台帳 `issues.jsonl` を置く）。
    fn marked(self) -> Self {
        let beads = self.repo.join(".beads");
        fs::create_dir_all(&beads).ok();
        fixture(&beads, "issues.jsonl", "{}\n");
        self
    }

    /// 置き場に局面の出力の fixture（`render_output` で組む `lifecycle.json`）を置く。
    fn output_put(&self) {
        use vessel::fleet::lifecycle::{render_output, Output as Lifecycle, Owned, Scope};
        use vessel::fleet::lifecycle_mark::{Events, Ledger, Marks};
        let stamp = "2026-10-01T10:00:00Z".to_owned();
        let marks = Marks { ledger: Ledger::Files { len: 1, mtime_ns: 1 }, events: Events { len: 0, head: None }, main: LC_MAIN.to_owned() };
        let out = Lifecycle {
            generated_at: stamp.clone(),
            scope: Scope::Full,
            full_at: stamp,
            interval_s: Some(600),
            closed_window_h: Some(72),
            marks,
            unmeasured: Vec::new(),
            owned: Owned::default(),
            parts: Vec::new(),
        };
        let dir = self.state.join("fleet");
        fs::create_dir_all(&dir).ok();
        fs::write(dir.join("lifecycle.json"), render_output(&out)).ok();
    }

    /// 開いた問い `question` と、その発話 `TS_A`（chat）を置く。
    fn asked(&self, question: &str) {
        self.say(TS_A, Channel::Chat, WORDS);
        self.show(question, &open_question("2026-09-30T06:00:00Z", None));
    }

    /// 偽の bd への `--readonly list` の撃ち（全部の書き直しの子の撃ち）の数。
    fn listed(&self) -> usize {
        let is_list = |call: &Vec<String>| call.first().is_some_and(|word| word == "--readonly") && call.get(1).is_some_and(|word| word == "list");
        self.calls().iter().filter(|call| is_list(call)).count()
    }

    /// 子の `--readonly list` が返った（`list.ended`）か。
    fn ended(&self) -> bool {
        self.dir.join("list.ended").exists()
    }

    /// `seat ruling bind` を argv[0] を `arg0` にして撃つ。
    #[expect(clippy::expect_used, reason = "統合 test の helper。clippy の allow-expect-in-tests は #[test] 関数の中だけに効く")]
    fn bind_as(&self, question: &str, arg0: &str) -> Output {
        use std::os::unix::process::CommandExt;
        let (repo, state) = (self.repo.display().to_string(), self.state.display().to_string());
        Command::new(bin())
            .arg0(arg0)
            .args(["seat", "ruling", "bind", "--repo", &repo, "--state-dir", &state, "--question", question, "--utterance", TS_A, "--bd", &self.bd])
            .output()
            .expect("binary を起動できる")
    }

    /// 置き場の古さの印のうち種類 `kind` のもの（印の file が読めなければ空）。
    fn stale_of(&self, kind: vessel::fleet::lifecycle_mark::Kind) -> Vec<vessel::fleet::lifecycle_mark::Mark> {
        match vessel::fleet::lifecycle_mark::read_stale(&self.state) {
            vessel::fleet::lifecycle_mark::Stale::Marks(found) => found.into_iter().filter(|mark| mark.kind == kind).collect(),
            _ => Vec::new(),
        }
    }
}

/// (a) 印の在る置き場で開いた問いを結ぶと、rc 0・stdout は既存の結びの歯と同じ 1 行・stderr 0 byte で、期限の内に close の後の
/// `--readonly list` の撃ちが現れ、`list.rulings` が 1（裁定 event の後）。
#[test]
fn seat_ruling_rewrites_lifecycle_in_a_detached_child_after_a_bind() {
    let fake = Fake::new().marked();
    fake.asked("s2-q1");
    let out = fake.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), bound_line("s2-q1", TS_A, "chat"), "既存の結びの歯と同じ 1 行");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
    assert!(within(|| fake.ended()), "期限の内に子が list を撃つ");
    assert_eq!(fake.seen("list.rulings"), "1", "子の読みは裁定 event の後");
    let calls = fake.calls();
    let close = calls.iter().position(|call| call.first().is_some_and(|word| word == "close"));
    let list = calls.iter().position(|call| call.get(1).is_some_and(|word| word == "list"));
    assert!(close.is_some() && close < list, "list は close の後: {calls:?}");
}

/// (b) 印の在る置き場に `list.hold` を置いて答えを撃つと、`list.go` を置く前に rc 0・裁定 id の 1 行・stderr 0 byte で wall 10 秒未満で返る。
/// 返った後に `list.go` を置くと、期限の内に `list.ended` が現れ `list.rulings` が 1。
#[test]
fn seat_ruling_rewrites_lifecycle_without_waiting_for_the_child_after_an_answer() {
    let fake = Fake::new().marked();
    fixture(&fake.dir, "list.hold", "");
    fake.show("s2-q1", &open_question("2026-09-30T06:00:00Z", Some("user")));
    let start = Instant::now();
    let out = fake.answer("s2-q1", ANSWER_WORDS);
    let wall = start.elapsed();
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    let said = fake.utterances();
    let id = answered_id("s2-q1", said.first().map_or("", |found| found.ts.as_str()));
    assert_eq!(stdout_of(&out), format!("{id}\n"), "裁定 id の 1 行");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
    assert!(wall < Duration::from_secs(10), "子を待たずに返る: {wall:?}");
    assert!(!fake.ended(), "go の前に子は返らない");
    fixture(&fake.dir, "list.go", "");
    assert!(within(|| fake.ended()), "go の後に期限の内に子が返る");
    assert_eq!(fake.seen("list.rulings"), "1", "子の読みは裁定 event の後");
}

/// (c) 印の在る置き場で、閉じた問いの結び・空白だけの答え・close を 1 回落とした結びは、期限まで待っても `--readonly list` の撃ちが 0 で、
/// stderr は今の 1 行だけ。同じ歯の撃ち直しの結び（rc 0）は期限の内に list を撃つ。印の無い置き場（出力の fixture は在る）の結びは rc 0 で、
/// 期限まで待っても `unreadable` の印が無く、その後に口を通さず撃った `fleet lifecycle write` は理由 `ledger` の印を 1 つ付ける。
#[test]
fn seat_ruling_rewrites_lifecycle_does_not_start_a_child_for_a_refused_or_unreadable_round() {
    use vessel::fleet::lifecycle_mark::{Kind, Value};
    let fake = Fake::new().marked();
    fake.asked("s2-q3");
    fake.show("s2-q1", &show_json("closed", &["intake:question"], "2026-09-30T06:00:00Z", None, ""));
    let closed = fake.bind("s2-q1", TS_A);
    assert_eq!((rc_of(&closed), stderr_of(&closed)), (i32::from(RC_REFUSED), refused("closed", "s2-q1", TS_A)), "閉じた問い");
    let empty = fake.answer("s2-q3", "  \n");
    let words_empty = "seat ruling: refused reason=words-empty question=s2-q3\n".to_owned();
    assert_eq!((rc_of(&empty), stderr_of(&empty)), (i32::from(RC_REFUSED), words_empty), "空白だけの答え");
    fixture(&fake.dir, "close.fail", "");
    let half = fake.bind("s2-q3", TS_A);
    let partial = "seat ruling: partial stage=close id=s2-q3:20260930T0705Z-1 question=s2-q3 utterance=2026-09-30T07:05:09.123Z\n";
    assert_eq!((rc_of(&half), stderr_of(&half)), (i32::from(RC_REFUSED), partial.to_owned()), "途中の止まり");
    let bare = Fake::new();
    bare.output_put();
    bare.asked("s2-q1");
    let unbound = bare.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&unbound), i32::from(RC_OK), "印の無い置き場: stderr={}", stderr_of(&unbound));
    sleep(DEADLINE);
    assert_eq!(fake.listed(), 0, "Ok でない周は子を起こさない");
    assert!(bare.stale_of(Kind::Unreadable).is_empty(), "印を読めない周は子を起こさない");
    let (repo, state) = (bare.repo.display().to_string(), bare.state.display().to_string());
    let direct = Command::new(bin()).args(["fleet", "lifecycle", "write", "--state-dir", &state, "--repo", &repo]).output();
    assert!(direct.is_ok(), "口を通さず撃てる");
    let reasons: Vec<Value> = bare.stale_of(Kind::Unreadable).into_iter().map(|found| found.value).collect();
    assert_eq!(reasons, [Value::Reason("ledger".to_owned())], "子が起きれば見える印（前提）");
    fake.show("s2-q3", &show_json("open", &["intake:question"], "2026-09-30T06:00:00Z", None, &fake.notes()));
    let again = fake.bind("s2-q3", TS_A);
    assert_eq!(rc_of(&again), i32::from(RC_OK), "撃ち直し: stderr={}", stderr_of(&again));
    assert!(within(|| fake.listed() >= 1), "撃ち直しの結びは期限の内に list を撃つ");
}

/// (d) 印の在る置き場に出力の fixture を置き、argv[0] を在らない path にして結ぶと、子を起こせず、rc 0・stdout・stderr は (a) と同じで、
/// `ledger-gate` の印が 1 つ在り、値は結ぶ前の台帳の印と等しく結んだ後の印と違う。期限まで待っても list は 0。本物の argv[0] の写しは
/// 期限の内に list を撃ち、`ledger-gate` の印が無い。
#[test]
fn seat_ruling_rewrites_lifecycle_marks_the_ledger_gate_when_the_child_cannot_start() {
    use vessel::fleet::lifecycle_mark::{read_ledger, Kind, Value};
    let fake = Fake::new().marked();
    fake.output_put();
    fake.asked("s2-q1");
    let before = read_ledger(&fake.repo);
    assert!(before.is_some(), "前提: 結ぶ前の台帳の印を読める");
    let out = fake.bind_as("s2-q1", "/nonexistent/scribe2-gone");
    assert_eq!(rc_of(&out), i32::from(RC_OK), "stderr={}", stderr_of(&out));
    assert_eq!(stdout_of(&out), bound_line("s2-q1", TS_A, "chat"), "(a) と同じ 1 行");
    assert!(stderr_of(&out).is_empty(), "stderr 0 byte");
    let gates = fake.stale_of(Kind::LedgerGate);
    let value = gates.first().map(|found| found.value.clone());
    assert_eq!((gates.len(), value), (1, before.clone().map(Value::Ledger)), "印は結ぶ前の台帳の印");
    assert_ne!(read_ledger(&fake.repo), before, "結んだ後の台帳の印とは違う");
    sleep(DEADLINE);
    assert_eq!(fake.listed(), 0, "子を起こせなかった周は list を撃たない");
    let twin = Fake::new().marked();
    twin.output_put();
    twin.asked("s2-q1");
    let real = twin.bind("s2-q1", TS_A);
    assert_eq!(rc_of(&real), i32::from(RC_OK), "stderr={}", stderr_of(&real));
    assert!(within(|| twin.ended()), "本物の argv[0] は期限の内に list を撃つ");
    assert!(twin.stale_of(Kind::LedgerGate).is_empty(), "起こせた周は印を足さない");
}
