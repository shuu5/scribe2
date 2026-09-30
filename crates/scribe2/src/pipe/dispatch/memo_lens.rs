//! memo の審査の裏の process（設計 docs/design/dispatcher.md §41・契約表の行 ap・FR87 / FR36 / NFR6・ADR-0085）。
//!
//! `pipe dispatch memo-lens <memo> --state-dir S --repo R --lens CMD [--rules P] [--bd B]` は、memo 1 本の置き場
//! （[`super::memo::dir`]）に撃ち中の印 `pid` を排他で置き、材料 `material` を書き、便用の規則の lens の選定で口座を選び、
//! lens（`--stage memo`）を終わりまで待って、最後の JSON の 1 行を `verdict` に rename で書き、event `MemoJudged` を 1 行足す。
//! 台帳は読むだけで書かず、memo を閉じない（ADR-0085）。口座の候補が無い周と測れない周は lens を撃たず、`rc` に理由の 1 語を
//! 書いて `verdict` を書かない（前の判定の時刻は動かない）。

use super::memo::{self, Verdict, Word};
use super::{Input, Materials, Read};
use crate::cli_outcome::{Outcome, RC_BROKEN, RC_REFUSED};
use crate::fleet::store::{self, lock_owner, started_ms, LockPolicy, Owner};
use crate::fleet::{Case, Event, EventKind, SCHEMA};
use crate::headless::fill;
use crate::invocation::Invocation;
use crate::ledger::form::{is_memo, is_question};
use crate::pipe::ratelimit::{select_lens_account, LensAccount, Pool};
use crate::pipe::spawn::with_account;
use crate::seat::ledger;
use std::collections::BTreeSet;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::process::Stdio;

/// 材料の file の名（置き場の下）。
const MATERIAL: &str = "material";

/// 一時の verdict の file の名（rename で [`memo::VERDICT`] にする）。
const VERDICT_TMP: &str = "verdict.tmp";

/// 席の pane の変数（lens は起こす側の環境を継承し、これだけを外す・prelens と同じ）。
const PANE_ENV: &str = "TMUX_PANE";

/// 起動行の末尾に足す段の字（lens の段 memo）。
const STAGE: [&str; 2] = ["--stage", crate::headless::lens::STAGE_MEMO];

/// `rc` に書く理由の語（lens を撃たず `verdict` を書かない周）。
const ACCOUNT_NONE: &str = "account-none";
const ACCOUNT_UNMEASURED: &str = "account-unmeasured";
const LEDGER_UNREADABLE: &str = "ledger-unreadable";
const MEMO_NONE: &str = "memo-none";

/// discovered-from の依存の種別の字。
const DISCOVERED_FROM: &str = "discovered-from";

/// `memo-lens` の 1 回。`input` は列の材料（置き場・repo・規則・`--bd`・`--lens`）で、memo の id は `args` の 3 語目。
pub fn run(input: &Input<'_>, args: &[String], policy: LockPolicy) -> Outcome {
    let Some(memo) = args.get(2).filter(|found| !found.starts_with("--")) else {
        return Outcome::failed_line(RC_REFUSED, "pipe: memo-lens に memo の id が要る".to_owned());
    };
    let Some(lens) = input.lens else {
        return Outcome::failed_line(RC_REFUSED, "pipe: memo-lens に --lens が要る".to_owned());
    };
    if memo.contains(['/', '\\']) || memo == ".." || memo == "." {
        return Outcome::failed_line(RC_REFUSED, format!("pipe: memo の id {memo} は置き場の名にできない"));
    }
    let place = memo::dir(input.state_dir, memo);
    if let Err(reason) = claim(&place) {
        return Outcome::failed_line(RC_REFUSED, format!("pipe: memo-lens memo={memo} {reason}"));
    }
    let outcome = judge(input, args, (memo, lens), &place, policy);
    let _ = std::fs::write(place.join(memo::RC), format!("{}\n", outcome.0));
    let _ = std::fs::remove_file(place.join(memo::PID));
    outcome.1
}

/// 撃ち中の印を排他で置く（`<pid> <起動時刻>`）。生きた持ち主（か読めない持ち主）が在れば `Err`・死んだ持ち主の印は外して 1 度だけ取り直す。
fn claim(place: &Path) -> Result<(), String> {
    std::fs::create_dir_all(place).map_err(|err| format!("置き場を作れない: {err}"))?;
    let path = place.join(memo::PID);
    for _ in 0..2 {
        match std::fs::OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                let pid = std::process::id();
                let mark = started_ms(pid).started().map_or_else(|| format!("{pid}\n"), |at| format!("{pid} {at}\n"));
                return file.write_all(mark.as_bytes()).map_err(|err| format!("撃ち中の印を書けない: {err}"));
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                let body = std::fs::read_to_string(&path).unwrap_or_default();
                if lock_owner(&body, started_ms) != Owner::Dead {
                    return Err("は撃ち中である（生きた持ち主の pid が在る）".to_owned());
                }
                let _ = std::fs::remove_file(&path);
            }
            Err(err) => return Err(format!("撃ち中の印を置けない: {err}")),
        }
    }
    Err("撃ち中の印を取れない".to_owned())
}

/// 材料を書き・口座を選び・lens を撃って判定を残す。返りは `rc` の file に書く語と、process の結果。
fn judge(input: &Input<'_>, args: &[String], (memo, lens): (&str, &str), place: &Path, policy: LockPolicy) -> (String, Outcome) {
    let stop = |word: &str, detail: String| (word.to_owned(), Outcome::failed_line(RC_REFUSED, format!("pipe: memo-lens memo={memo} {word}: {detail}")));
    let now = crate::seat::state::now_secs();
    let Some(timeout) = ledger::timeout_of(input.manifest) else {
        return stop(LEDGER_UNREADABLE, "台帳の待ちの rules 行を読めない".to_owned());
    };
    let Ok(issues) = ledger::read_ledger(input.bd, input.repo, timeout) else {
        return stop(LEDGER_UNREADABLE, "台帳を読めない".to_owned());
    };
    let read = Read { issues, materials: Materials::of(input.repo, input.manifest, input.bd), events: None, unreflected: None };
    let Some(trigger) = memo::trigger_value(input, &read, now, memo) else {
        return stop(MEMO_NONE, "開いた memo が台帳に無い".to_owned());
    };
    let material = place.join(MATERIAL);
    if let Err(err) = std::fs::write(&material, material_of(&read, memo, &trigger)) {
        return stop("material", format!("{} を書けない: {err}", material.display()));
    }
    let filled = fill(lens, &[("{contract}", &material.display().to_string()), ("{worktree}", &input.repo.display().to_string())]);
    let line = match account_line(input, args, filled) {
        Ok(found) => found,
        Err((word, detail)) => return stop(word, detail),
    };
    let ran = Invocation::new("sh")
        .args(["-c", &format!("{line} {} {}", STAGE[0], STAGE[1])])
        .current_dir(input.repo)
        .env_remove(PANE_ENV)
        .stdin(Stdio::null())
        .output();
    let (rc, verdict) = match ran {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let _ = std::fs::write(place.join(memo::OUT), &text);
            (output.status.code().map_or_else(|| "signal".to_owned(), |code| code.to_string()), verdict_of(output.status.code(), &text))
        }
        Err(err) => ("spawn".to_owned(), unparsed(format!("lens を起こせない: {err}"))),
    };
    (rc, record(input, memo, &verdict, policy))
}

/// 宣言した口座が在る周は便用の規則の lens の選定で口座を選び、起動行の末尾に足す（宣言の無い周は行を変えない）。
fn account_line(input: &Input<'_>, args: &[String], line: String) -> Result<String, (&'static str, String)> {
    let pool = match Pool::declared(args, input.manifest, input.state_dir) {
        Ok(Some(found)) => found,
        Ok(None) => return Ok(line),
        Err(reason) => return Err((ACCOUNT_UNMEASURED, reason)),
    };
    let mut notes = Vec::new();
    let label = match select_lens_account(&pool, input.state_dir, input.repo, &mut notes) {
        Ok(LensAccount::Chosen(label)) => label,
        Ok(LensAccount::None(reason)) => return Err((ACCOUNT_NONE, format!("lens の口座の候補が無い（{reason}）"))),
        Err(reason) => return Err((ACCOUNT_UNMEASURED, reason)),
    };
    with_account(line, Some(&label), input.state_dir).map_err(|refusal| (ACCOUNT_UNMEASURED, refusal.to_string()))
}

/// lens の出力の最後の JSON の 1 行から判定を読む（JSON が無い・語の外・lens の rc が 0 でない周は unparsed）。
fn verdict_of(code: Option<i32>, text: &str) -> Verdict {
    if code != Some(0) {
        return unparsed(format!("lens の rc が 0 でない（{}）", code.map_or_else(|| "signal".to_owned(), |found| found.to_string())));
    }
    let Some(last) = text.lines().rev().map(str::trim).find(|line| line.starts_with('{')) else {
        return unparsed("lens の出力に JSON の行が無い".to_owned());
    };
    let Ok(pairs) = crate::fleet::json_lite::parse_object(last) else {
        return unparsed("lens の出力の最後の JSON を読めない".to_owned());
    };
    let get = |key: &str| pairs.iter().find(|(found, _)| found == key).and_then(|(_, value)| value.as_str()).unwrap_or_default().to_owned();
    let Some(word) = [Word::Promote, Word::Close, Word::Keep].into_iter().find(|found| found.as_str() == get("verdict")) else {
        return unparsed(format!("verdict {:?} は promote・close・keep のどれでもない", get("verdict")));
    };
    let sketch = if word == Word::Promote { get("sketch") } else { String::new() };
    Verdict { word, at: crate::fleet::cli::now_utc(), evidence: get("evidence"), sketch }
}

/// 読めなかった周の判定（理由は evidence）。
fn unparsed(reason: String) -> Verdict {
    Verdict { word: Word::Unparsed, at: crate::fleet::cli::now_utc(), evidence: reason, sketch: String::new() }
}

/// `verdict` を一時 file から rename で書き、`MemoJudged` を 1 行足し、stdout の 1 行を返す。
fn record(input: &Input<'_>, memo: &str, verdict: &Verdict, policy: LockPolicy) -> Outcome {
    let place = memo::dir(input.state_dir, memo);
    let (tmp, done) = (place.join(VERDICT_TMP), place.join(memo::VERDICT));
    if let Err(err) = std::fs::write(&tmp, format!("{}\n", verdict.to_line())).and_then(|()| std::fs::rename(&tmp, &done)) {
        return Outcome::failed_line(RC_BROKEN, format!("pipe: memo-lens memo={memo} verdict を書けない: {err}"));
    }
    let event = Event {
        schema: SCHEMA,
        ts: crate::fleet::cli::now_utc(),
        kind: EventKind::MemoJudged,
        run: String::new(),
        bead: memo.to_owned(),
        host: crate::fleet::cli::host(),
        actor: EventKind::MemoJudged.default_actor().to_owned(),
        stage: None,
        seat: None,
        pid: None,
        detail: Some(verdict.word.as_str().to_owned()),
        allowance: None,
        registration: None,
        mark: None,
        account: None,
        cost: None,
        rule: None,
        case: Some(Case::Judged),
    };
    if let Err(err) = store::append(input.state_dir, &event, policy) {
        return Outcome::failed_line(RC_BROKEN, format!("pipe: memo-lens memo={memo} MemoJudged を記帳できない: {err}"));
    }
    Outcome::ok_line(format!("memo-lens memo={memo} verdict={}", verdict.word.as_str()))
}

/// 審査の材料（memo の description・notes・引き金の読み・discovered-from で辿れる契約の id と status）。
fn material_of(read: &Read, memo: &str, trigger: &str) -> String {
    let own = read.issues.iter().find(|found| found.id == memo);
    let traced = traced(read, memo);
    let contracts: Vec<String> = read
        .issues
        .iter()
        .filter(|found| traced.contains(found.id.as_str()) && !is_memo(found) && !is_question(found))
        .map(|found| format!("- {} status={}", found.id, found.status))
        .collect();
    let listed = if contracts.is_empty() { "（無い）".to_owned() } else { contracts.join("\n") };
    format!(
        "# memo {memo}\n\n## description\n{}\n\n## notes\n{}\n\n## 引き金の読み\n{trigger}\n\n## discovered-from で辿れる契約\n{listed}\n",
        own.map_or("", |found| found.description.as_str()),
        own.map_or("", |found| found.notes.as_str()),
    )
}

/// memo から discovered-from の依存で辿れる bead の id（向きは問わず・memo 自身を除く）。
fn traced<'a>(read: &'a Read, memo: &'a str) -> BTreeSet<&'a str> {
    let mut seen: BTreeSet<&str> = BTreeSet::from([memo]);
    let mut frontier = vec![memo];
    while let Some(at) = frontier.pop() {
        for found in &read.issues {
            let forward = found.id == at;
            for dep in found.deps.iter().filter(|dep| dep.kind == DISCOVERED_FROM) {
                let next = if forward { Some(dep.on.as_str()) } else { (dep.on == at).then_some(found.id.as_str()) };
                if let Some(next) = next.filter(|next| seen.insert(next)) {
                    frontier.push(next);
                }
            }
        }
    }
    seen.remove(memo);
    seen
}
